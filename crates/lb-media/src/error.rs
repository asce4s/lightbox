//! Media error type.

/// Everything that can go wrong while reading media files.
#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    /// The file could not be read.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    /// The file is empty, truncated, not an image, or crashed a decoder.
    #[error("corrupt or unreadable image: {0}")]
    Corrupt(String),
    /// The file is a valid container of a kind this build cannot decode.
    #[error("unsupported format: {0}")]
    Unsupported(String),
}

/// Result alias for media operations.
pub type Result<T, E = MediaError> = std::result::Result<T, E>;
