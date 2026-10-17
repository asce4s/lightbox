//! Embedded JPEG preview extraction for camera RAW files (SPEC §6.2.3).
//!
//! Full demosaic is out of scope. RAW containers here are TIFF (CR2, NEF, ARW, DNG, ORF, RW2);
//! the largest embedded JPEG (preview before thumbnail) is what gets decoded.

use crate::{MediaError, Result, decode::decode_jpeg_reader, decode::present};
use std::collections::HashSet;
use std::io::Cursor;
use std::path::Path;

const MAX_BYTES: u64 = 512 * 1024 * 1024;
const MAX_DEPTH: u8 = 8;
const MAX_ENTRIES: usize = 512;
const MAX_VALUES: usize = 64;

/// Decodes the largest embedded JPEG preview of a RAW file.
pub(crate) fn decode_raw_preview(
    path: &Path,
    max_long_edge: Option<u32>,
) -> Result<image::DynamicImage> {
    let len = std::fs::metadata(path)?.len();
    if len == 0 {
        return Err(MediaError::Corrupt("empty RAW file".into()));
    }
    if len > MAX_BYTES {
        return Err(MediaError::Corrupt(
            "RAW file is too large to scan for a preview".into(),
        ));
    }
    let data = std::fs::read(path)?;
    let jpeg = largest_jpeg(&data)
        .ok_or_else(|| MediaError::Corrupt("RAW file has no embedded JPEG preview".into()))?;
    let img = decode_jpeg_reader(Cursor::new(jpeg), None)?;
    present(path, img, max_long_edge)
}

fn largest_jpeg(data: &[u8]) -> Option<&[u8]> {
    let be = match data.get(..2)? {
        b"II" => false,
        b"MM" => true,
        _ => return None,
    };
    if read_u16(data, 2, be)? != 42 {
        return None;
    }
    let mut found: Vec<(usize, usize)> = Vec::new();
    let mut seen = HashSet::new();
    walk(
        data,
        read_u32(data, 4, be)? as usize,
        be,
        0,
        &mut seen,
        &mut found,
    );
    found
        .into_iter()
        .max_by_key(|(_, len)| *len)
        .and_then(|(off, len)| data.get(off..off + len))
}

fn walk(
    data: &[u8],
    offset: usize,
    be: bool,
    depth: u8,
    seen: &mut HashSet<usize>,
    found: &mut Vec<(usize, usize)>,
) {
    if depth > MAX_DEPTH || !seen.insert(offset) {
        return;
    }
    let Some(count) = read_u16(data, offset, be) else {
        return;
    };
    let count = usize::from(count);
    if count > MAX_ENTRIES {
        return;
    }
    let entries = offset + 2;
    let bytes = count * 12;
    if entries + bytes + 4 > data.len() {
        return;
    }
    let mut jpeg_off = None;
    let mut jpeg_len = None;
    let mut strip_off = None;
    let mut strip_len = None;
    let mut sub_ifds = Vec::new();
    for i in 0..count {
        let pos = entries + i * 12;
        let tag = read_u16(data, pos, be).unwrap_or(0);
        let kind = read_u16(data, pos + 2, be).unwrap_or(0);
        let n = read_u32(data, pos + 4, be).unwrap_or(0) as usize;
        let field = data.get(pos + 8..pos + 12).unwrap_or(&[0; 4]);
        match tag {
            0x014A => sub_ifds = numbers(data, kind, n, field, be).unwrap_or_default(),
            0x0201 => jpeg_off = numbers(data, kind, n, field, be).and_then(|v| v.first().copied()),
            0x0202 => jpeg_len = numbers(data, kind, n, field, be).and_then(|v| v.first().copied()),
            0x0111 => strip_off = numbers(data, kind, n, field, be),
            0x0117 => strip_len = numbers(data, kind, n, field, be),
            _ => {}
        }
    }
    if let (Some(off), Some(len)) = (jpeg_off, jpeg_len) {
        note(data, off, len, found);
    }
    if let (Some(offs), Some(lens)) = (strip_off, strip_len) {
        for (off, len) in offs.into_iter().zip(lens) {
            note(data, off, len, found);
        }
    }
    let next = read_u32(data, entries + bytes, be).unwrap_or(0) as usize;
    for sub in sub_ifds {
        walk(data, sub as usize, be, depth + 1, seen, found);
    }
    if next != 0 {
        walk(data, next, be, depth + 1, seen, found);
    }
}

fn note(data: &[u8], off: u32, len: u32, found: &mut Vec<(usize, usize)>) {
    let (off, len) = (off as usize, len as usize);
    if len >= 4 && data.get(off..off + 3) == Some(&[0xFF, 0xD8, 0xFF]) {
        found.push((off, len));
    }
}

