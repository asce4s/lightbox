//! Where Lightbox keeps its data (SPEC §4.4).

use crate::error::{AppError, Result};
use std::path::{Path, PathBuf};

/// Environment variable that redirects data and cache directories under one root.
pub const LIGHTBOX_HOME_ENV: &str = "LIGHTBOX_HOME";

/// Every directory and file Lightbox owns.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppPaths {
    /// Data directory (catalog, models, logs, backups).
    pub data: PathBuf,
    /// Cache directory (thumbnails, map tiles).
    pub cache: PathBuf,
}

impl AppPaths {
    /// `<home>/data` and `<home>/cache`, as `LIGHTBOX_HOME` prescribes.
    pub fn under(home: &Path) -> Self {
        Self {
            data: home.join("data"),
            cache: home.join("cache"),
        }
    }

    /// Resolves the directories for the running process: `LIGHTBOX_HOME` if set, otherwise the
    /// platform's per-user data and cache directories.
    pub fn resolve() -> Result<Self> {
        if let Some(home) = std::env::var_os(LIGHTBOX_HOME_ENV).filter(|h| !h.is_empty()) {
            return Ok(Self::under(Path::new(&home)));
        }
        let dirs = directories::ProjectDirs::from("app", "Lightbox", "Lightbox")
            .ok_or_else(|| AppError::Invalid("no home directory could be determined".into()))?;
        Ok(Self {
            data: dirs.data_dir().to_path_buf(),
            cache: dirs.cache_dir().to_path_buf(),
        })
    }

    /// `{data}/catalog.sqlite3`.
    pub fn catalog(&self) -> PathBuf {
        self.data.join("catalog.sqlite3")
    }

    /// `{data}/models`.
    pub fn models(&self) -> PathBuf {
        self.data.join("models")
    }

    /// `{data}/logs`.
    pub fn logs(&self) -> PathBuf {
        self.data.join("logs")
    }

    /// `{data}/backups`.
    pub fn backups(&self) -> PathBuf {
        self.data.join("backups")
    }

    /// `{cache}/thumbs`.
    pub fn thumbs(&self) -> PathBuf {
        self.cache.join("thumbs")
    }

    /// `{cache}/tiles`.
    pub fn tiles(&self) -> PathBuf {
        self.cache.join("tiles")
    }

    /// Creates every directory.
    pub fn ensure(&self) -> Result<()> {
        for dir in [
            &self.data,
            &self.cache,
            &self.models(),
            &self.logs(),
            &self.backups(),
            &self.thumbs(),
            &self.tiles(),
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}
