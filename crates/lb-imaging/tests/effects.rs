//! Effect op behaviour (SPEC §6.6.3, §6.6.4).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use image::Rgba;
use lb_core::{EditOp, NormRect};
use lb_imaging::Rgba32FImage;
use lb_imaging::color::luma;
use lb_imaging::ops::{OpCtx, apply_op, effects};
use proptest::prelude::*;

fn run(img: &Rgba32FImage, op: &EditOp) -> Rgba32FImage {
    apply_op(img.clone(), op, &OpCtx { seed: 99, index: 0 })
}

fn flat(w: u32, h: u32, rgb: [f32; 3]) -> Rgba32FImage {
    Rgba32FImage::from_pixel(w, h, Rgba([rgb[0], rgb[1], rgb[2], 1.0]))
}

fn px(img: &Rgba32FImage, x: u32, y: u32) -> [f32; 4] {
    img.get_pixel(x, y).0
}

fn assert_close(a: &[f32], b: &[f32], tol: f32) {
    for (x, y) in a.iter().zip(b) {
        assert!((x - y).abs() <= tol, "{a:?} vs {b:?}");
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(160))]

    #[test]
    fn ops_never_produce_out_of_range(
        op in lb_core::strategies::edit_op(),
        w in 1_u32..48, h in 1_u32..48, seed in 0_u64..200,
    ) {
        let img = common::noise(w, h, seed);
        let out = apply_op(img.clone(), &op, &OpCtx { seed, index: 3 });
        prop_assert!(out.width() >= 1 && out.height() >= 1);
        for p in out.pixels() {
            for v in p.0 {
                prop_assert!(v.is_finite() && (0.0..=1.0).contains(&v), "{} gave {v}", op.name());
            }
        }
        if !matches!(op, EditOp::Crop { .. } | EditOp::RotateQuarter { .. } | EditOp::Straighten { .. }) {
            prop_assert_eq!(out.dimensions(), img.dimensions());
            for (a, b) in out.pixels().zip(img.pixels()) {
                prop_assert_eq!(a.0[3], b.0[3], "alpha preserved by {}", op.name());
            }
        }
    }
}

#[test]
fn ops_handle_extreme_inputs_without_going_out_of_range() {
    let mut img = Rgba32FImage::new(9, 7);
    for (i, p) in img.pixels_mut().enumerate() {
        *p = if i % 2 == 0 {
            Rgba([0.0, 0.0, 0.0, 1.0])
        } else {
            Rgba([1.0, 1.0, 1.0, 1.0])
        };
    }
    let ops = [
        EditOp::AutoContrast,
        EditOp::AutoColor,
        EditOp::FeelingLucky,
        EditOp::FillLight { amount: 1.0 },
        EditOp::Sharpen { amount: 2.0 },
        EditOp::Glow {
            intensity: 1.0,
            radius: 5.0,
        },
        EditOp::SoftFocus {
            cx: 0.0,
            cy: 0.0,
            radius: 0.0,
            amount: 1.0,
        },
        EditOp::Vignette {
            amount: 1.0,
            radius: 0.0,
        },
        EditOp::Tuning {
            fill_light: 1.0,
            highlights: 1.0,
            shadows: 1.0,
            temperature: 1.0,
            neutral: Some([0.0, 0.0, 0.0]),
        },
    ];
    for op in ops {
        for start in [&img, &flat(9, 7, [0.0; 3]), &flat(9, 7, [1.0; 3])] {
            let out = run(start, &op);
            assert!(
                out.as_raw()
                    .iter()
                    .all(|v| v.is_finite() && (0.0..=1.0).contains(v)),
                "{}",
                op.name()
            );
        }
    }
}

#[test]
fn sepia_matches_the_matrix() {
    let out = run(&flat(2, 2, [0.5, 0.5, 0.5]), &EditOp::Sepia);
    assert_close(&px(&out, 0, 0)[..3], &[0.6755, 0.6015, 0.4685], 1e-5);
    let white = run(&flat(2, 2, [1.0; 3]), &EditOp::Sepia);
    assert_close(&px(&white, 0, 0)[..3], &[1.0, 1.0, 0.937], 1e-5);
}

