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
