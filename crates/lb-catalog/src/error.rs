//! Catalog error type.

/// Everything that can go wrong inside `lb-catalog`.
#[derive(Debug, thiserror::Error)]
pub enum CatalogError {
    /// SQLite reported an error (constraint violation, I/O, ...).
    #[error("database error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Applying schema migrations failed.
    #[error("migration error: {0}")]
    Migration(#[from] rusqlite_migration::Error),
    /// Filesystem error while preparing the catalog location.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
    /// The catalog writer thread is gone (the catalog was shut down).
    #[error("catalog writer thread is not running")]
    WriterStopped,
    /// A closure running on the writer thread panicked; the transaction was rolled back.
    #[error("catalog write operation panicked: {0}")]
    WriterPanicked(String),
    /// A write was requested from inside a write closure, which would deadlock the writer.
    #[error("catalog write requested from the writer thread (re-entrant write)")]
    ReentrantWrite,
    /// A path cannot be stored because it is not valid UTF-8.
    #[error("path is not valid UTF-8: {}", .0.display())]
    NonUtf8Path(std::path::PathBuf),
    /// The referenced row does not exist.
    #[error("{0} not found")]
    NotFound(&'static str),
    /// A tag name failed validation.
    #[error(transparent)]
    InvalidTagName(#[from] lb_core::InvalidTagName),
    /// The database was created by a newer version of Lightbox.
    #[error("catalog schema is newer than this version of Lightbox supports")]
    SchemaTooNew,
}

/// Result alias for catalog operations.
pub type Result<T, E = CatalogError> = std::result::Result<T, E>;
