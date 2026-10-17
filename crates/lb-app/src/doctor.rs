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
    /// Problems reported by `PRAGMA integrity_check`; empty when healthy.
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
    /// Whether the face-detection models are in place and match their checksums.
    pub face_models_ready: bool,
    /// Display server and GPU hints for this machine.
    pub graphics: GraphicsReport,
    /// Features compiled into this build.
    pub features: Vec<&'static str>,
}

/// What can be told about the display and GPU without opening a window.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct GraphicsReport {
    /// Windowing/graphics system the app will use, e.g. `x11`, `wayland`, `metal`.
    pub session: String,
    /// GPU render nodes found (`/dev/dri/renderD*` on Linux).
    pub render_nodes: Vec<PathBuf>,
}

impl GraphicsReport {
    /// Inspects the environment.
    pub fn detect() -> Self {
        if cfg!(target_os = "macos") {
            return Self {
                session: "metal".into(),
                render_nodes: Vec::new(),
            };
        }
        if cfg!(windows) {
            return Self {
                session: "direct3d".into(),
                render_nodes: Vec::new(),
            };
        }
        let set = |name: &str| std::env::var_os(name).is_some_and(|v| !v.is_empty());
        let session = if set("WAYLAND_DISPLAY") {
            "wayland"
        } else if set("DISPLAY") {
            "x11"
        } else {
            "none (no WAYLAND_DISPLAY or DISPLAY)"
        };
        let mut render_nodes: Vec<PathBuf> = std::fs::read_dir("/dev/dri")
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .map(|e| e.path())
                    .filter(|p| {
                        p.file_name()
                            .is_some_and(|n| n.to_string_lossy().starts_with("renderD"))
                    })
                    .collect()
            })
            .unwrap_or_default();
        render_nodes.sort();
        Self {
            session: session.into(),
            render_nodes,
        }
    }
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
    #[cfg(feature = "faces")]
    fn face_models_ready(&self) -> bool {
        let dir = std::env::var_os(FACE_MODELS_ENV)
            .map(PathBuf::from)
            .unwrap_or_else(|| self.paths().models());
        lb_faces::models_present(&dir)
    }

    #[cfg(not(feature = "faces"))]
    fn face_models_ready(&self) -> bool {
        false
    }

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
            integrity_problems: self.catalog().integrity_check()?,
            photos: self.catalog().all_photo_ids()?.len(),
            roots: self.library().roots()?.len(),
            ffmpeg: find_on_path("ffmpeg"),
            ffprobe: find_on_path("ffprobe"),
            face_models: std::env::var_os(FACE_MODELS_ENV).map(|p| {
                let p = PathBuf::from(p);
                let exists = p.is_dir();
                (p, exists)
            }),
            face_models_ready: self.face_models_ready(),
            graphics: GraphicsReport::detect(),
            features,
        })
    }
}