#[test]
fn black_and_white_uses_luma() {
    let out = run(&flat(1, 1, [1.0, 0.0, 0.0]), &EditOp::BlackAndWhite);
    assert_close(&px(&out, 0, 0)[..3], &[0.2126; 3], 1e-6);
    let img = common::portrait_patches_640x480();
    for p in run(&img, &EditOp::BlackAndWhite).pixels() {
        assert_eq!(p.0[0], p.0[1]);
        assert_eq!(p.0[1], p.0[2]);
    }
}

#[test]
fn saturation_endpoints() {
    let img = common::portrait_patches_640x480();
    assert_eq!(run(&img, &EditOp::Saturation { amount: 0.0 }), img);
    let grey = run(&img, &EditOp::Saturation { amount: -1.0 });
    let bw = run(&img, &EditOp::BlackAndWhite);
    for (a, b) in grey.as_raw().iter().zip(bw.as_raw()) {
        assert!((a - b).abs() < 1e-5);
    }
    let g = flat(2, 2, [0.4; 3]);
    assert_eq!(
        run(&g, &EditOp::Saturation { amount: 1.0 }),
        g,
        "grey stays grey"
    );
    let boosted = run(
        &flat(1, 1, [0.6, 0.4, 0.3]),
        &EditOp::Saturation { amount: 0.5 },
    );
    let l = luma(0.6, 0.4, 0.3);
    assert_close(
        &px(&boosted, 0, 0)[..3],
        &[
            l + (0.6 - l) * 1.5,
            l + (0.4 - l) * 1.5,
            l + (0.3 - l) * 1.5,
        ],
        1e-6,
    );
}

#[test]
fn warmify_is_temperature_then_saturation() {
    let img = common::portrait_patches_640x480();
    let warm = run(&img, &EditOp::Warmify);
    let manual = run(
        &run(
            &img,
            &EditOp::Tuning {
                fill_light: 0.0,
                highlights: 0.0,
                shadows: 0.0,
                temperature: 0.4,
                neutral: None,
            },
        ),
        &EditOp::Saturation { amount: 0.15 },
    );
    assert_eq!(warm, manual);
    let p = px(&run(&flat(1, 1, [0.5; 3]), &EditOp::Warmify), 0, 0);
    assert!(p[0] > p[2], "warmer: red above blue");
}

#[test]
fn tint_blends_between_original_and_coloured_luma() {
    let img = flat(2, 2, [0.5, 0.5, 0.5]);
    let keep = EditOp::Tint {
        color: [1.0, 0.5, 0.0],
        preserve: 1.0,
    };
    assert_eq!(run(&img, &keep), img);
    let full = run(
        &img,
        &EditOp::Tint {
            color: [1.0, 0.5, 0.0],
            preserve: 0.0,
        },
    );
    assert_close(&px(&full, 0, 0)[..3], &[0.5, 0.25, 0.0], 1e-6);
    let half = run(
        &img,
        &EditOp::Tint {
            color: [1.0, 0.5, 0.0],
            preserve: 0.5,
        },
    );
    assert_close(&px(&half, 0, 0)[..3], &[0.5, 0.375, 0.25], 1e-6);
}

#[test]
fn sharpen_properties() {
    let constant = flat(16, 16, [0.3, 0.5, 0.7]);
    let out = run(&constant, &EditOp::Sharpen { amount: 2.0 });
    assert_close(out.as_raw(), constant.as_raw(), 1e-5);
    let img = common::noise(32, 32, 4);
    assert_eq!(run(&img, &EditOp::Sharpen { amount: 0.0 }), img);

    let mut edge = flat(20, 4, [0.3; 3]);
    for y in 0..4 {
        for x in 10..20 {
            edge.put_pixel(x, y, Rgba([0.7, 0.7, 0.7, 1.0]));
        }
    }
    let out = run(&edge, &EditOp::Sharpen { amount: 1.0 });
    assert!(
        px(&out, 9, 1)[0] < 0.3 - 0.01,
        "undershoot on the dark side of the edge"
    );
    assert!(
        px(&out, 10, 1)[0] > 0.7 + 0.01,
        "overshoot on the bright side"
    );
    assert_close(&px(&out, 2, 1)[..3], &[0.3; 3], 1e-5);
    let exact = 0.3 + 1.0 * (0.3 - (0.3 * 0.5 + 0.7 * 0.5)); // sanity of the formula at the step
    assert!(exact < 0.3);
}

