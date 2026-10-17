//! Edit operations (SPEC §6.6.3).

pub mod geometry;

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
pub fn apply_op(img: Rgba32FImage, op: &EditOp, _ctx: &OpCtx) -> Rgba32FImage {
    match op {
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
