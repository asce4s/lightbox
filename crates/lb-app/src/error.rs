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
    /// A file or folder with that name already exists.
    #[error("{} already exists", .0.display())]
    AlreadyExists(std::path::PathBuf),
    /// The referenced entity does not exist.
    #[error("{0} not found")]
    NotFound(&'static str),
}

impl AppError {
    /// A sentence for the user (SPEC §10): what went wrong and what can be done, without paths
    /// into internals, SQL or type names. The technical detail stays in `Display` and the log.
    pub fn user_message(&self) -> String {
        use lb_catalog::CatalogError as C;
        match self {
            Self::Catalog(C::SchemaTooNew) => {
                "This library was created by a newer version of Lightbox. Update Lightbox to open it."
                    .into()
            }
            Self::Catalog(C::NotFound(what)) | Self::NotFound(what) => {
                format!("That {what} no longer exists.")
            }
            Self::Catalog(C::InvalidTagName(e)) => e.to_string(),
            Self::Catalog(C::NonUtf8Path(_)) | Self::Scan(lb_scan::ScanError::NonUtf8Path(_)) => {
                "A file name uses characters Lightbox cannot store, so the file was skipped."
                    .into()
            }
            Self::Catalog(C::BackupInvalid(_)) => {
                "That backup is damaged and cannot be restored.".into()
            }
            Self::Catalog(_) => {
                "Lightbox could not read or update its library. Restart Lightbox; if this keeps happening, restore a backup from Settings."
                    .into()
            }
            Self::Scan(lb_scan::ScanError::RootNotFound) => {
                "That folder is no longer part of the library.".into()
            }
            Self::Scan(lb_scan::ScanError::Watch(_)) => {
                "Lightbox could not watch folders for changes. New photos will appear after a rescan."
                    .into()
            }
            Self::Scan(lb_scan::ScanError::Io { path, .. }) => {
                format!("Could not read {}. Check that it exists and you have access.", path.display())
            }
            Self::Scan(_) => "Scanning the folder failed. Try again.".into(),
            Self::Thumb(lb_thumbs::ThumbError::Media(m)) => Self::media_message(m),
            Self::Thumb(_) => "A thumbnail could not be created.".into(),
            Self::Edit(e) => format!("That edit cannot be applied: {e}."),
            Self::Media(m) => Self::media_message(m),
            Self::Cancelled => "Cancelled.".into(),
            Self::Io(e) => match e.kind() {
                std::io::ErrorKind::PermissionDenied => {
                    "Lightbox does not have permission to do that.".into()
                }
                std::io::ErrorKind::NotFound => "The file or folder could not be found.".into(),
                std::io::ErrorKind::StorageFull => "There is not enough disk space.".into(),
                _ => "A file operation failed. Check that the disk is available and try again."
                    .into(),
            },
            Self::Invalid(msg) => {
                let mut m = msg.clone();
                if let Some(first) = m.get_mut(0..1) {
                    first.make_ascii_uppercase();
                }
                if !m.ends_with('.') {
                    m.push('.');
                }
                m
            }
            Self::AlreadyExists(path) => format!(
                "{} already exists. Choose another name.",
                path.file_name().map_or_else(
                    || path.display().to_string(),
                    |n| n.to_string_lossy().into_owned()
                )
            ),
        }
    }

    fn media_message(m: &lb_media::MediaError) -> String {
        use lb_media::MediaError as M;
        match m {
            M::Io(_) => "The image file could not be read.".into(),
            M::Corrupt(_) => "The image file is damaged and cannot be shown.".into(),
            M::Unsupported(_) => "This image format is not supported yet.".into(),
        }
    }
}

/// Result alias for service calls.
pub type Result<T, E = AppError> = std::result::Result<T, E>;
