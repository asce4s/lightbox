//! Collage service (SPEC §6.13).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::library;
use lb_app::{CollageLayout, CollageOptions, CollageSize};

fn opts(layout: CollageLayout, w: u32, h: u32) -> CollageOptions {
    CollageOptions {
        layout,
        size: CollageSize::Custom {
            width: w,
            height: h,
        },
        ..CollageOptions::default()
    }
}

#[test]
fn collage_create_saves_exact_size_and_indexes() {
    let lib = library(4);
    let out = lib.photos.path().join("Collages").join("c.jpg");
    let made = lib
        .lb
        .collage()
        .create(&lib.ids, &opts(CollageLayout::Grid, 640, 360), &out)
        .unwrap();
    assert_eq!(made.path, out);
    assert_eq!(image::image_dimensions(&out).unwrap(), (640, 360));
    assert!(made.id.is_some(), "collage inside a root is indexed");
}

#[test]
fn collage_rejects_bad_photo_counts_and_sizes() {
    let lib = library(3);
    let svc = lib.lb.collage();
    let o = opts(CollageLayout::Mosaic, 200, 200);
    assert!(svc.render(&lib.ids[..1], &o).is_err());
    let many: Vec<_> = (0..201).map(|i| lib.ids[i % 3]).collect();
    assert!(svc.render(&many, &o).is_err());
    assert!(
        svc.render(&lib.ids, &opts(CollageLayout::Grid, 8, 200))
            .is_err()
    );
}

#[test]
fn collage_preview_is_capped_and_keeps_aspect() {
    let lib = library(3);
    let img = lib
        .lb
        .collage()
        .preview(&lib.ids, &opts(CollageLayout::Pile, 3000, 2000))
        .unwrap();
    assert_eq!(img.dimensions(), (1024, 683));
}
