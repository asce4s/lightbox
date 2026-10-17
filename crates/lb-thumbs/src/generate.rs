//! Rendering a thumbnail: decode, apply edits on a proxy, downscale, encode.

use crate::{EditFn, Result, ThumbError, ThumbSize};
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ExtendedColorType};
use lb_media::{Decoder, decode_with, fit_within, resize_exact};
use std::path::Path;

/// JPEG quality of cached thumbnails.
pub const JPEG_QUALITY: u8 = 82;
/// Edits run on a proxy no larger than this before the final downscale (SPEC §6.4).
pub const PROXY_LONG_EDGE: u32 = 1280;

pub(crate) fn render(
    decoder: &dyn Decoder,
    path: &Path,
    size: ThumbSize,
    edits: Option<&EditFn>,
) -> Result<DynamicImage> {
    let proxy_edge = if edits.is_some() {
        PROXY_LONG_EDGE
    } else {
        size.px()
    };
    let mut img = decode_with(decoder, path, Some(proxy_edge))?;
    if let Some(edit) = edits {
        img = edit(img);
    }
    let (w, h) = fit_within(img.width(), img.height(), size.px());
    if (w, h) != (img.width(), img.height()) {
        img = resize_exact(&img, w, h);
    }
    Ok(img)
}

pub(crate) fn encode_jpeg(img: &DynamicImage) -> Result<Vec<u8>> {
    let rgb = img.to_rgb8();
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, JPEG_QUALITY)
        .encode(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            ExtendedColorType::Rgb8,
        )
        .map_err(|e| ThumbError::Encode(e.to_string()))?;
    Ok(out)
}
