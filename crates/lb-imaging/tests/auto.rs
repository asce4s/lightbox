//! Auto-op properties (SPEC §6.6.3, §6.6.4).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use lb_core::EditOp;
use lb_imaging::ops::{OpCtx, apply_op};
use proptest::prelude::*;

fn run(img: lb_imaging::Rgba32FImage, op: &EditOp) -> lb_imaging::Rgba32FImage {
    apply_op(img, op, &OpCtx::default())
}

fn squeeze(img: &mut lb_imaging::Rgba32FImage, lo: f32, hi: f32) {
    for p in img.pixels_mut() {
        for c in &mut p.0[..3] {
            *c = lo + (hi - lo) * *c;
        }
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn auto_contrast_idempotent_within_tolerance(
        seed in 0_u64..500, lo in 0.0_f32..0.4, span in 0.1_f32..0.6,
    ) {
        let mut img = common::noise(64, 48, seed);
        squeeze(&mut img, lo, lo + span);
        let original = img.clone();
        let once = run(img, &EditOp::AutoContrast);
        let twice = run(once.clone(), &EditOp::AutoContrast);
        let mean_abs = |a: &lb_imaging::Rgba32FImage, b: &lb_imaging::Rgba32FImage| {
            let sum: f32 = a.as_raw().iter().zip(b.as_raw()).map(|(x, y)| (x - y).abs()).sum();
            sum / a.as_raw().len() as f32
        };
        let first_pass = mean_abs(&original, &once);
        let second_pass = mean_abs(&once, &twice);
        // The spec stretches each channel by luma percentiles, so clipped colours lift the luma
        // percentiles of the result and a second pass can still stretch somewhat. It must be a
        // small correction compared with the first pass.
        prop_assert!(second_pass < 0.06, "second pass moved the image by {second_pass}");
        prop_assert!(
            second_pass <= 0.6 * first_pass + 1e-3,
            "second pass {second_pass} vs first pass {first_pass}"
        );
    }

    #[test]
    fn auto_contrast_is_tightly_idempotent_on_grey_images(
        seed in 0_u64..500, lo in 0.0_f32..0.3, span in 0.25_f32..0.7,
    ) {
        let mut img = common::noise(64, 48, seed);
        for p in img.pixels_mut() {
            let l = lb_imaging::color::luma(p.0[0], p.0[1], p.0[2]);
            p.0[..3].fill(lo + span * l);
        }
        let once = run(img, &EditOp::AutoContrast);
        let twice = run(once.clone(), &EditOp::AutoContrast);
        let max_diff = once
            .as_raw()
            .iter()
            .zip(twice.as_raw())
            .map(|(a, b)| (a - b).abs())
            .fold(0.0_f32, f32::max);
        // 1024 bins limit the percentile accuracy to about 0.001 / span.
        prop_assert!(max_diff < 0.01, "max diff {max_diff}");
    }

    #[test]
    fn auto_ops_keep_values_in_range_and_alpha(seed in 0_u64..500, which in 0_usize..3) {
        let img = common::noise(40, 30, seed);
        let op = [EditOp::AutoContrast, EditOp::AutoColor, EditOp::FeelingLucky][which].clone();
        let out = run(img.clone(), &op);
        prop_assert_eq!(out.dimensions(), img.dimensions());
        for (a, b) in out.pixels().zip(img.pixels()) {
            prop_assert!(a.0.iter().all(|v| (0.0..=1.0).contains(v)));
            prop_assert_eq!(a.0[3], b.0[3]);
        }
    }
}

#[test]
fn auto_contrast_widens_the_dynamic_range_of_the_low_contrast_fixture() {
    let img = common::low_contrast_400();
    let range = |i: &lb_imaging::Rgba32FImage| {
        let l: Vec<f32> = i
            .pixels()
            .map(|p| lb_imaging::color::luma(p.0[0], p.0[1], p.0[2]))
            .collect();
        l.iter().cloned().fold(0.0_f32, f32::max) - l.iter().cloned().fold(1.0_f32, f32::min)
    };
    let before = range(&img);
    let after = range(&run(img, &EditOp::AutoContrast));
    assert!(before < 0.3, "fixture really is low contrast: {before}");
    assert!(after > 0.9, "contrast stretched: {after}");
}

#[test]
fn feeling_lucky_equals_contrast_then_colour() {
    let img = common::portrait_patches_640x480();
    let lucky = run(img.clone(), &EditOp::FeelingLucky);
    let manual = run(run(img, &EditOp::AutoContrast), &EditOp::AutoColor);
    assert_eq!(lucky, manual);
}
