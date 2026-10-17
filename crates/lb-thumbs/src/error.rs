//! Thumbnail errors.

/// Everything that can go wrong while producing or caching a thumbnail.
#[derive(Debug, thiserror::Error)]
pub enum ThumbError {
    /// The source image could not be decoded.
    #[error(transparent)]
    Media(#[from] lb_media::MediaError),
    /// The cache directory could not be read or written.
    #[error("cache i/o error: {0}")]
    Io(#[from] std::io::Error),
    /// The thumbnail could not be encoded.
    #[error("thumbnail encoding failed: {0}")]
    Encode(String),
}

/// Result alias for thumbnail operations.
pub type Result<T, E = ThumbError> = std::result::Result<T, E>;
