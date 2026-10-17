//! Video support: finding ffmpeg (SPEC §6.16).

use crate::settings::load_settings;
use lb_catalog::Catalog;
use lb_video::{Tools, VideoTools};
use std::path::PathBuf;

/// Finds ffmpeg and ffprobe using the configured path or `PATH`.
pub(crate) fn detect(catalog: &Catalog) -> Option<VideoTools> {
    let configured = load_settings(catalog)
        .ok()
        .and_then(|s| s.video.ffmpeg_path)
        .map(PathBuf::from);
    let tools = Tools::detect(configured.as_deref());
    if tools.is_none() {
        tracing::warn!(
            "ffmpeg/ffprobe not found; videos are indexed without metadata or thumbnails"
        );
    }
    tools.map(VideoTools::new)
}
