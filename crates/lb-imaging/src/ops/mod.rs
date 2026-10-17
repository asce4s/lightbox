//! Edit operations (SPEC §6.6.3).

pub mod auto;
pub mod geometry;
pub mod hist;
pub mod tuning;

use image::Rgba32FImage;
use lb_core::EditOp;

/// Per-op context.
#[derive(Debug, Clone, Copy, Default)]
pub struct OpCtx {
    /// Photo-specific seed (from the quick key) for ops that need randomness.
    pub seed: u64,
    /// Index of the op in the stack, mixed into seeds.
    pub index: usize,
}

/// Applies one op to `img`. Parameters are expected to be validated (`EditOp::validate`).
pub fn apply_op(mut img: Rgba32FImage, op: &EditOp, _ctx: &OpCtx) -> Rgba32FImage {
    match op {
        EditOp::AutoContrast => {
            auto::auto_contrast(&mut img);
            img
        }
        EditOp::AutoColor => {
            auto::auto_color(&mut img);
            img
        }
        EditOp::FeelingLucky => {
            auto::auto_contrast(&mut img);
            auto::auto_color(&mut img);
            img
        }
        EditOp::FillLight { amount } => {
            tuning::fill_light(&mut img, *amount);
            img
        }
        EditOp::Tuning {
            fill_light,
            highlights,
            shadows,
            temperature,
            neutral,
        } => {
            tuning::tuning(
                &mut img,
                *fill_light,
                *highlights,
                *shadows,
                *temperature,
                *neutral,
            );
            img
        }
        EditOp::RotateQuarter { turns } => geometry::rotate_quarter(&img, *turns),
        EditOp::Crop { x, y, w, h } => geometry::crop(&img, *x, *y, *w, *h),
        EditOp::Straighten { degrees } => geometry::straighten(&img, *degrees),
        other => {
            tracing::warn!(
                op = other.name(),
                "edit op is not implemented yet; skipping"
            );
            img
        }
    }
}
