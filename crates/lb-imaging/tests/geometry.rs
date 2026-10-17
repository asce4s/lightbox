//! Geometry op properties (SPEC §6.6.3, §6.6.4).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use lb_core::EditOp;
use lb_imaging::ops::{OpCtx, apply_op, geometry};
use proptest::prelude::*;

fn run(img: lb_imaging::Rgba32FImage, op: &EditOp) -> lb_imaging::Rgba32FImage {
    apply_op(img, op, &OpCtx::default())
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(48))]

    #[test]
    fn rotate_four_quarters_is_identity(w in 1_u32..40, h in 1_u32..40, seed in 0_u64..1000, turns in prop_oneof![Just(-2_i8), Just(-1), Just(1), Just(2)]) {
        let img = common::noise(w, h, seed);
        let mut cur = img.clone();
        for _ in 0..4 {
            cur = run(cur, &EditOp::RotateQuarter { turns });
        }
        prop_assert_eq!(&cur, &img);
        let there = run(img.clone(), &EditOp::RotateQuarter { turns });
        let back = run(there, &EditOp::RotateQuarter { turns: -turns });
        prop_assert_eq!(&back, &img);
    }

    #[test]
    fn crop_full_is_identity(w in 1_u32..60, h in 1_u32..60, seed in 0_u64..1000) {
        let img = common::noise(w, h, seed);
        let out = run(img.clone(), &EditOp::Crop { x: 0.0, y: 0.0, w: 1.0, h: 1.0 });
        prop_assert_eq!(&out, &img);
    }

    #[test]
    fn crop_rect_is_always_inside_and_nonempty(
        w in 1_u32..500, h in 1_u32..500,
        x in 0.0_f32..1.0, y in 0.0_f32..1.0, cw in 0.001_f32..1.0, ch in 0.001_f32..1.0,
    ) {
        let (x0, y0, x1, y1) = geometry::crop_rect(w, h, x, y, cw, ch);
        prop_assert!(x0 < x1 && x1 <= w);
        prop_assert!(y0 < y1 && y1 <= h);
    }

    #[test]
    fn straighten_zero_is_identity(w in 1_u32..50, h in 1_u32..50, seed in 0_u64..1000) {
        let img = common::noise(w, h, seed);
        prop_assert_eq!(&run(img.clone(), &EditOp::Straighten { degrees: 0.0 }), &img);
    }

    #[test]
    fn straighten_never_samples_outside_the_source(
        w in 2_u32..4000, h in 2_u32..4000, degrees in -45.0_f32..=45.0,
    ) {
        let (ow, oh) = geometry::straighten_size(w, h, degrees);
        prop_assert!(ow <= w && oh <= h && ow >= 1 && oh >= 1);
        for (ox, oy) in [(0, 0), (ow - 1, 0), (0, oh - 1), (ow - 1, oh - 1)] {
            let (sx, sy) = geometry::straighten_source_point((w, h), (ow, oh), degrees, (f64::from(ox), f64::from(oy)));
            // Rounding the output size can overshoot by half a pixel at most.
            prop_assert!(sx >= -1.0 && sx <= f64::from(w) + 1.0, "x {sx} outside 0..{w}");
            prop_assert!(sy >= -1.0 && sy <= f64::from(h) + 1.0, "y {sy} outside 0..{h}");
        }
    }
}

#[test]
fn rotate_quarter_moves_corners_clockwise() {
    let mut img = lb_imaging::Rgba32FImage::new(3, 2);
    img.put_pixel(0, 0, image::Rgba([1.0, 0.0, 0.0, 1.0]));
    img.put_pixel(2, 1, image::Rgba([0.0, 1.0, 0.0, 1.0]));
    let cw = run(img.clone(), &EditOp::RotateQuarter { turns: 1 });
    assert_eq!(cw.dimensions(), (2, 3));
    assert_eq!(
        cw.get_pixel(1, 0).0,
        [1.0, 0.0, 0.0, 1.0],
        "top-left goes to top-right"
    );
    assert_eq!(
        cw.get_pixel(0, 2).0,
        [0.0, 1.0, 0.0, 1.0],
        "bottom-right goes to bottom-left"
    );
    let ccw = run(img.clone(), &EditOp::RotateQuarter { turns: -1 });
    assert_eq!(
        ccw.get_pixel(0, 2).0,
        [1.0, 0.0, 0.0, 1.0],
        "top-left goes to bottom-left"
    );
    let half = run(img, &EditOp::RotateQuarter { turns: 2 });
    assert_eq!(half.get_pixel(2, 1).0, [1.0, 0.0, 0.0, 1.0]);
}

