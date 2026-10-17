//! Per-op golden tests (SPEC §6.6.4): `tests/goldens/<suite>/<name>.png`, tolerance max diff
//! 2/255 and PSNR >= 45 dB. Regenerate only per AGENTS.md §4.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use lb_core::EditOp;
use lb_imaging::{
    Rgba32FImage, convert,
    ops::{OpCtx, apply_op},
};
use lb_testkit::assert_golden;

fn golden(suite: &str, name: &str, fixture: Rgba32FImage, op: &EditOp) {
    let op = op.validate().unwrap();
    let out = apply_op(
        fixture,
        &op,
        &OpCtx {
            seed: 0x5eed,
            index: 0,
        },
    );
    assert_golden(suite, name, &convert::to_rgba8(&out));
}

#[test]
fn golden_straighten_7deg_gradient() {
    golden(
        "geometry",
        "golden_straighten_7deg_gradient",
        common::gradient_rgb_512(),
        &EditOp::Straighten { degrees: 7.0 },
    );
}

#[test]
fn golden_crop_center_portrait() {
    golden(
        "geometry",
        "golden_crop_center_portrait",
        common::portrait_patches_640x480(),
        &EditOp::Crop {
            x: 0.25,
            y: 0.25,
            w: 0.5,
            h: 0.5,
        },
    );
}

#[test]
fn golden_rotate_quarter_cw_portrait() {
    golden(
        "geometry",
        "golden_rotate_quarter_cw_portrait",
        common::portrait_patches_640x480(),
        &EditOp::RotateQuarter { turns: 1 },
    );
}

const LOW_CONTRAST: &str = "low_contrast";
const PORTRAIT: &str = "portrait";

fn fixture(name: &str) -> Rgba32FImage {
    match name {
        LOW_CONTRAST => common::low_contrast_400(),
        _ => common::portrait_patches_640x480(),
    }
}

fn auto_goldens(op_name: &str, op: &EditOp) {
    for fixture_name in [LOW_CONTRAST, PORTRAIT] {
        golden(
            "auto",
            &format!("golden_{op_name}_{fixture_name}"),
            fixture(fixture_name),
            op,
        );
    }
}

#[test]
fn golden_auto_contrast() {
    auto_goldens("auto_contrast", &EditOp::AutoContrast);
}

#[test]
fn golden_auto_color() {
    auto_goldens("auto_color", &EditOp::AutoColor);
}

#[test]
fn golden_feeling_lucky() {
    auto_goldens("feeling_lucky", &EditOp::FeelingLucky);
}

#[test]
fn golden_fill_light() {
    auto_goldens("fill_light", &EditOp::FillLight { amount: 0.6 });
}

#[test]
fn golden_tuning() {
    auto_goldens(
        "tuning",
        &EditOp::Tuning {
            fill_light: 0.3,
            highlights: 0.4,
            shadows: 0.2,
            temperature: 0.35,
            neutral: Some([0.62, 0.55, 0.47]),
        },
    );
}

const GRADIENT: &str = "gradient";

fn fixture_any(name: &str) -> Rgba32FImage {
    match name {
        GRADIENT => common::gradient_rgb_512(),
        LOW_CONTRAST => common::low_contrast_400(),
        _ => common::portrait_patches_640x480(),
    }
}

fn effect_goldens(op_name: &str, op: &EditOp, fixtures: &[&str]) {
    for fixture_name in fixtures {
        golden(
            "effects",
            &format!("golden_{op_name}_{fixture_name}"),
            fixture_any(fixture_name),
            op,
        );
    }
}

const BOTH: [&str; 2] = [GRADIENT, PORTRAIT];

#[test]
fn golden_sharpen() {
    effect_goldens(
        "sharpen",
        &EditOp::Sharpen { amount: 1.5 },
        &[LOW_CONTRAST, PORTRAIT],
    );
}

#[test]
fn golden_sepia() {
    effect_goldens("sepia", &EditOp::Sepia, &BOTH);
}

#[test]
fn golden_black_and_white() {
    effect_goldens("black_and_white", &EditOp::BlackAndWhite, &BOTH);
}

#[test]
fn golden_warmify() {
    effect_goldens("warmify", &EditOp::Warmify, &BOTH);
}

#[test]
fn golden_film_grain() {
    let img = common::crop_160x120(&common::portrait_patches_640x480());
    golden(
        "effects",
        "golden_film_grain_portrait_crop",
        img,
        &EditOp::FilmGrain { amount: 0.7 },
    );
}

#[test]
fn golden_tint() {
    effect_goldens(
        "tint",
        &EditOp::Tint {
            color: [1.0, 0.65, 0.3],
            preserve: 0.25,
        },
        &BOTH,
    );
}

#[test]
fn golden_saturation() {
    effect_goldens("saturation_up", &EditOp::Saturation { amount: 0.6 }, &BOTH);
    effect_goldens(
        "saturation_down",
        &EditOp::Saturation { amount: -0.6 },
        &BOTH,
    );
}

#[test]
fn golden_soft_focus() {
    effect_goldens(
        "soft_focus",
        &EditOp::SoftFocus {
            cx: 0.5,
            cy: 0.45,
            radius: 0.2,
            amount: 0.9,
        },
        &[LOW_CONTRAST, PORTRAIT],
    );
}

#[test]
fn golden_glow() {
    effect_goldens(
        "glow",
        &EditOp::Glow {
            intensity: 0.8,
            radius: 1.5,
        },
        &BOTH,
    );
}

#[test]
fn golden_focal_black_and_white() {
    effect_goldens(
        "focal_black_and_white",
        &EditOp::FocalBlackAndWhite {
            cx: 0.5,
            cy: 0.5,
            radius: 0.2,
            sharpness: 0.5,
        },
        &BOTH,
    );
}

#[test]
fn golden_graduated_tint() {
    effect_goldens(
        "graduated_tint",
        &EditOp::GraduatedTint {
            color: [0.2, 0.5, 1.0],
            y: 0.45,
            feather: 0.2,
            amount: 0.9,
        },
        &BOTH,
    );
}

#[test]
fn golden_vignette() {
    effect_goldens(
        "vignette",
        &EditOp::Vignette {
            amount: 0.8,
            radius: 0.35,
        },
        &BOTH,
    );
}

#[test]
fn golden_red_eye() {
    use lb_core::NormRect;
    let regions = [100.0_f32, 220.0]
        .map(|cx| NormRect {
            x: (cx - 24.0) / 320.0,
            y: (110.0 - 24.0) / 240.0,
            w: 48.0 / 320.0,
            h: 48.0 / 240.0,
        })
        .to_vec();
    golden(
        "effects",
        "golden_red_eye_eyes",
        common::red_eye_320x240(),
        &EditOp::RedEye { regions },
    );
}

fn blemished_gradient() -> Rgba32FImage {
    let mut img = common::gradient_rgb_512();
    for (x, y, p) in img.enumerate_pixels_mut() {
        let (dx, dy) = (x as f32 - 256.0, y as f32 - 256.0);
        if dx * dx + dy * dy <= 22.0 * 22.0 {
            p.0 = [0.25, 0.1, 0.1, 1.0];
        }
    }
    img
}

#[test]
fn golden_retouch_patch() {
    use lb_core::RetouchSpot;
    let op = EditOp::Retouch {
        spots: vec![RetouchSpot {
            src_x: 0.68,
            src_y: 0.5,
            dst_x: 0.5,
            dst_y: 0.5,
            radius: 0.06,
        }],
    };
    golden("retouch", "golden_retouch_patch", blemished_gradient(), &op);
}