/// Reads `count` SHORT (type 3) or LONG (type 4) values, inline when they fit in the 4-byte field.
fn numbers(data: &[u8], kind: u16, count: usize, field: &[u8], be: bool) -> Option<Vec<u32>> {
    if count == 0 || count > MAX_VALUES {
        return None;
    }
    let unit = match kind {
        3 => 2,
        4 => 4,
        _ => return None,
    };
    let nbytes = count * unit;
    let raw: Vec<u8> = if nbytes <= 4 {
        field.get(..nbytes)?.to_vec()
    } else {
        let off = read_u32_bytes(field, be)? as usize;
        data.get(off..off + nbytes)?.to_vec()
    };
    let mut out = Vec::with_capacity(count);
    if unit == 2 {
        for chunk in raw.as_chunks::<2>().0 {
            let n = if be {
                u16::from_be_bytes(*chunk)
            } else {
                u16::from_le_bytes(*chunk)
            };
            out.push(u32::from(n));
        }
    } else {
        for chunk in raw.as_chunks::<4>().0 {
            out.push(if be {
                u32::from_be_bytes(*chunk)
            } else {
                u32::from_le_bytes(*chunk)
            });
        }
    }
    Some(out)
}

fn read_u16(data: &[u8], offset: usize, be: bool) -> Option<u16> {
    let b = data.get(offset..offset + 2)?;
    Some(if be {
        u16::from_be_bytes([b[0], b[1]])
    } else {
        u16::from_le_bytes([b[0], b[1]])
    })
}

fn read_u32(data: &[u8], offset: usize, be: bool) -> Option<u32> {
    read_u32_bytes(data.get(offset..offset + 4)?, be)
}

fn read_u32_bytes(b: &[u8], be: bool) -> Option<u32> {
    let b: [u8; 4] = b.get(..4)?.try_into().ok()?;
    Some(if be {
        u32::from_be_bytes(b)
    } else {
        u32::from_le_bytes(b)
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ExtendedColorType, Rgb, RgbImage};
    use std::path::PathBuf;

    fn jpeg(w: u32, h: u32, color: [u8; 3]) -> Vec<u8> {
        let image = RgbImage::from_pixel(w, h, Rgb(color));
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 95)
            .encode(image.as_raw(), w, h, ExtendedColorType::Rgb8)
            .unwrap();
        bytes
    }

    /// Little-endian TIFF: a small preview in IFD0 and a larger one in a SubIFD.
    fn raw_with_two_previews(small: &[u8], large: &[u8]) -> Vec<u8> {
        let sub_at = 8 + 2 + 3 * 12 + 4;
        let small_at = sub_at + 2 + 2 * 12 + 4;
        let large_at = small_at + small.len();
        let mut out = Vec::new();
        out.extend_from_slice(b"II*\0");
        out.extend_from_slice(&8u32.to_le_bytes());
        out.extend_from_slice(&3u16.to_le_bytes());
        // tag order: SubIFD 0x014A, JPEGInterchangeFormat 0x0201, length 0x0202
        let entry = |tag: u16, value: u32| {
            let mut e = Vec::new();
            e.extend_from_slice(&tag.to_le_bytes());
            e.extend_from_slice(&4u16.to_le_bytes());
            e.extend_from_slice(&1u32.to_le_bytes());
            e.extend_from_slice(&value.to_le_bytes());
            e
        };
        out.extend(entry(0x014A, sub_at as u32));
        out.extend(entry(0x0201, small_at as u32));
        out.extend(entry(0x0202, small.len() as u32));
        out.extend_from_slice(&0u32.to_le_bytes());
        out.extend_from_slice(&2u16.to_le_bytes());
        out.extend(entry(0x0201, large_at as u32));
        out.extend(entry(0x0202, large.len() as u32));
        out.extend_from_slice(&0u32.to_le_bytes());
        assert_eq!(out.len(), small_at);
        out.extend_from_slice(small);
        out.extend_from_slice(large);
        out
    }

    fn write_dng(bytes: &[u8]) -> (tempfile::TempDir, PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("preview.dng");
        std::fs::write(&path, bytes).unwrap();
        (dir, path)
    }

    #[test]
    fn raw_embedded_preview_decodes() {
        let small = jpeg(2, 2, [1, 2, 3]);
        let large = jpeg(10, 6, [200, 10, 10]);
        let (_dir, path) = write_dng(&raw_with_two_previews(&small, &large));
        let img = crate::decode(&path, None).unwrap();
        assert_eq!(
            (img.width(), img.height()),
            (10, 6),
            "the larger SubIFD preview wins over the thumbnail"
        );
        let direct = decode_jpeg_reader(Cursor::new(large), None).unwrap();
        assert_eq!(img.to_rgb8().as_raw(), direct.to_rgb8().as_raw());

        let shrunk = crate::decode(&path, Some(4)).unwrap();
        assert_eq!(shrunk.width().max(shrunk.height()), 4);
    }

    #[test]
    fn raw_without_a_preview_is_unreadable() {
        let mut tiff = b"II*\0".to_vec();
        tiff.extend_from_slice(&8u32.to_le_bytes());
        tiff.extend_from_slice(&0u16.to_le_bytes());
        tiff.extend_from_slice(&0u32.to_le_bytes());
        let (_dir, path) = write_dng(&tiff);
        let err = crate::decode(&path, None).unwrap_err();
        assert!(
            matches!(err, MediaError::Corrupt(ref m) if m.contains("preview")),
            "{err:?}"
        );
    }
}
