//! The default tile provider: OpenStreetMap-style tile servers over HTTPS.

use crate::mercator::TileId;
use crate::tiles::{GeoError, TileProvider, check_tile};
use parking_lot::{Condvar, Mutex};
use std::time::Duration;

/// Tile servers allow at most this many simultaneous connections per client.
pub const MAX_CONCURRENT_REQUESTS: usize = 2;
/// Identifies the application to the tile server (required by the OSM tile usage policy).
pub const USER_AGENT: &str = concat!(
    "Lightbox/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/asce4s/lightbox)"
);
const TIMEOUT: Duration = Duration::from_secs(15);
const MAX_TILE_BYTES: u64 = 2 * 1024 * 1024;

/// Fetches tiles from a URL template such as `https://tile.openstreetmap.org/{z}/{x}/{y}.png`.
pub struct HttpTileProvider {
    template: String,
    agent: ureq::Agent,
    slots: Mutex<usize>,
    freed: Condvar,
}

impl HttpTileProvider {
    /// A provider for `template` with `{z}`, `{x}` and `{y}` placeholders.
    pub fn new(template: impl Into<String>) -> Self {
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .user_agent(USER_AGENT)
            .timeout_global(Some(TIMEOUT))
            .build()
            .into();
        Self {
            template: template.into(),
            agent,
            slots: Mutex::new(0),
            freed: Condvar::new(),
        }
    }

    /// The URL of `tile`.
    pub fn url(&self, tile: TileId) -> String {
        self.template
            .replace("{z}", &tile.z.to_string())
            .replace("{x}", &tile.x.to_string())
            .replace("{y}", &tile.y.to_string())
    }
}

impl TileProvider for HttpTileProvider {
    fn fetch(&self, tile: TileId) -> Result<Vec<u8>, GeoError> {
        check_tile(tile)?;
        {
            let mut busy = self.slots.lock();
            while *busy >= MAX_CONCURRENT_REQUESTS {
                self.freed.wait(&mut busy);
            }
            *busy += 1;
        }
        let result = self
            .agent
            .get(&self.url(tile))
            .call()
            .map_err(|e| GeoError::Fetch(e.to_string()))
            .and_then(|mut response| {
                response
                    .body_mut()
                    .with_config()
                    .limit(MAX_TILE_BYTES)
                    .read_to_vec()
                    .map_err(|e| GeoError::Fetch(e.to_string()))
            });
        *self.slots.lock() -= 1;
        self.freed.notify_one();
        result
    }
}
