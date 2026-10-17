//! Scanner error type.

/// Everything that can go wrong while scanning or managing roots.
#[derive(Debug, thiserror::Error)]
pub enum ScanError {
    /// The catalog failed.
    #[error(transparent)]
    Catalog(#[from] lb_catalog::CatalogError),
    /// A file-system operation failed.
    #[error("i/o error on {path}: {source}")]
    Io {
        /// Path involved.
        path: std::path::PathBuf,
        /// Underlying error.
        #[source]
        source: std::io::Error,
    },
    /// The path is not valid UTF-8 and cannot be stored.
    #[error("path is not valid UTF-8: {0}")]
    NonUtf8Path(std::path::PathBuf),
    /// The referenced root does not exist.
    #[error("root not found")]
    RootNotFound,
}

/// Result alias for scanner operations.
pub type Result<T, E = ScanError> = std::result::Result<T, E>;
