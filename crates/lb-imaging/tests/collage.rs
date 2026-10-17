//! Collage layouts (SPEC §6.13): goldens `tests/goldens/collage/`, geometry properties.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use image::{Rgba, RgbaImage};
use lb_imaging::collage::{CollageLayout, CollageSpec, Placement, layout, render};
use lb_testkit::assert_golden;
use proptest::prelude::*;

/// A lossless test photo: a two-colour diagonal gradient with a bar whose position tells photos
/// apart, so a golden shows which photo went where and how it was cropped.
fn photo(i: u32, w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let t = (x * 255 / w.max(1) + y * 255 / h.max(1)) / 2;
        let base = [
            (40 + i * 53) % 256,
            (200 + i * 29) % 256,
            (90 + i * 97) % 256,
        ];
        let bar = x * 10 / w.max(1) == i % 10;
        let mix = |c: u32| {
            if bar {
                255 - c
            } else {
                (c * (128 + t / 2) / 255).min(255)
            }
        };
        Rgba([
            mix(base[0]) as u8,
            mix(base[1]) as u8,
            mix(base[2]) as u8,
            255,
        ])
    })
}

fn photos(dims: &[(u32, u32)]) -> Vec<RgbaImage> {
    dims.iter()
        .enumerate()
        .map(|(i, (w, h))| photo(i as u32, *w, *h))
        .collect()
}

#[test]
fn golden_collage_grid_4() {
    let imgs = photos(&[(300, 200), (200, 300), (240, 240), (320, 240)]);
    let spec = CollageSpec::new(CollageLayout::Grid, 480, 320);
    assert_golden("collage", "golden_collage_grid_4", &render(&spec, &imgs));
}

#[test]
fn golden_collage_mosaic_7() {
    let imgs = photos(&[
        (300, 200),
        (200, 300),
        (240, 240),
        (320, 180),
        (180, 320),
        (400, 200),
        (250, 200),
    ]);
    let spec = CollageSpec::new(CollageLayout::Mosaic, 600, 400);
    assert_golden("collage", "golden_collage_mosaic_7", &render(&spec, &imgs));
}

#[test]
fn golden_collage_pile_5_seed_42() {
    let imgs = photos(&[(300, 200), (200, 300), (240, 240), (320, 240), (280, 200)]);
    let spec = CollageSpec {
        seed: 42,
        background: [48, 52, 60],
        ..CollageSpec::new(CollageLayout::Pile, 600, 400)
    };
    assert_golden(
        "collage",
        "golden_collage_pile_5_seed_42",
        &render(&spec, &imgs),
    );
}

#[test]
fn golden_collage_frame_6() {
    let imgs = photos(&[
        (400, 300),
        (200, 300),
        (240, 240),
        (320, 240),
        (280, 200),
        (300, 300),
    ]);
    let spec = CollageSpec::new(CollageLayout::FrameMosaic, 600, 400);
    assert_golden("collage", "golden_collage_frame_6", &render(&spec, &imgs));
}

#[test]
fn collage_output_dimensions_exact() {
    for layout in [
        CollageLayout::Grid,
        CollageLayout::Mosaic,
        CollageLayout::Pile,
        CollageLayout::FrameMosaic,
    ] {
        for n in [2_usize, 5, 13] {
            for (w, h) in [(640, 360), (300, 300), (361, 517)] {
                let dims: Vec<(u32, u32)> = (0..n)
                    .map(|i| if i % 2 == 0 { (60, 40) } else { (40, 60) })
                    .collect();
                let spec = CollageSpec::new(layout, w, h);
                let out = render(&spec, &photos(&dims));
                assert_eq!(out.dimensions(), (w, h), "{layout:?} n={n}");
            }
        }
    }
}

#[test]
fn collage_deterministic_with_seed() {
    let imgs = photos(&[(300, 200), (200, 300), (240, 240), (320, 240), (280, 200)]);
    for layout in [
        CollageLayout::Grid,
        CollageLayout::Mosaic,
        CollageLayout::Pile,
    ] {
        let spec = CollageSpec {
            seed: 42,
            ..CollageSpec::new(layout, 400, 300)
        };
        assert_eq!(render(&spec, &imgs), render(&spec, &imgs), "{layout:?}");
    }
    let a = CollageSpec {
        seed: 42,
        ..CollageSpec::new(CollageLayout::Pile, 400, 300)
    };
    let b = CollageSpec {
        seed: 43,
        ..a.clone()
    };
    assert_ne!(
        render(&a, &imgs),
        render(&b, &imgs),
        "another seed shuffles the pile"
    );
}

fn rect_of(p: &Placement) -> (f32, f32, f32, f32) {
    p.bounds()
}

fn overlaps(a: &Placement, b: &Placement) -> bool {
    let (al, at, ar, ab) = rect_of(a);
    let (bl, bt, br, bb) = rect_of(b);
    al < br - 0.01 && bl < ar - 0.01 && at < bb - 0.01 && bt < ab - 0.01
}

proptest! {
    #[test]
    fn collage_layout_never_exceeds_canvas(
        layout_ix in 0_usize..4,
        aspects in proptest::collection::vec(0.2_f32..5.0, 2..=200),
        w in 100_u32..3000,
        h in 100_u32..3000,
        spacing in 0_u32..40,
        seed in any::<u64>(),
        frame_index in 0_usize..200,
    ) {
        let layout_kind = [
            CollageLayout::Grid,
            CollageLayout::Mosaic,
            CollageLayout::Pile,
            CollageLayout::FrameMosaic,
        ][layout_ix];
        let spec = CollageSpec {
            spacing,
            seed,
            frame_index,
            ..CollageSpec::new(layout_kind, w, h)
        };
        let placed = layout(&spec, &aspects);
        prop_assert_eq!(placed.len(), aspects.len());
        let mut seen = vec![false; aspects.len()];
        for p in &placed {
            prop_assert!(!seen[p.index], "index {} placed twice", p.index);
            seen[p.index] = true;
            let (l, t, r, b) = p.bounds();
            prop_assert!(p.w >= 1.0 && p.h >= 1.0);
            prop_assert!(l >= -0.01 && t >= -0.01, "{:?} {:?}", layout_kind, p);
            prop_assert!(r <= w as f32 + 0.01 && b <= h as f32 + 0.01, "{:?} {:?} canvas {}x{}", layout_kind, p, w, h);
        }
        if layout_kind != CollageLayout::Pile {
            for (i, a) in placed.iter().enumerate() {
                for b in &placed[i + 1..] {
                    prop_assert!(!overlaps(a, b), "{:?}: {:?} overlaps {:?}", layout_kind, a, b);
                }
            }
        }
    }
}
