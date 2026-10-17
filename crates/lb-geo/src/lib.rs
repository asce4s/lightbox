//! Web-mercator math, marker clustering, tile cache and tile provider trait for the map view
//! (SPEC §6.14).
#![warn(missing_docs)]

mod cluster;
mod mercator;
mod tiles;

pub use cluster::{Cluster, Marker, cluster_markers};
pub use mercator::{
    MAX_LATITUDE, MAX_ZOOM, MIN_ZOOM, TILE_SIZE, TileId, Viewport, lonlat_to_world, tile_at,
    tile_nw_corner, world_size, world_to_lonlat,
};
pub use tiles::{
    CACHE_TTL, GeoError, LocalTileProvider, PlacedTile, TileCache, TileProvider, tiles_for_viewport,
};

#[cfg(feature = "http")]
mod http;
#[cfg(feature = "http")]
pub use http::{HttpTileProvider, MAX_CONCURRENT_REQUESTS, USER_AGENT};

/// Attribution text that must stay visible next to OpenStreetMap tiles.
pub const ATTRIBUTION: &str = "© OpenStreetMap contributors";
