//! Map math, clustering and tile cache (SPEC §6.14).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use lb_core::{GeoPoint, PhotoId};
use lb_geo::{
    CACHE_TTL, LocalTileProvider, MAX_LATITUDE, Marker, TileCache, TileId, TileProvider, Viewport,
    cluster_markers, lonlat_to_world, tile_at, tile_nw_corner, world_size, world_to_lonlat,
};
use lb_testkit::FakeClock;
use proptest::prelude::*;
use std::sync::Arc;
use std::time::Duration;

fn p(lat: f64, lon: f64) -> GeoPoint {
    GeoPoint { lat, lon }
}

proptest! {
    #[test]
    fn mercator_roundtrip(lat in -MAX_LATITUDE..MAX_LATITUDE, lon in -180.0f64..180.0, z in 0u8..=20) {
        let (x, y) = lonlat_to_world(p(lat, lon), z);
        let back = world_to_lonlat(x, y, z);
        prop_assert!((back.lat - lat).abs() < 1e-9, "lat {} vs {}", back.lat, lat);
        prop_assert!((back.lon - lon).abs() < 1e-9, "lon {} vs {}", back.lon, lon);
    }

    #[test]
    fn cluster_counts_sum_to_markers(
        pts in proptest::collection::vec((-85.0f64..85.0, -180.0f64..180.0), 0..200),
        z in 2u8..=18,
    ) {
        let markers: Vec<Marker> = pts
            .iter()
            .enumerate()
            .map(|(i, (lat, lon))| Marker { id: PhotoId::new(i as i64), point: p(*lat, *lon) })
            .collect();
        let clusters = cluster_markers(&markers, z, 60.0);
        let total: usize = clusters.iter().map(|c| c.count()).sum();
        prop_assert_eq!(total, markers.len());
        let mut seen: Vec<i64> = clusters.iter().flat_map(|c| c.ids.iter().map(|i| i.get())).collect();
        seen.sort_unstable();
        seen.dedup();
        prop_assert_eq!(seen.len(), markers.len(), "every marker is in exactly one cluster");
        prop_assert!(clusters.len() <= markers.len());
    }
}

#[test]
fn mercator_known_values() {
    let (x, y) = lonlat_to_world(p(0.0, 0.0), 0);
    assert!((x - 128.0).abs() < 1e-9 && (y - 128.0).abs() < 1e-9);
    assert_eq!(world_size(3), 2048.0);
    assert_eq!(tile_at(p(0.0, -179.9), 1), TileId { z: 1, x: 0, y: 1 });
    assert_eq!(
        tile_at(p(51.5, -0.12), 10),
        TileId {
            z: 10,
            x: 511,
            y: 340
        }
    );
    let nw = tile_nw_corner(TileId { z: 1, x: 0, y: 0 });
    assert!((nw.lon + 180.0).abs() < 1e-9 && (nw.lat - MAX_LATITUDE).abs() < 1e-6);
    let poles = tile_at(p(90.0, 0.0), 2);
    assert_eq!(poles.y, 0, "latitude is clamped to the projection");
}

#[test]
fn tiles_for_viewport() {
    let vp = |lat, lon, zoom, w, h| Viewport {
        center: p(lat, lon),
        zoom,
        width: w,
        height: h,
    };
    let tiles = lb_geo::tiles_for_viewport(&vp(0.0, 0.0, 1, 512.0, 512.0));
    let ids: Vec<_> = tiles.iter().map(|t| (t.tile.x, t.tile.y)).collect();
    assert_eq!(ids, [(0, 0), (1, 0), (0, 1), (1, 1)]);
    assert!(tiles[0].x.abs() < 1e-9 && tiles[0].y.abs() < 1e-9);
    assert!((tiles[3].x - 256.0).abs() < 1e-9);

    let one = lb_geo::tiles_for_viewport(&vp(0.0, 0.0, 2, 256.0, 256.0));
    assert_eq!(
        one.len(),
        4,
        "a window centred on a tile corner touches four tiles"
    );

    let wide = lb_geo::tiles_for_viewport(&vp(0.0, 179.5, 2, 1024.0, 256.0));
    assert!(
        wide.iter().any(|t| t.tile.x == 0) && wide.iter().any(|t| t.tile.x == 3),
        "columns wrap around the antimeridian: {wide:?}"
    );
    assert!(wide.iter().all(|t| t.tile.x < 4));

    let polar = lb_geo::tiles_for_viewport(&vp(85.0, 0.0, 3, 2048.0, 2048.0));
    assert!(
        polar.iter().all(|t| t.tile.y < 8),
        "rows never leave the grid"
    );

    let sized = lb_geo::tiles_for_viewport(&vp(10.0, 20.0, 12, 800.0, 600.0));
    for t in &sized {
        assert!(t.x > -256.0 && t.x < 800.0 && t.y > -256.0 && t.y < 600.0);
    }
    assert!(
        (9..=20).contains(&sized.len()),
        "800x600 spans between 3x3 and 5x4 tiles"
    );
}

