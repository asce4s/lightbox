//! Image decoding, orientation, metadata extraction and hashing (SPEC §6.2).
#![warn(missing_docs)]

mod decode;
mod error;
mod extract;
mod format;
mod hash;
mod metadata;
mod probe;
mod resize;
mod taken_at;

pub use decode::{Decoder, DefaultDecoder, apply_orientation, decode, decode_with, guard};
pub use error::{MediaError, Result};
pub use extract::{ImageMetadata, extract_image, fallback_taken_at};
pub use format::{
    Capabilities, format_for_extension, is_supported_extension, sniff_format, supported_extensions,
};
pub use hash::{QUICK_KEY_SAMPLE, content_hash, quick_key, quick_key_of_bytes};
pub use metadata::{ExifData, read_exif};
pub use probe::{Probe, probe};
pub use resize::{fit_within, resize_exact};
pub use taken_at::{DateSources, TakenAt, filename_date, mtime_to_naive_local, resolve_taken_at};
