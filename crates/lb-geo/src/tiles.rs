//! Tile providers, the on-disk tile cache and viewport tile selection.

use crate::mercator::{TILE_SIZE, TileId, Viewport, world_size};
use image::{ImageEncoder, Rgb, RgbImage, codecs::png::PngEncoder};
use lb_core::Clock;
use parking_lot::Mutex;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

/// How long a cached tile is served before it is fetched again (7 days).
pub const CACHE_TTL: Duration = Duration::from_secs(7 * 24 * 3600);

/// Why a tile could not be produced.
#[derive(Debug, thiserror::Error)]
pub enum GeoError {
    /// The tile coordinates are outside the grid.
    #[error("tile {z}/{x}/{y} is outside the map")]
    BadTile {
        /// Zoom level.
        z: u8,
        /// Column.
        x: u32,
        /// Row.
        y: u32,
    },
    /// The provider could not deliver the tile.
    #[error("could not fetch tile: {0}")]
    Fetch(String),
    /// Reading or writing the cache failed.
    #[error("tile cache: {0}")]
    Io(#[from] io::Error),
}

/// Checks that `tile` lies inside the grid.
pub(crate) fn check_tile(tile: TileId) -> Result<(), GeoError> {
    if tile.z > 22 || tile.x >= 1 << tile.z || tile.y >= 1 << tile.z {
        return Err(GeoError::BadTile {
            z: tile.z,
            x: tile.x,
            y: tile.y,
        });
    }
    Ok(())
}

/// A source of map tile images (PNG bytes).
pub trait TileProvider: Send + Sync {
    /// Fetches one tile.
    fn fetch(&self, tile: TileId) -> Result<Vec<u8>, GeoError>;
}

/// Serves generated PNG tiles without any network; tests and offline use.
#[derive(Debug, Default)]
pub struct LocalTileProvider {
    requests: Mutex<Vec<TileId>>,
}

impl LocalTileProvider {
    /// A provider that has served nothing yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Every tile requested so far, in order.
    pub fn requests(&self) -> Vec<TileId> {
        self.requests.lock().clone()
    }
}

impl TileProvider for LocalTileProvider {
    fn fetch(&self, tile: TileId) -> Result<Vec<u8>, GeoError> {
        check_tile(tile)?;
        self.requests.lock().push(tile);
        let shade = ((tile.x * 31 + tile.y * 17) % 96) as u8 + 120;
        let img = RgbImage::from_fn(TILE_SIZE as u32, TILE_SIZE as u32, |x, y| {
            if x == 0 || y == 0 {
                Rgb([90, 90, 90])
            } else {
                Rgb([shade, 200, 160 + tile.z])
            }
        });
        let mut out = Vec::new();
        PngEncoder::new(&mut out)
            .write_image(
                img.as_raw(),
                img.width(),
                img.height(),
                image::ExtendedColorType::Rgb8,
            )
            .map_err(|e| GeoError::Fetch(e.to_string()))?;
        Ok(out)
    }
}

/// A tile placed in a viewport.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PlacedTile {
    /// The tile (columns wrapped around the antimeridian).
    pub tile: TileId,
    /// Pixel position of its left edge in the viewport.
    pub x: f64,
    /// Pixel position of its top edge in the viewport.
    pub y: f64,
}

/// The tiles needed to cover `viewport`, row by row.
pub fn tiles_for_viewport(viewport: &Viewport) -> Vec<PlacedTile> {
    let n = 1i64 << viewport.zoom;
    let (ox, oy) = viewport.origin();
    let first_col = (ox / TILE_SIZE).floor() as i64;
    let last_col = ((ox + viewport.width) / TILE_SIZE).ceil() as i64 - 1;
    let first_row = ((oy / TILE_SIZE).floor() as i64).max(0);
    let last_row = (((oy + viewport.height) / TILE_SIZE).ceil() as i64 - 1).min(n - 1);
    let mut out = Vec::new();
    for row in first_row..=last_row {
        for col in first_col..=last_col {
            out.push(PlacedTile {
                tile: TileId {
                    z: viewport.zoom,
                    x: col.rem_euclid(n) as u32,
                    y: row as u32,
                },
                x: col as f64 * TILE_SIZE - ox,
                y: row as f64 * TILE_SIZE - oy,
            });
        }
    }
    debug_assert!(world_size(viewport.zoom) > 0.0);
    out
}

/// Disk cache in front of a provider. A tile younger than the TTL is served from disk.
pub struct TileCache {
    dir: PathBuf,
    provider: Arc<dyn TileProvider>,
    clock: Arc<dyn Clock>,
    ttl: Duration,
}

impl TileCache {
    /// Caches tiles of `provider` under `dir`.
    pub fn new(
        dir: impl Into<PathBuf>,
        provider: Arc<dyn TileProvider>,
        clock: Arc<dyn Clock>,
        ttl: Duration,
    ) -> Self {
        Self {
            dir: dir.into(),
            provider,
            clock,
            ttl,
        }
    }

    fn path(&self, tile: TileId) -> PathBuf {
        self.dir
            .join(tile.z.to_string())
            .join(tile.x.to_string())
            .join(format!("{}.tile", tile.y))
    }

    fn now(&self) -> i64 {
        self.clock.now().and_utc().timestamp()
    }

    /// The cached bytes and the time they were fetched.
    fn read(&self, tile: TileId) -> Option<(i64, Vec<u8>)> {
        let raw = std::fs::read(self.path(tile)).ok()?;
        let (stamp, body) = raw.split_at_checked(8)?;
        let stamp = i64::from_be_bytes(stamp.try_into().ok()?);
        Some((stamp, body.to_vec()))
    }

    /// Whether `tile` is cached and still fresh.
    pub fn is_fresh(&self, tile: TileId) -> bool {
        self.read(tile)
            .is_some_and(|(at, _)| self.now() - at < self.ttl.as_secs() as i64)
    }

    /// The tile image: from disk when fresh, otherwise fetched and stored. When the provider
    /// fails, an expired copy is still better than nothing.
    pub fn get(&self, tile: TileId) -> Result<Vec<u8>, GeoError> {
        check_tile(tile)?;
        let cached = self.read(tile);
        if let Some((at, body)) = &cached
            && self.now() - at < self.ttl.as_secs() as i64
        {
            return Ok(body.clone());
        }
        match self.provider.fetch(tile) {
            Ok(body) => {
                self.store(tile, &body)?;
                Ok(body)
            }
            Err(e) => cached.map(|(_, body)| body).ok_or(e),
        }
    }

    fn store(&self, tile: TileId, body: &[u8]) -> io::Result<()> {
        let path = self.path(tile);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut raw = self.now().to_be_bytes().to_vec();
        raw.extend_from_slice(body);
        let tmp = path.with_extension("tmp");
        std::fs::write(&tmp, raw)?;
        std::fs::rename(&tmp, &path)
    }

    /// The cache folder.
    pub fn dir(&self) -> &Path {
        &self.dir
    }
}