#[test]
fn viewport_projection_inverts_and_pans() {
    let vp = Viewport {
        center: p(48.85, 2.35),
        zoom: 8,
        width: 900.0,
        height: 600.0,
    };
    let (x, y) = vp.project(vp.center);
    assert!((x - 450.0).abs() < 1e-6 && (y - 300.0).abs() < 1e-6);
    let at = vp.unproject(100.0, 50.0);
    let (bx, by) = vp.project(at);
    assert!((bx - 100.0).abs() < 1e-6 && (by - 50.0).abs() < 1e-6);

    let panned = vp.panned(-50.0, 20.0);
    let (px, py) = panned.project(vp.center);
    assert!((px - 400.0).abs() < 1e-6 && (py - 320.0).abs() < 1e-6);

    let zoomed = vp.zoomed(2, 100.0, 50.0);
    assert_eq!(zoomed.zoom, 10);
    let (zx, zy) = zoomed.project(at);
    assert!(
        (zx - 100.0).abs() < 1e-3 && (zy - 50.0).abs() < 1e-3,
        "anchor stays put"
    );
    assert_eq!(vp.zoomed(-10, 0.0, 0.0).zoom, 2, "clamped to the minimum");
    assert_eq!(vp.zoomed(50, 0.0, 0.0).zoom, 18, "clamped to the maximum");
}

#[test]
fn cluster_merges_nearby_markers_only() {
    let m = |i, lat, lon| Marker {
        id: PhotoId::new(i),
        point: p(lat, lon),
    };
    let markers = [m(1, 10.0, 10.0), m(2, 10.0001, 10.0001), m(3, -30.0, 100.0)];
    let low = cluster_markers(&markers, 12, 60.0);
    assert_eq!(low.len(), 2);
    assert_eq!(low[0].count(), 2);
    let far = cluster_markers(&markers, 2, 60.0);
    assert!(far.len() <= 2);
    assert_eq!(cluster_markers(&[], 5, 60.0), vec![]);
}

#[test]
fn tile_cache_ttl() {
    let dir = tempfile::tempdir().unwrap();
    let provider = Arc::new(LocalTileProvider::new());
    let clock = Arc::new(FakeClock::new());
    let cache = TileCache::new(dir.path(), provider.clone(), clock.clone(), CACHE_TTL);
    let tile = TileId { z: 3, x: 2, y: 5 };

    let first = cache.get(tile).unwrap();
    assert_eq!(&first[..4], b"\x89PNG");
    assert_eq!(provider.requests().len(), 1);
    assert!(cache.is_fresh(tile));

    clock.advance(Duration::from_secs(6 * 24 * 3600));
    assert_eq!(cache.get(tile).unwrap(), first);
    assert_eq!(
        provider.requests().len(),
        1,
        "served from disk inside the TTL"
    );

    clock.advance(Duration::from_secs(2 * 24 * 3600));
    assert!(!cache.is_fresh(tile));
    cache.get(tile).unwrap();
    assert_eq!(provider.requests().len(), 2, "refetched after seven days");
    assert!(cache.is_fresh(tile));

    let reopened = TileCache::new(dir.path(), provider.clone(), clock, CACHE_TTL);
    reopened.get(tile).unwrap();
    assert_eq!(provider.requests().len(), 2, "the cache survives a restart");
    assert!(
        cache.get(TileId { z: 2, x: 4, y: 0 }).is_err(),
        "outside the grid"
    );
}

#[test]
fn stale_tile_is_served_when_the_provider_fails() {
    struct Down;
    impl TileProvider for Down {
        fn fetch(&self, _: TileId) -> Result<Vec<u8>, lb_geo::GeoError> {
            Err(lb_geo::GeoError::Fetch("offline".into()))
        }
    }
    let dir = tempfile::tempdir().unwrap();
    let clock = Arc::new(FakeClock::new());
    let tile = TileId { z: 4, x: 1, y: 1 };
    let warm = TileCache::new(
        dir.path(),
        Arc::new(LocalTileProvider::new()),
        clock.clone(),
        CACHE_TTL,
    );
    let bytes = warm.get(tile).unwrap();
    clock.advance(CACHE_TTL * 2);
    let offline = TileCache::new(dir.path(), Arc::new(Down), clock, CACHE_TTL);
    assert_eq!(offline.get(tile).unwrap(), bytes);
    assert!(offline.get(TileId { z: 4, x: 2, y: 2 }).is_err());
}

#[cfg(feature = "http")]
#[test]
fn http_provider_builds_urls_without_touching_the_network() {
    let provider = lb_geo::HttpTileProvider::new("https://tiles.example/{z}/{x}/{y}.png");
    assert_eq!(
        provider.url(TileId { z: 5, x: 17, y: 11 }),
        "https://tiles.example/5/17/11.png"
    );
    assert!(lb_geo::USER_AGENT.starts_with("Lightbox/"));
    assert_eq!(lb_geo::MAX_CONCURRENT_REQUESTS, 2);
}
