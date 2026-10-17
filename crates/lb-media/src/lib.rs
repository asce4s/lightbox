//! Image decoding, orientation, metadata extraction and hashing (SPEC §6.2).
#![warn(missing_docs)]

mod decode;
mod error;
mod exif_copy;
mod extract;
mod format;
mod hash;
#[cfg(feature = "heif")]
mod heif;
mod icc;
mod metadata;
mod phash;
#[cfg(feature = "raw")]
mod preview;
mod probe;
mod resize;
mod taken_at;
mod video;

pub use decode::{Decoder, DefaultDecoder, apply_orientation, decode, decode_with, guard};
pub use error::{MediaError, Result};
pub use exif_copy::{ExifRewrite, embed_exif, jpeg_exif, map_jpeg_exif, raw_exif, rewrite_exif};
pub use extract::{ImageMetadata, extract_image, fallback_taken_at};
pub use format::{
    Capabilities, format_for_extension, is_heif_extension, is_raw_extension,
    is_supported_extension, sniff_format, supported_extensions,
};

/// Whether this build can decode HEIC/HEIF/AVIF.
///
/// True only when the `heif` feature is on and libheif was found at build time.
pub fn heif_decoder_available() -> bool {
    cfg!(all(feature = "heif", lb_heif))
}

/// Whether this build decodes camera RAW files from their embedded JPEG preview.
pub fn raw_decoder_available() -> bool {
    cfg!(feature = "raw")
}
pub use hash::{QUICK_KEY_SAMPLE, content_hash, quick_key, quick_key_of_bytes};
pub use metadata::{ExifData, read_exif};
pub use phash::{BkTree, hamming, phash, phash_file};
pub use probe::{Probe, probe};
pub use resize::{fit_within, resize_exact};
pub use taken_at::{DateSources, TakenAt, filename_date, mtime_to_naive_local, resolve_taken_at};
pub use video::{VideoMetadata, VideoProber};
