//! Rendering an edit stack onto an image (SPEC §6.6.4).

use crate::convert::to_working;
use crate::ops::{OpCtx, apply_op, effects::seed_from_quick_key};
use image::{DynamicImage, Rgba32FImage};
use lb_core::EditStack;
use lb_media::{fit_within, resize_exact};
use std::sync::atomic::{AtomicBool, Ordering};

/// How much of the source resolution a render uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RenderTarget {
    /// Shrink the source so its long edge is at most `max_edge` before applying the stack.
    /// Used for interactive previews and thumbnails.
    Proxy {
        /// Largest allowed long edge in pixels.
        max_edge: u32,
    },
    /// Render at the source's full resolution (export).
    Full,
}

/// Settings for one render.
#[derive(Debug, Clone, Copy)]
pub struct RenderCtx {
    /// Photo-specific seed for grain (see [`seed_from_quick_key`]).
    pub seed: u64,
    /// Proxy or full resolution.
    pub target: RenderTarget,
}

impl RenderCtx {
    /// A context seeded from the photo's quick key.
    pub fn new(quick_key: &str, target: RenderTarget) -> Self {
        Self {
            seed: seed_from_quick_key(quick_key),
            target,
        }
    }
}

/// Returned by [`render_cancellable`] when the cancel flag was raised.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
#[error("render cancelled")]
pub struct Cancelled;

/// Renders `stack` onto `src`, which must already be upright (EXIF orientation applied).
///
/// Ops that fail validation are skipped. Output is clamped to `[0, 1]` after every op and is
/// bit-identical between runs on the same machine.
pub fn render(src: &DynamicImage, stack: &EditStack, ctx: &RenderCtx) -> Rgba32FImage {
    match render_cancellable(src, stack, ctx, &AtomicBool::new(false)) {
        Ok(img) => img,
        Err(Cancelled) => Rgba32FImage::new(0, 0),
    }
}

/// Like [`render`] but checks `cancel` before every op so a newer request can abandon it.
pub fn render_cancellable(
    src: &DynamicImage,
    stack: &EditStack,
    ctx: &RenderCtx,
    cancel: &AtomicBool,
) -> Result<Rgba32FImage, Cancelled> {
    let scaled;
    let source = match ctx.target {
        RenderTarget::Proxy { max_edge } => {
            let (w, h) = fit_within(src.width(), src.height(), max_edge.max(1));
            if (w, h) == (src.width(), src.height()) {
                src
            } else {
                scaled = resize_exact(src, w, h);
                &scaled
            }
        }
        RenderTarget::Full => src,
    };
    let mut img = to_working(source);
    for (index, op) in stack.ops.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            return Err(Cancelled);
        }
        match op.validate() {
            Ok(op) => {
                img = apply_op(
                    img,
                    &op,
                    &OpCtx {
                        seed: ctx.seed,
                        index,
                    },
                );
            }
            Err(e) => tracing::warn!(error = %e, index, "skipping invalid edit op"),
        }
    }
    if cancel.load(Ordering::Relaxed) {
        return Err(Cancelled);
    }
    Ok(img)
}