#[test]
fn crop_uses_rounded_pixel_bounds() {
    let img = common::noise(10, 8, 1);
    // x0=round(2.5)=3 (half away from zero), x1=round(7.5)=8; y0=round(2)=2, y1=round(6)=6.
    let out = run(
        img.clone(),
        &EditOp::Crop {
            x: 0.25,
            y: 0.25,
            w: 0.5,
            h: 0.5,
        },
    );
    assert_eq!(out.dimensions(), (5, 4));
    assert_eq!(out.get_pixel(0, 0), img.get_pixel(3, 2));
    let tiny = run(
        img.clone(),
        &EditOp::Crop {
            x: 0.5,
            y: 0.5,
            w: 0.001,
            h: 0.001,
        },
    );
    assert_eq!(tiny.dimensions(), (1, 1), "minimum 1x1");
    let edge = run(
        img,
        &EditOp::Crop {
            x: 0.999,
            y: 0.999,
            w: 0.001,
            h: 0.001,
        },
    );
    assert_eq!(edge.dimensions(), (1, 1));
}

#[test]
fn straighten_output_size_follows_the_formula() {
    assert_eq!(geometry::straighten_size(100, 100, 0.0), (100, 100));
    // 45 degrees on a square: s = 1/(cos45+sin45) = 1/sqrt2.
    assert_eq!(geometry::straighten_size(100, 100, 45.0), (71, 71));
    assert_eq!(
        geometry::straighten_size(100, 100, -45.0),
        (71, 71),
        "sign does not matter"
    );
    let (w, h) = geometry::straighten_size(512, 512, 7.0);
    let s = 1.0 / (7.0_f64.to_radians().cos() + 7.0_f64.to_radians().sin());
    assert_eq!(
        (w, h),
        ((512.0 * s).round() as u32, (512.0 * s).round() as u32)
    );
    let (w, h) = geometry::straighten_size(1000, 100, 10.0);
    assert!(f64::from(w) / f64::from(h) > 9.9 && f64::from(w) / f64::from(h) < 10.1);
}

#[test]
fn straighten_rotates_clockwise_and_keeps_flat_colour() {
    let flat = lb_imaging::Rgba32FImage::from_pixel(40, 30, image::Rgba([0.2, 0.4, 0.6, 1.0]));
    let out = run(flat, &EditOp::Straighten { degrees: 12.0 });
    for p in out.pixels() {
        for (a, b) in p.0.iter().zip([0.2, 0.4, 0.6, 1.0]) {
            assert!((a - b).abs() < 1e-5);
        }
    }
    // A bright vertical bar through the centre tilts clockwise: its top moves right.
    let mut img = lb_imaging::Rgba32FImage::from_pixel(101, 101, image::Rgba([0.0, 0.0, 0.0, 1.0]));
    for y in 0..101 {
        img.put_pixel(50, y, image::Rgba([1.0, 1.0, 1.0, 1.0]));
    }
    let out = run(img, &EditOp::Straighten { degrees: 10.0 });
    let brightest_x = |y: u32| {
        (0..out.width())
            .max_by(|a, b| {
                out.get_pixel(*a, y).0[0]
                    .partial_cmp(&out.get_pixel(*b, y).0[0])
                    .unwrap()
            })
            .unwrap()
    };
    let (top, bottom) = (brightest_x(5), brightest_x(out.height() - 6));
    assert!(top > bottom, "top {top} should be right of bottom {bottom}");
}

#[test]
fn straighten_has_no_transparent_corners() {
    for degrees in [-45.0_f32, -17.3, -3.0, 0.5, 7.0, 30.0, 45.0] {
        let img = common::gradient_rgb_512();
        let out = run(img, &EditOp::Straighten { degrees });
        for (x, y) in [
            (0, 0),
            (out.width() - 1, 0),
            (0, out.height() - 1),
            (out.width() - 1, out.height() - 1),
        ] {
            assert!(
                (out.get_pixel(x, y).0[3] - 1.0).abs() < 1e-6,
                "{degrees}: corner ({x},{y}) is not opaque"
            );
        }
    }
    // Content check: a white image with a black one-pixel frame. The inscribed crop only
    // touches the rotated frame at its four corners, so only a thin sliver (a few pixels per corner) may be dark;
    // a rotation without the crop would show frame colour across whole corner wedges.
    let (w, h) = (200_u32, 120_u32);
    let mut img = lb_imaging::Rgba32FImage::from_pixel(w, h, image::Rgba([1.0, 1.0, 1.0, 1.0]));
    for x in 0..w {
        img.put_pixel(x, 0, image::Rgba([0.0, 0.0, 0.0, 1.0]));
        img.put_pixel(x, h - 1, image::Rgba([0.0, 0.0, 0.0, 1.0]));
    }
    for y in 0..h {
        img.put_pixel(0, y, image::Rgba([0.0, 0.0, 0.0, 1.0]));
        img.put_pixel(w - 1, y, image::Rgba([0.0, 0.0, 0.0, 1.0]));
    }
    for degrees in [3.0_f32, 10.0, 25.0, 45.0] {
        let out = run(img.clone(), &EditOp::Straighten { degrees });
        let dark = out.pixels().filter(|p| p.0[0] < 0.5).count();
        assert!(
            dark <= 64,
            "{degrees}: {dark} frame pixels leak into the crop"
        );
    }
}
