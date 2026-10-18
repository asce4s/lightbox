//! Errors from planning or applying file-type associations.

/// A file-type association could not be planned or applied.
#[derive(Debug, thiserror::Error)]
pub enum AssocError {
    /// A file-system or registry operation failed.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    /// The saved payload or an argument cannot be applied.
    #[error("{0}")]
    Invalid(String),
    /// This build is not an installed app bundle, so the OS will not take a default.
    #[error("file type registration is available in the installed app")]
    Unavailable,
}

/// Result alias for association calls.
pub type Result<T, E = AssocError> = std::result::Result<T, E>;
