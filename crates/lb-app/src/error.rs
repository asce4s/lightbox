//! The error type of the service layer.

use lb_core::EditError;

/// Everything a service call can fail with.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    /// The catalog failed.
    #[error(transparent)]
    Catalog(#[from] lb_catalog::CatalogError),
    /// Scanning or root management failed.
    #[error(transparent)]
    Scan(#[from] lb_scan::ScanError),
    /// A thumbnail could not be produced.
    #[error(transparent)]
    Thumb(#[from] lb_thumbs::ThumbError),
    /// An edit operation was rejected.
    #[error(transparent)]
    Edit(#[from] EditError),
    /// Decoding or reading an image failed.
    #[error(transparent)]
    Media(#[from] lb_media::MediaError),
    /// The operation was cancelled before it finished.
    #[error("cancelled")]
    Cancelled,
    /// A file-system operation failed.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    /// The caller passed a value the operation cannot accept.
    #[error("invalid input: {0}")]
    Invalid(String),
    /// The referenced entity does not exist.
    #[error("{0} not found")]
    NotFound(&'static str),
}

/// Result alias for service calls.
pub type Result<T, E = AppError> = std::result::Result<T, E>;
