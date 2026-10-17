//! M10.1: map services (SPEC §6.14).
#![cfg(feature = "map")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use lb_app::{LightboxOptions, MapService};
use lb_core::GeoPoint;
use lb_geo::{LocalTileProvider, TileId, Viewport};
use std::sync::Arc;

fn local_options() -> (Arc<LocalTileProvider>, LightboxOptions) {
    let provider = Arc::new(LocalTileProvider::new());
    let options = LightboxOptions {
        tile_provider: Some(provider.clone()),
        ..LightboxOptions::default()
    };
    (provider, options)
}

fn paris() -> GeoPoint {
    GeoPoint {
        lat: 48.8566,
        lon: 2.3522,
    }
}

#[test]
fn set_location_updates_catalog() {
    let t = common::library(3);
    let (lb, ids) = (&t.lb, &t.ids);
    let map: MapService = lb.map();
    assert_eq!(map.set_location(&ids[..2], paris()).unwrap(), 2);
    for id in &ids[..2] {
        assert_eq!(lb.photos().get(*id).unwrap().unwrap().gps, Some(paris()));
    }
    assert_eq!(lb.photos().get(ids[2]).unwrap().unwrap().gps, None);

    let mut view = lb_app::ViewQuery::new(lb_app::ViewSource::AllPhotos);
    view.chips.geotagged = true;
    let tagged = lb.search().query(&view).unwrap();
    assert_eq!(
        tagged.total, 2,
        "the geotagged filter sees the new location"
    );

    assert_eq!(map.clear_location(&ids[..1]).unwrap(), 1);
    assert_eq!(lb.photos().get(ids[0]).unwrap().unwrap().gps, None);
    assert!(
        map.set_location(
            ids,
            GeoPoint {
                lat: 95.0,
                lon: 0.0
            }
        )
        .is_err(),
        "out-of-range positions are rejected"
    );
    let original = std::fs::read(lb.photos().get(ids[1]).unwrap().unwrap().path).unwrap();
    assert!(!original.is_empty());
}

#[test]
fn markers_and_clusters_follow_the_geotagged_photos() {
    let t = common::library(4);
    let (lb, ids) = (&t.lb, &t.ids);
    let map = lb.map();
    map.set_location(&ids[..2], paris()).unwrap();
    map.set_location(
        &ids[2..3],
        GeoPoint {
            lat: -33.87,
            lon: 151.21,
        },
    )
    .unwrap();
    let markers = map.markers(ids).unwrap();
    assert_eq!(markers.len(), 3, "the fourth photo has no position");
    let vp = Viewport {
        center: paris(),
        zoom: 3,
        width: 800.0,
        height: 600.0,
    };
    let clusters = map.clusters(ids, &vp).unwrap();
    assert_eq!(clusters.iter().map(|c| c.count()).sum::<usize>(), 3);
    assert!(clusters.iter().any(|c| c.count() == 2));
}

#[test]
fn no_network_in_tests() {
    let (provider, options) = local_options();
    let t = common::build(&[("", 1)], options);
    let map = t.lb.map();
    assert!(map.uses_injected_provider());
    assert!(LightboxOptions::default().tile_provider.is_none());

    let tile = TileId { z: 4, x: 8, y: 5 };
    let bytes = map.tile(tile).unwrap();
    assert_eq!(&bytes[..4], b"\x89PNG");
    assert_eq!(provider.requests(), vec![tile]);
    map.tile(tile).unwrap();
    assert_eq!(
        provider.requests().len(),
        1,
        "second read comes from the disk cache"
    );

    let vp = Viewport {
        center: paris(),
        zoom: 5,
        width: 512.0,
        height: 512.0,
    };
    for placed in map.tiles(&vp) {
        map.tile(placed.tile).unwrap();
    }
    assert!(provider.requests().len() > 1);
    assert!(
        t.home.path().join("cache").join("tiles").is_dir(),
        "tiles are cached under the app cache dir"
    );
}

#[test]
fn vm_map_fit_pan_and_cluster_click() {
    use lb_app::vm::MapVm;
    let t = common::library(5);
    let (lb, ids) = (&t.lb, t.ids.clone());
    let map = lb.map();
    let rome = GeoPoint {
        lat: 41.9,
        lon: 12.5,
    };
    map.set_location(&ids[..2], paris()).unwrap();
    map.set_location(&ids[2..4], rome).unwrap();

    let mut vm = MapVm::new(lb, ids.clone(), 800.0, 600.0).unwrap();
    let total: usize = vm
        .clusters()
        .unwrap()
        .iter()
        .map(|c| c.cluster.count())
        .sum();
    assert_eq!(total, 4, "fit frames every geotagged photo");
    assert!(
        vm.viewport().zoom >= 3,
        "Paris and Rome fit at a regional zoom"
    );

    for _ in 0..20 {
        vm.zoom_by(1, 400.0, 300.0);
    }
    assert_eq!(vm.viewport().zoom, 18, "zoom is capped at 18");
    let clusters = vm.clusters().unwrap();
    assert!(
        clusters.len() <= 1,
        "zoomed onto the middle of nowhere, markers fall out of view"
    );

    vm.fit().unwrap();
    let clusters = vm.clusters().unwrap();
    assert_eq!(clusters.len(), 2, "two places, two clusters");
    let first = vm.cluster_photos(0).unwrap();
    assert_eq!(first.len(), 2);

    let before = vm.viewport();
    vm.pan(40.0, 0.0);
    let (b, a) = (before.project(paris()), vm.viewport().project(paris()));
    assert!((a.0 - b.0 - 40.0).abs() < 1e-6, "the map follows the drag");

    let target = vm.location_at(400.0, 300.0);
    assert_eq!(vm.set_location_at(&ids[4..], 400.0, 300.0).unwrap(), 1);
    let placed = lb.photos().get(ids[4]).unwrap().unwrap().gps.unwrap();
    assert!((placed.lat - target.lat).abs() < 1e-9 && (placed.lon - target.lon).abs() < 1e-9);

    let empty = MapVm::new(lb, vec![], 800.0, 600.0).unwrap();
    assert_eq!(empty.viewport().zoom, 2);
    assert!(!empty.tiles().is_empty());
}
