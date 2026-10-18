//! Ambient backdrop (SPEC-VIEWER §V6.2).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use image::{Rgba, RgbaImage};
use lb_imaging::ambient::{AmbientParams, render_ambient};
use lb_testkit::assert_golden;
use lb_testkit::fixtures::{manifest::Pattern, pattern};
use lb_testkit::perf::{check, median_of, scale};
use std::time::Duration;

fn solid(w: u32, h: u32, c: [u8; 3]) -> RgbaImage {
    RgbaImage::from_pixel(w, h, Rgba([c[0], c[1], c[2], 255]))
}

/// Portrait source (taller than wide) with a unique centre band, so a landscape cover-crop
/// keeps the centre and drops the top and bottom.
fn banded_portrait() -> RgbaImage {
    RgbaImage::from_fn(120, 240, |_x, y| {
        let c = if y < 60 {
            [220, 20, 20]
        } else if y >= 180 {
            [20, 20, 220]
        } else {
            [20, 220, 40]
        };
        Rgba([c[0], c[1], c[2], 255])
    })
}

#[test]
fn golden_ambient_portrait_1280x800() {
    let thumb = pattern::render(Pattern::Patches, 31, 480, 640);
    let out = render_ambient(&thumb, (1280, 800), &AmbientParams::default());
    assert_eq!(out.dimensions(), (320, 200));
    assert_golden("ambient", "golden_ambient_portrait_1280x800", &out);
}

#[test]
fn golden_ambient_landscape_1280x800() {
    let thumb = pattern::render(Pattern::Patches, 31, 640, 480);
    let out = render_ambient(&thumb, (1280, 800), &AmbientParams::default());
    assert_eq!(out.dimensions(), (320, 200));
    assert_golden("ambient", "golden_ambient_landscape_1280x800", &out);
}

#[test]
fn ambient_deterministic() {
    let thumb = pattern::render(Pattern::Scene, 7, 320, 200);
    let a = render_ambient(&thumb, (1280, 800), &AmbientParams::default());
    let b = render_ambient(&thumb, (1280, 800), &AmbientParams::default());
    assert_eq!(a, b);
}

#[test]
fn ambient_cover_crop_matches_stage_aspect() {
    let landscape = render_ambient(&banded_portrait(), (1280, 800), &AmbientParams::default());
    let (w, h) = landscape.dimensions();
    assert_eq!((w, h), (320, 200));
    let stage_aspect = 1280.0 / 800.0;
    let out_aspect = f64::from(w) / f64::from(h);
    assert!((out_aspect - stage_aspect).abs() < 0.02, "{out_aspect}");

    // The centre of a landscape crop of the banded portrait is the green band.
    let mid = landscape.get_pixel(w / 2, h / 2).0;
    assert!(
        mid[1] > mid[0] + 40 && mid[1] > mid[2] + 40,
        "centre should stay the green band, got {mid:?}"
    );

    let portrait_stage = render_ambient(
        &solid(200, 100, [180, 40, 40]),
        (800, 1280),
        &AmbientParams::default(),
    );
    assert_eq!(portrait_stage.dimensions(), (200, 320));
}

#[test]
fn ambient_brightness_cap() {
    let thumb = solid(64, 64, [255, 255, 255]);
    let out = render_ambient(&thumb, (1280, 800), &AmbientParams::default());
    let cap = 0.04 + 0.40 + 1.0 / 255.0;
    for px in out.pixels() {
        for channel in &px.0[..3] {
            let v = f32::from(*channel) / 255.0;
            assert!(
                v <= cap + 1e-4,
                "channel {channel} above the brightness cap"
            );
        }
        assert_eq!(px.0[3], 255);
    }
    // A white source is not crushed to black: the centre stays near the cap.
    let mid = out.get_pixel(out.width() / 2, out.height() / 2).0;
    assert!(
        mid[0] > 80,
        "centre of a white ambient is too dark: {mid:?}"
    );
}

#[test]
fn ambient_4k_under_budget() {
    let long = u32::try_from(scale(3840, 640)).unwrap();
    let short = u32::try_from(scale(2160, 360)).unwrap();
    let thumb = pattern::render(Pattern::Gradient, 3, 160, 90);
    let median = median_of(3, || {
        let out = render_ambient(&thumb, (long, short), &AmbientParams::default());
        assert_eq!(out.width(), long.div_ceil(4));
        assert_eq!(out.height(), short.div_ceil(4));
    });
    check("ambient_4k", median, Duration::from_millis(8));
}
