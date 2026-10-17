//! Web Mercator projection (EPSG:3857) over 256 px tiles.

use lb_core::GeoPoint;

/// Edge of one map tile in pixels.
pub const TILE_SIZE: f64 = 256.0;
/// Lowest zoom level the map offers.
pub const MIN_ZOOM: u8 = 2;
/// Highest zoom level the map offers.
pub const MAX_ZOOM: u8 = 18;
/// Latitude beyond which the projection is cut off.
pub const MAX_LATITUDE: f64 = 85.051_128_779_806_59;

/// A tile of the slippy-map grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TileId {
    /// Zoom level.
    pub z: u8,
    /// Column, `0..2^z`.
    pub x: u32,
    /// Row, `0..2^z`, north to south.
    pub y: u32,
}

/// Width and height in pixels of the whole world at zoom `z`.
pub fn world_size(z: u8) -> f64 {
    TILE_SIZE * 2f64.powi(i32::from(z))
}

/// World pixel coordinates of `point` at zoom `z` (origin at the north-west corner).
pub fn lonlat_to_world(point: GeoPoint, z: u8) -> (f64, f64) {
    let size = world_size(z);
    let lat = point.lat.clamp(-MAX_LATITUDE, MAX_LATITUDE).to_radians();
    let x = (point.lon + 180.0) / 360.0 * size;
    let y = (1.0 - (lat.tan() + 1.0 / lat.cos()).ln() / std::f64::consts::PI) / 2.0 * size;
    (x, y)
}

/// The position under world pixel `(x, y)` at zoom `z`.
pub fn world_to_lonlat(x: f64, y: f64, z: u8) -> GeoPoint {
    let size = world_size(z);
    let lon = x / size * 360.0 - 180.0;
    let n = std::f64::consts::PI * (1.0 - 2.0 * y / size);
    GeoPoint {
        lat: n.sinh().atan().to_degrees(),
        lon,
    }
}

/// The tile holding `point` at zoom `z`.
pub fn tile_at(point: GeoPoint, z: u8) -> TileId {
    let (x, y) = lonlat_to_world(point, z);
    let max = (1u32 << z) - 1;
    TileId {
        z,
        x: ((x / TILE_SIZE).floor().max(0.0) as u32).min(max),
        y: ((y / TILE_SIZE).floor().max(0.0) as u32).min(max),
    }
}

/// The north-west corner of `tile`.
pub fn tile_nw_corner(tile: TileId) -> GeoPoint {
    world_to_lonlat(
        f64::from(tile.x) * TILE_SIZE,
        f64::from(tile.y) * TILE_SIZE,
        tile.z,
    )
}

/// A map window: what is centred, how far in, and how many pixels it covers.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Viewport {
    /// Position at the centre of the window.
    pub center: GeoPoint,
    /// Zoom level, [`MIN_ZOOM`]..=[`MAX_ZOOM`].
    pub zoom: u8,
    /// Window width in pixels.
    pub width: f64,
    /// Window height in pixels.
    pub height: f64,
}

impl Viewport {
    /// World pixel coordinates of the window's top-left corner.
    pub fn origin(&self) -> (f64, f64) {
        let (cx, cy) = lonlat_to_world(self.center, self.zoom);
        (cx - self.width / 2.0, cy - self.height / 2.0)
    }

    /// Window pixel position of `point`, relative to the top-left corner.
    pub fn project(&self, point: GeoPoint) -> (f64, f64) {
        let (ox, oy) = self.origin();
        let (x, y) = lonlat_to_world(point, self.zoom);
        let size = world_size(self.zoom);
        let mut dx = x - ox;
        // Take the copy of the world that is closest to the window.
        if dx < -size / 2.0 + self.width / 2.0 {
            dx += size;
        } else if dx > size / 2.0 + self.width / 2.0 {
            dx -= size;
        }
        (dx, y - oy)
    }

    /// The position under window pixel `(x, y)`.
    pub fn unproject(&self, x: f64, y: f64) -> GeoPoint {
        let (ox, oy) = self.origin();
        let size = world_size(self.zoom);
        let wx = (ox + x).rem_euclid(size);
        world_to_lonlat(wx, (oy + y).clamp(0.0, size), self.zoom)
    }

    /// The window moved by a drag of `(dx, dy)` pixels (the map follows the pointer).
    pub fn panned(&self, dx: f64, dy: f64) -> Self {
        let (cx, cy) = lonlat_to_world(self.center, self.zoom);
        let size = world_size(self.zoom);
        let center = world_to_lonlat(
            (cx - dx).rem_euclid(size),
            (cy - dy).clamp(0.0, size),
            self.zoom,
        );
        Self { center, ..*self }
    }

    /// The window zoomed by `delta` levels keeping the point under pixel `(x, y)` fixed.
    pub fn zoomed(&self, delta: i8, x: f64, y: f64) -> Self {
        let zoom = (i16::from(self.zoom) + i16::from(delta))
            .clamp(i16::from(MIN_ZOOM), i16::from(MAX_ZOOM)) as u8;
        if zoom == self.zoom {
            return *self;
        }
        let anchor = self.unproject(x, y);
        let (ax, ay) = lonlat_to_world(anchor, zoom);
        let (cx, cy) = (ax - x + self.width / 2.0, ay - y + self.height / 2.0);
        let size = world_size(zoom);
        Self {
            center: world_to_lonlat(cx.rem_euclid(size), cy.clamp(0.0, size), zoom),
            zoom,
            ..*self
        }
    }
}
