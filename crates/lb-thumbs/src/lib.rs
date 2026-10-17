//! Thumbnail generation and the content-addressed disk cache (SPEC §6.4).
#![warn(missing_docs)]

mod cache;
mod error;
mod generate;
mod index;
mod key;

pub use cache::{GcReport, Thumb, ThumbCache, ThumbRequest};
pub use error::{Result, ThumbError};
pub use generate::{JPEG_QUALITY, PROXY_LONG_EDGE};
pub use key::{ThumbSize, cache_key};

/// A function applied to the oriented proxy image before it is scaled down to thumbnail size.
/// The edit pipeline plugs in here.
pub type EditFn = dyn Fn(image::DynamicImage) -> image::DynamicImage + Send + Sync;
