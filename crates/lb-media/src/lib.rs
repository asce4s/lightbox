//! Image decoding, orientation, metadata extraction and hashing (SPEC §6.2).
#![warn(missing_docs)]

mod decode;
mod error;
mod format;
mod metadata;
mod probe;
mod resize;

pub use decode::{Decoder, DefaultDecoder, apply_orientation, decode, decode_with, guard};
pub use error::{MediaError, Result};
pub use format::{
    Capabilities, format_for_extension, is_supported_extension, sniff_format, supported_extensions,
};
pub use probe::{Probe, probe};
pub use resize::{fit_within, resize_exact};