#[test]
fn film_grain_deterministic_with_seed() {
    let img = flat(40, 30, [0.5; 3]);
    let op = EditOp::FilmGrain { amount: 0.8 };
    let a = apply_op(img.clone(), &op, &OpCtx { seed: 7, index: 2 });
    let b = apply_op(img.clone(), &op, &OpCtx { seed: 7, index: 2 });
    assert_eq!(a, b, "same seed, same grain");
    let other_seed = apply_op(img.clone(), &op, &OpCtx { seed: 8, index: 2 });
    assert_ne!(a, other_seed);
    let other_index = apply_op(img.clone(), &op, &OpCtx { seed: 7, index: 3 });
    assert_ne!(a, other_index, "op index is mixed into the seed");
    assert_eq!(run(&img, &EditOp::FilmGrain { amount: 0.0 }), img);

    let mean: f32 = a.pixels().map(|p| p.0[0] - 0.5).sum::<f32>() / (40.0 * 30.0);
    assert!(mean.abs() < 0.01, "noise is roughly zero mean: {mean}");
    let max = a
        .pixels()
        .map(|p| (p.0[0] - 0.5).abs())
        .fold(0.0_f32, f32::max);
    assert!(
        max <= 0.8 * 0.12 + 1e-6 && max > 0.05,
        "amplitude bounded by amount*0.12: {max}"
    );
    for p in a.pixels() {
        assert_eq!(p.0[0], p.0[1], "same noise on every channel");
        assert_eq!(p.0[1], p.0[2]);
    }
    let black = flat(8, 8, [0.0; 3]);
    let white = flat(8, 8, [1.0; 3]);
    assert_eq!(run(&black, &op), black, "no grain at L=0");
    assert_eq!(run(&white, &op), white, "no grain at L=1");
}

#[test]
fn quick_key_seed_uses_the_first_eight_bytes() {
    assert_eq!(effects::seed_from_quick_key(""), 0);
    assert_eq!(
        effects::seed_from_quick_key("abcdefgh"),
        u64::from_le_bytes(*b"abcdefgh")
    );
    assert_eq!(
        effects::seed_from_quick_key("abcdefghIGNORED"),
        effects::seed_from_quick_key("abcdefgh")
    );
    assert_eq!(
        effects::seed_from_quick_key("ab"),
        u64::from(b'a') | (u64::from(b'b') << 8)
    );
}

#[test]
fn soft_focus_keeps_the_focus_area_and_blurs_the_rest() {
    let mut checker = flat(80, 80, [0.0; 3]);
    for y in 0..80 {
        for x in 0..80 {
            if (x + y) % 2 == 0 {
                checker.put_pixel(x, y, Rgba([1.0, 1.0, 1.0, 1.0]));
            }
        }
    }
    let op = EditOp::SoftFocus {
        cx: 0.5,
        cy: 0.5,
        radius: 0.2,
        amount: 1.0,
    };
    let out = run(&checker, &op);
    assert_eq!(px(&out, 40, 40), px(&checker, 40, 40), "centre untouched");
    let corner = px(&out, 2, 2)[0];
    assert!(
        (corner - 0.5).abs() < 0.15,
        "corner is blurred towards grey: {corner}"
    );
    let none = EditOp::SoftFocus {
        cx: 0.5,
        cy: 0.5,
        radius: 0.2,
        amount: 0.0,
    };
    assert_eq!(run(&checker, &none), checker);
}

