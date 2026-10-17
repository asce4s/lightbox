//! Error type of `lb-faces`.

/// Everything that can go wrong while detecting or embedding faces.
#[derive(Debug, thiserror::Error)]
pub enum FaceError {
    /// A model file is missing, corrupt or failed to run.
    #[error("face model error: {0}")]
    Model(String),
    /// A model file does not match its expected checksum.
    #[error("model file {name} has the wrong checksum")]
    Checksum {
        /// File name of the model.
        name: String,
    },
    /// Reading a model file failed.
    #[error("i/o error: {0}")]
    Io(#[from] std::io::Error),
}

/// Result alias for this crate.
pub type Result<T, E = FaceError> = std::result::Result<T, E>;
