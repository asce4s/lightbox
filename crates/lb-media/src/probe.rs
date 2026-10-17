//! Cheap header probe: format, oriented dimensions and structural completeness.

use crate::{MediaError, Result, decode::sniff_file, metadata::read_orientation};
use image::{ImageFormat, ImageReader};
use lb_core::Format;
use std::fs::File;
use std::io::{BufReader, Read, Seek, SeekFrom};
use std::path::Path;

/// What can be learned about an image without decoding its pixels.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Probe {
    /// Format identified from the file content.
    pub format: Format,
    /// Width after applying the EXIF orientation.
    pub width: u32,
    /// Height after applying the EXIF orientation.
    pub height: u32,
    /// EXIF orientation of the stored pixels, 1..=8.
    pub orientation: u8,
}

const TAIL: u64 = 64 * 1024;

fn tail(file: &mut File, size: u64) -> Result<Vec<u8>> {
    let len = size.min(TAIL);
    file.seek(SeekFrom::Start(size - len))?;
    let mut buf = vec![0_u8; usize::try_from(len).unwrap_or(0)];
    file.read_exact(&mut buf)?;
    Ok(buf)
}

/// Detects truncated JPEG and PNG files without decoding: JPEGs must contain an end-of-image
/// marker in their last 64 KiB, PNGs an `IEND` chunk. Other formats are not checked here.
fn check_complete(path: &Path, format: Format) -> Result<()> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    match format {
        Format::Jpeg => {
            let tail = tail(&mut file, size)?;
            if !tail.windows(2).any(|w| w == [0xFF, 0xD9]) {
                return Err(MediaError::Corrupt(
                    "truncated JPEG (no end-of-image marker)".into(),
                ));
            }
        }
        Format::Png => {
            let tail = tail(&mut file, size)?;
            if !tail.windows(4).any(|w| w == b"IEND") {
                return Err(MediaError::Corrupt("truncated PNG (no IEND chunk)".into()));
            }
        }
        _ => {}
    }
    Ok(())
}

/// Probes `path`: identifies the format by content, rejects obviously truncated files and
/// returns the oriented dimensions without decoding pixel data.
pub fn probe(path: &Path) -> Result<Probe> {
    let format = sniff_file(path)?;
    let image_format = match format {
        Format::Jpeg => ImageFormat::Jpeg,
        Format::Png => ImageFormat::Png,
        Format::Gif => ImageFormat::Gif,
        Format::Webp => ImageFormat::WebP,
        Format::Tiff => ImageFormat::Tiff,
        Format::Bmp => ImageFormat::Bmp,
        other => return Err(MediaError::Unsupported(other.to_string())),
    };
    check_complete(path, format)?;
    let reader = ImageReader::with_format(BufReader::new(File::open(path)?), image_format);
    let (w, h) = crate::decode::guard(|| {
        reader
            .into_dimensions()
            .map_err(|e| MediaError::Corrupt(e.to_string()))
    })?;
    let orientation = read_orientation(path);
    let (width, height) = if (5..=8).contains(&orientation) {
        (h, w)
    } else {
        (w, h)
    };
    Ok(Probe {
        format,
        width,
        height,
        orientation,
    })
}
