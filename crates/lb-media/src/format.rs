//! File format detection from extensions and magic bytes (SPEC §6.1, §6.2.4).

use lb_core::Format;

/// Which optional format families are indexed (SPEC §6.1 supported extensions).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Capabilities {
    /// `heic heif avif`.
    pub heif: bool,
    /// Camera RAW extensions.
    pub raw: bool,
    /// Video containers.
    pub video: bool,
}

const IMAGE_EXTS: &[(&str, Format)] = &[
    ("jpg", Format::Jpeg),
    ("jpeg", Format::Jpeg),
    ("jpe", Format::Jpeg),
    ("png", Format::Png),
    ("gif", Format::Gif),
    ("webp", Format::Webp),
    ("tif", Format::Tiff),
    ("tiff", Format::Tiff),
    ("bmp", Format::Bmp),
];
const HEIF_EXTS: &[(&str, Format)] = &[
    ("heic", Format::Heif),
    ("heif", Format::Heif),
    ("avif", Format::Heif),
];
const RAW_EXTS: &[(&str, Format)] = &[
    ("cr2", Format::Raw),
    ("cr3", Format::Raw),
    ("nef", Format::Raw),
    ("arw", Format::Raw),
    ("dng", Format::Raw),
    ("orf", Format::Raw),
    ("rw2", Format::Raw),
    ("raf", Format::Raw),
];
const VIDEO_EXTS: &[(&str, Format)] = &[
    ("mp4", Format::Mp4),
    ("m4v", Format::Mp4),
    ("3gp", Format::Mp4),
    ("mts", Format::Mp4),
    ("mov", Format::Mov),
    ("avi", Format::Avi),
    ("mkv", Format::Mkv),
    ("webm", Format::Webm),
];

fn groups(caps: Capabilities) -> impl Iterator<Item = &'static (&'static str, Format)> {
    IMAGE_EXTS
        .iter()
        .chain(caps.heif.then_some(HEIF_EXTS).into_iter().flatten())
        .chain(caps.raw.then_some(RAW_EXTS).into_iter().flatten())
        .chain(caps.video.then_some(VIDEO_EXTS).into_iter().flatten())
}

/// The format a file extension denotes (case-insensitive, without the dot), if the
/// capabilities include it.
pub fn format_for_extension(ext: &str, caps: Capabilities) -> Option<Format> {
    let ext = ext.to_ascii_lowercase();
    groups(caps).find(|(e, _)| *e == ext).map(|(_, f)| *f)
}

/// Whether `ext` (no dot, any case) is a camera RAW extension.
pub fn is_raw_extension(ext: &str) -> bool {
    let ext = ext.to_ascii_lowercase();
    RAW_EXTS.iter().any(|(e, _)| *e == ext)
}

/// Whether `ext` (no dot, any case) is a HEIF/HEIC/AVIF extension.
pub fn is_heif_extension(ext: &str) -> bool {
    let ext = ext.to_ascii_lowercase();
    HEIF_EXTS.iter().any(|(e, _)| *e == ext)
}

/// Whether `ext` is an indexed extension under `caps`.
pub fn is_supported_extension(ext: &str, caps: Capabilities) -> bool {
    format_for_extension(ext, caps).is_some()
}

/// All indexed extensions under `caps`.
pub fn supported_extensions(caps: Capabilities) -> Vec<&'static str> {
    groups(caps).map(|(e, _)| *e).collect()
}

/// Identifies a still-image format from its first bytes, ignoring the file name.
pub fn sniff_format(head: &[u8]) -> Option<Format> {
    match head {
        [0xFF, 0xD8, 0xFF, ..] => Some(Format::Jpeg),
        [0x89, b'P', b'N', b'G', ..] => Some(Format::Png),
        [b'G', b'I', b'F', b'8', ..] => Some(Format::Gif),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'W',
            b'E',
            b'B',
            b'P',
            ..,
        ] => Some(Format::Webp),
        [b'I', b'I', 0x2A, 0x00, ..] | [b'M', b'M', 0x00, 0x2A, ..] => Some(Format::Tiff),
        [b'B', b'M', ..] => Some(Format::Bmp),
        [_, _, _, _, b'f', b't', b'y', b'p', brand @ ..] => Some(if brand.starts_with(b"qt") {
            Format::Mov
        } else {
            Format::Mp4
        }),
        [
            b'R',
            b'I',
            b'F',
            b'F',
            _,
            _,
            _,
            _,
            b'A',
            b'V',
            b'I',
            b' ',
            ..,
        ] => Some(Format::Avi),
        [0x1A, 0x45, 0xDF, 0xA3, ..] => Some(Format::Mkv),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_mapping_respects_capabilities() {
        let none = Capabilities::default();
        assert_eq!(format_for_extension("JPG", none), Some(Format::Jpeg));
        assert_eq!(format_for_extension("tiff", none), Some(Format::Tiff));
        assert_eq!(format_for_extension("heic", none), None);
        assert_eq!(format_for_extension("mp4", none), None);
        let all = Capabilities {
            heif: true,
            raw: true,
            video: true,
        };
        assert_eq!(format_for_extension("HEIC", all), Some(Format::Heif));
        assert_eq!(format_for_extension("cr2", all), Some(Format::Raw));
        assert_eq!(format_for_extension("MTS", all), Some(Format::Mp4));
        assert_eq!(format_for_extension("txt", all), None);
        assert!(supported_extensions(all).len() > supported_extensions(none).len());
    }

    #[test]
    fn sniffing_ignores_extensions() {
        assert_eq!(sniff_format(&[0xFF, 0xD8, 0xFF, 0xE0]), Some(Format::Jpeg));
        assert_eq!(sniff_format(b"\x89PNG\r\n\x1a\n"), Some(Format::Png));
        assert_eq!(sniff_format(b"GIF89a"), Some(Format::Gif));
        assert_eq!(sniff_format(b"RIFF\0\0\0\0WEBPVP8 "), Some(Format::Webp));
        assert_eq!(sniff_format(b"II*\0"), Some(Format::Tiff));
        assert_eq!(sniff_format(b"BM\0\0"), Some(Format::Bmp));
        assert_eq!(sniff_format(b"\0\0\0\x18ftypisom"), Some(Format::Mp4));
        assert_eq!(sniff_format(b"\0\0\0\x14ftypqt  "), Some(Format::Mov));
        assert_eq!(sniff_format(b"RIFF\0\0\0\0AVI LIST"), Some(Format::Avi));
        assert_eq!(
            sniff_format(&[0x1A, 0x45, 0xDF, 0xA3, 0x01]),
            Some(Format::Mkv)
        );
        assert_eq!(sniff_format(b"hello world, not an image"), None);
        assert_eq!(sniff_format(b""), None);
    }
}