#[test]
fn glow_never_darkens_and_ignores_dark_images() {
    let dark = flat(30, 30, [0.5, 0.3, 0.1]);
    let op = EditOp::Glow {
        intensity: 1.0,
        radius: 1.0,
    };
    assert_close(run(&dark, &op).as_raw(), dark.as_raw(), 1e-6);

    let img = common::portrait_patches_640x480();
    let out = run(&img, &op);
    for (a, b) in out.as_raw().iter().zip(img.as_raw()) {
        assert!(a + 1e-6 >= *b, "screen blend never darkens");
    }
    assert_ne!(out, img);
    assert_eq!(
        run(
            &img,
            &EditOp::Glow {
                intensity: 0.0,
                radius: 3.0
            }
        ),
        img
    );
}

#[test]
fn focal_black_and_white_keeps_colour_only_near_the_centre() {
    let img = common::portrait_patches_640x480();
    let op = EditOp::FocalBlackAndWhite {
        cx: 0.5,
        cy: 0.5,
        radius: 0.15,
        sharpness: 1.0,
    };
    let out = run(&img, &op);
    assert_eq!(
        px(&out, 320, 240),
        px(&img, 320, 240),
        "centre keeps its colour"
    );
    let far = px(&out, 5, 5);
    assert_eq!(far[0], far[1]);
    assert_eq!(far[1], far[2]);
    assert!((far[0] - luma(px(&img, 5, 5)[0], px(&img, 5, 5)[1], px(&img, 5, 5)[2])).abs() < 1e-6);
}

#[test]
fn graduated_tint_fades_from_top_to_bottom() {
    let img = flat(20, 100, [0.6; 3]);
    let op = EditOp::GraduatedTint {
        color: [1.0, 0.2, 0.2],
        y: 0.5,
        feather: 0.2,
        amount: 1.0,
    };
    let out = run(&img, &op);
    let top = px(&out, 10, 0);
    assert_close(&top[..3], &[0.6, 0.12, 0.12], 1e-4);
    assert_eq!(px(&out, 10, 99), px(&img, 10, 99), "bottom untouched");
    let mid = px(&out, 10, 50);
    assert!(
        (mid[1] - (0.6 + (0.12 - 0.6) * 0.5)).abs() < 0.02,
        "half tinted at y: {mid:?}"
    );
    assert!(px(&out, 10, 20)[1] < px(&out, 10, 60)[1]);
}

#[test]
fn vignette_darkens_corners_only() {
    let img = flat(100, 80, [0.8; 3]);
    let out = run(
        &img,
        &EditOp::Vignette {
            amount: 0.6,
            radius: 0.5,
        },
    );
    assert_eq!(px(&out, 50, 40), px(&img, 50, 40), "centre untouched");
    let corner = px(&out, 0, 0)[0];
    assert!(
        corner < 0.8 * 0.5,
        "corner darkened by about (1 - amount): {corner}"
    );
    assert!(corner > 0.8 * 0.35);
    assert!(px(&out, 20, 20)[0] > corner && px(&out, 20, 20)[0] <= 0.8);
    assert_eq!(
        run(
            &img,
            &EditOp::Vignette {
                amount: 0.0,
                radius: 0.5
            }
        ),
        img
    );
}

#[test]
fn red_eye_only_changes_red_pixels_inside_the_regions() {
    let mut img = flat(100, 100, [0.8, 0.6, 0.5]);
    for y in 25..45 {
        for x in 25..45 {
            img.put_pixel(x, y, Rgba([0.9, 0.1, 0.1, 1.0]));
        }
    }
    img.put_pixel(80, 80, Rgba([0.9, 0.1, 0.1, 1.0]));
    let region = NormRect {
        x: 0.25,
        y: 0.25,
        w: 0.2,
        h: 0.2,
    };
    let out = run(
        &img,
        &EditOp::RedEye {
            regions: vec![region],
        },
    );
    assert_close(&px(&out, 35, 35)[..3], &[0.1, 0.1, 0.1], 1e-6);
    assert_eq!(
        px(&out, 50, 50),
        px(&img, 50, 50),
        "skin tone is not red enough"
    );
    assert_eq!(px(&out, 80, 80), px(&img, 80, 80), "outside the region");
    assert_eq!(
        px(&out, 25, 25),
        px(&img, 25, 25),
        "corner of the box is outside the ellipse"
    );
    let none = run(&img, &EditOp::RedEye { regions: vec![] });
    assert_eq!(none, img);
}
