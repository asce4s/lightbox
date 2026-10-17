//! Non-destructive edit pipeline, compositing, export encoding and collage rendering.
#![warn(missing_docs)]
#![allow(clippy::chunks_exact_to_as_chunks)]

pub mod blur;
pub mod color;
pub mod convert;
pub mod ops;
pub mod render;
pub mod text;

pub use image::Rgba32FImage;
pub use render::{Cancelled, RenderCtx, RenderTarget, render, render_cancellable};
