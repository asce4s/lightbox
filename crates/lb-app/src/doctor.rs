//! Environment report for `lightbox-cli doctor` and the settings dialog (SPEC §8, §13).

use crate::error::Result;
use crate::lightbox::Lightbox;
use serde::Serialize;
use std::path::{Path, PathBuf};

/// Environment variable pointing at the directory with face-detection models.
pub const FACE_MODELS_ENV: &str = "LIGHTBOX_FACE_MODELS";

/// What the doctor found.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct DoctorReport {
    /// Data directory holding the catalog.
    pub data_dir: PathBuf,
    /// Cache directory holding thumbnails.
    pub cache_dir: PathBuf,
    /// Schema version of the catalog.
    pub schema_version: u32,
    /// Problems reported by `PRAGMA quick_check`; empty when healthy.
    pub integrity_problems: Vec<String>,
    /// Number of indexed photos and videos.
    pub photos: usize,
    /// Number of configured roots.
    pub roots: usize,
    /// Path of `ffmpeg` on `PATH`, if any.
    pub ffmpeg: Option<PathBuf>,
    /// Path of `ffprobe` on `PATH`, if any.
    pub ffprobe: Option<PathBuf>,
    /// Face model directory from the environment and whether it exists.
    pub face_models: Option<(PathBuf, bool)>,
    /// Features compiled into this build.
    pub features: Vec<&'static str>,
}

impl DoctorReport {
    /// Whether nothing needs attention (missing optional tools do not count).
    pub fn healthy(&self) -> bool {
        self.integrity_problems.is_empty()
    }
}

fn find_on_path(name: &str) -> Option<PathBuf> {
    let exe = if cfg!(windows) {
        format!("{name}.exe")
    } else {
        name.to_owned()
    };
    std::env::split_paths(&std::env::var_os("PATH")?)
        .map(|dir| dir.join(&exe))
        .find(|p| Path::new(p).is_file())
}

impl Lightbox {
    /// Checks the catalog and the optional tools Lightbox can use.
    pub fn doctor(&self) -> Result<DoctorReport> {
        let mut features = Vec::new();
        if cfg!(feature = "faces") {
            features.push("faces");
        }
        if cfg!(feature = "video") {
            features.push("video");
        }
        if cfg!(feature = "map") {
            features.push("map");
        }
        Ok(DoctorReport {
            data_dir: self.paths().data.clone(),
            cache_dir: self.paths().cache.clone(),
            schema_version: lb_catalog::LATEST_SCHEMA_VERSION,
            integrity_problems: self.catalog().quick_check()?,
            photos: self.catalog().all_photo_ids()?.len(),
            roots: self.library().roots()?.len(),
            ffmpeg: find_on_path("ffmpeg"),
            ffprobe: find_on_path("ffprobe"),
            face_models: std::env::var_os(FACE_MODELS_ENV).map(|p| {
                let p = PathBuf::from(p);
                let exists = p.is_dir();
                (p, exists)
            }),
            features,
        })
    }
}
