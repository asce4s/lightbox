//! Retouch behaviour (SPEC §6.6.3).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use image::Rgba;
use lb_core::{EditOp, RetouchSpot};
use lb_imaging::Rgba32FImage;
use lb_imaging::ops::{OpCtx, apply_op};

fn run(img: &Rgba32FImage, spots: Vec<RetouchSpot>) -> Rgba32FImage {
    let op = EditOp::Retouch { spots }.validate().unwrap();
    apply_op(img.clone(), &op, &OpCtx::default())
}

fn spot(src: (f32, f32), dst: (f32, f32), radius: f32) -> RetouchSpot {
    RetouchSpot {
        src_x: src.0,
        src_y: src.1,
        dst_x: dst.0,
        dst_y: dst.1,
        radius,
    }
}

fn px(img: &Rgba32FImage, x: u32, y: u32) -> [f32; 4] {
    img.get_pixel(x, y).0
}

#[test]
fn retouch_copies_the_source_disk_into_the_target() {
    let mut img = common::gradient_rgb_512();
    for y in 246..266 {
        for x in 246..266 {
            img.put_pixel(x, y, Rgba([0.0, 0.0, 0.0, 1.0]));
        }
    }
    let out = run(&img, vec![spot((0.75, 0.5), (0.5, 0.5), 0.05)]);
    let before = common::gradient_rgb_512();
    // The centre of the blemish now looks like the gradient 128 px to its right.
    let healed = px(&out, 256, 256);
    let source = px(&before, 256 + 128, 256);
    for c in 0..3 {
        assert!(
            (healed[c] - source[c]).abs() < 0.01,
            "{healed:?} vs {source:?}"
        );
    }
    assert_ne!(px(&out, 256, 256), px(&img, 256, 256));
    // Outside the disk (radius 25.6 px) nothing changes.
    assert_eq!(px(&out, 256 + 30, 256), px(&img, 256 + 30, 256));
    assert_eq!(px(&out, 0, 0), px(&img, 0, 0));
    assert_eq!(out.dimensions(), img.dimensions());
}

#[test]
fn retouch_feathers_the_edge() {
    let img = common::gradient_rgb_512();
    let out = run(&img, vec![spot((0.8, 0.8), (0.3, 0.3), 0.1)]);
    let (cx, cy) = (0.3_f32 * 512.0, 0.3_f32 * 512.0);
    let radius = 0.1_f32 * 512.0;
    let at = |d: f32| {
        let x = (cx + d) as u32;
        let (a, b) = (px(&out, x, cy as u32), px(&img, x, cy as u32));
        (a[0] - b[0]).abs() + (a[1] - b[1]).abs()
    };
    assert!(
        at(0.0) > at(radius * 0.95),
        "influence fades towards the rim"
    );
    assert!(at(radius * 0.95) > 0.0 || at(radius * 0.9) > 0.0);
    assert_eq!(at(radius + 2.0), 0.0);
}

#[test]
fn retouch_with_identical_source_and_target_is_a_no_op() {
    let img = common::noise(64, 48, 9);
    let out = run(&img, vec![spot((0.5, 0.5), (0.5, 0.5), 0.2)]);
    for (a, b) in out.as_raw().iter().zip(img.as_raw()) {
        assert!((a - b).abs() < 1e-5);
    }
}

#[test]
fn retouch_outside_bounds_clamped() {
    let img = common::noise(80, 60, 3);
    for spots in [
        vec![spot((0.0, 0.0), (1.0, 1.0), 0.5)],
        vec![spot((1.0, 0.5), (0.0, 0.5), 0.4)],
        vec![spot((0.5, 1.0), (0.5, 0.0), 0.3)],
        vec![spot((0.02, 0.02), (0.98, 0.02), 0.5)],
    ] {
        let out = run(&img, spots);
        assert_eq!(out.dimensions(), img.dimensions());
        for p in out.pixels() {
            assert!(p.0.iter().all(|v| v.is_finite() && (0.0..=1.0).contains(v)));
        }
    }

    // A source entirely beyond the right edge reads the clamped edge column.
    let mut tall = Rgba32FImage::from_pixel(40, 40, Rgba([0.0, 0.0, 0.0, 1.0]));
    for y in 0..40 {
        tall.put_pixel(39, y, Rgba([1.0, 0.5, 0.25, 1.0]));
    }
    let out = run(&tall, vec![spot((1.0, 0.5), (0.5, 0.5), 0.1)]);
    let p = px(&out, 20, 20);
    assert!(p[0] > 0.3, "centre picked up the edge colour: {p:?}");
    // Target disk partly outside the image only touches pixels that exist.
    let out = run(&tall, vec![spot((0.5, 0.5), (0.0, 0.0), 0.3)]);
    assert_eq!(out.dimensions(), (40, 40));
}

#[test]
fn spots_apply_in_order() {
    let mut img = Rgba32FImage::from_pixel(100, 20, Rgba([0.0, 0.0, 0.0, 1.0]));
    for y in 0..20 {
        for x in 0..20 {
            img.put_pixel(x, y, Rgba([1.0, 1.0, 1.0, 1.0]));
        }
    }
    // Copy the white block to x=50, then copy that copy to x=90: both become white.
    let out = run(
        &img,
        vec![
            spot((0.1, 0.5), (0.5, 0.5), 0.05),
            spot((0.5, 0.5), (0.9, 0.5), 0.05),
        ],
    );
    assert!(px(&out, 50, 10)[0] > 0.9);
    assert!(px(&out, 90, 10)[0] > 0.9);
}
