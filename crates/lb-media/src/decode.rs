//! Decoding with EXIF orientation, DCT-scaled JPEG and panic containment (SPEC §6.2.3).

use crate::{MediaError, Result, fit_within, format::sniff_format, resize_exact};
use image::{DynamicImage, GrayImage, ImageFormat, ImageReader, RgbImage, metadata::Orientation};
use lb_core::Format;
use std::fs::File;
use std::io::{BufReader, Read};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::Path;

/// A source of raw (not yet oriented) pixels. Tests inject their own to simulate decoder
/// failures; production uses [`DefaultDecoder`].
pub trait Decoder: Send + Sync {
    /// Decodes `path`, known to contain `format`. When `max_long_edge` is set the decoder may
    /// return a smaller image (for example via DCT scaling) but never one whose long edge is
    /// below `max_long_edge`, unless the source itself is smaller.
    fn decode_raw(
        &self,
        path: &Path,
        format: Format,
        max_long_edge: Option<u32>,
    ) -> Result<DynamicImage>;
}

/// The built-in decoder: `jpeg-decoder` for JPEG (DCT scaling, strict about truncation) and the
/// `image` crate for everything else.
#[derive(Debug, Clone, Copy, Default)]
pub struct DefaultDecoder;

fn corrupt(e: impl std::fmt::Display) -> MediaError {
    MediaError::Corrupt(e.to_string())
}

fn decode_jpeg(path: &Path, max_long_edge: Option<u32>) -> Result<DynamicImage> {
    let mut dec = jpeg_decoder::Decoder::new(BufReader::new(File::open(path)?));
    dec.read_info().map_err(corrupt)?;
    let info = dec
        .info()
        .ok_or_else(|| MediaError::Corrupt("missing JPEG header".into()))?;
    if let Some(max) = max_long_edge {
        let (w, h) = (u32::from(info.width), u32::from(info.height));
        let (tw, th) = fit_within(w, h, max);
        if (tw, th) != (w, h) {
            let clamp = |v: u32| u16::try_from(v).unwrap_or(u16::MAX);
            dec.scale(clamp(tw), clamp(th)).map_err(corrupt)?;
        }
    }
    let pixels = dec.decode().map_err(corrupt)?;
    let info = dec
        .info()
        .ok_or_else(|| MediaError::Corrupt("missing JPEG header".into()))?;
    let (w, h) = (u32::from(info.width), u32::from(info.height));
    let bad = || MediaError::Corrupt("JPEG pixel buffer has the wrong size".into());
    match info.pixel_format {
        jpeg_decoder::PixelFormat::L8 => GrayImage::from_raw(w, h, pixels)
            .map(DynamicImage::ImageLuma8)
            .ok_or_else(bad),
        jpeg_decoder::PixelFormat::L16 => {
            let gray: Vec<u8> = pixels.as_chunks::<2>().0.iter().map(|c| c[0]).collect();
            GrayImage::from_raw(w, h, gray)
                .map(DynamicImage::ImageLuma8)
                .ok_or_else(bad)
        }
        jpeg_decoder::PixelFormat::RGB24 => RgbImage::from_raw(w, h, pixels)
            .map(DynamicImage::ImageRgb8)
            .ok_or_else(bad),
        jpeg_decoder::PixelFormat::CMYK32 => {
            let rgb: Vec<u8> = pixels
                .as_chunks::<4>()
                .0
                .iter()
                .flat_map(|p| {
                    let k = u32::from(p[3]);
                    [0, 1, 2].map(|i| u8::try_from(u32::from(p[i]) * k / 255).unwrap_or(255))
                })
                .collect();
            RgbImage::from_raw(w, h, rgb)
                .map(DynamicImage::ImageRgb8)
                .ok_or_else(bad)
        }
    }
}

fn image_format(format: Format) -> Result<ImageFormat> {
    Ok(match format {
        Format::Jpeg => ImageFormat::Jpeg,
        Format::Png => ImageFormat::Png,
        Format::Gif => ImageFormat::Gif,
        Format::Webp => ImageFormat::WebP,
        Format::Tiff => ImageFormat::Tiff,
        Format::Bmp => ImageFormat::Bmp,
        other => return Err(MediaError::Unsupported(other.to_string())),
    })
}

impl Decoder for DefaultDecoder {
    fn decode_raw(
        &self,
        path: &Path,
        format: Format,
        max_long_edge: Option<u32>,
    ) -> Result<DynamicImage> {
        if format == Format::Jpeg {
            return decode_jpeg(path, max_long_edge);
        }
        let reader =
            ImageReader::with_format(BufReader::new(File::open(path)?), image_format(format)?);
        reader.decode().map_err(corrupt)
    }
}

/// Runs `f`, converting a panic inside a decoder into [`MediaError::Corrupt`].
pub fn guard<T>(f: impl FnOnce() -> Result<T>) -> Result<T> {
    match catch_unwind(AssertUnwindSafe(f)) {
        Ok(result) => result,
        Err(payload) => {
            let message = payload
                .downcast_ref::<&str>()
                .map(|s| (*s).to_owned())
                .or_else(|| payload.downcast_ref::<String>().cloned())
                .unwrap_or_else(|| "unknown panic".to_owned());
            tracing::warn!(%message, "decoder panicked");
            Err(MediaError::Corrupt(format!("decoder panicked: {message}")))
        }
    }
}

/// Applies an EXIF orientation value (1..=8; anything else means 1) so the result is upright.
pub fn apply_orientation(mut img: DynamicImage, orientation: u8) -> DynamicImage {
    if let Some(o) = Orientation::from_exif(orientation) {
        img.apply_orientation(o);
    }
    img
}

/// Reads the head of the file and identifies its format by content.
pub(crate) fn sniff_file(path: &Path) -> Result<Format> {
    let mut head = [0_u8; 16];
    let mut file = File::open(path)?;
    let mut filled = 0;
    while filled < head.len() {
        let n = file.read(&mut head[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    if filled == 0 {
        return Err(MediaError::Corrupt("empty file".into()));
    }
    sniff_format(&head[..filled])
        .ok_or_else(|| MediaError::Corrupt("not a recognized image format".into()))
}

/// Decodes `path` with `decoder`: identifies the format by content, decodes (panics contained),
/// shrinks to `max_long_edge` when given, then applies the EXIF orientation.
pub fn decode_with(
    decoder: &dyn Decoder,
    path: &Path,
    max_long_edge: Option<u32>,
) -> Result<DynamicImage> {
    let format = sniff_file(path)?;
    let raw = guard(|| decoder.decode_raw(path, format, max_long_edge))?;
    let img = match max_long_edge {
        Some(max) => {
            let (w, h) = fit_within(raw.width(), raw.height(), max);
            if (w, h) == (raw.width(), raw.height()) {
                raw
            } else {
                resize_exact(&raw, w, h)
            }
        }
        None => raw,
    };
    let orientation = crate::metadata::read_orientation(path);
    Ok(apply_orientation(img, orientation))
}

/// Decodes `path` upright with the built-in decoder (SPEC §6.2.3).
pub fn decode(path: &Path, max_long_edge: Option<u32>) -> Result<DynamicImage> {
    decode_with(&DefaultDecoder, path, max_long_edge)
}
