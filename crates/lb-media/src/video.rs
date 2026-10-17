//! What the scanner needs to know about a video, and the seam that supplies it (SPEC §6.16).

use crate::Result;
use chrono::NaiveDateTime;
use lb_core::GeoPoint;
use std::path::Path;

/// Metadata read from a video container.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoMetadata {
    /// Display width in pixels (rotation applied).
    pub width: u32,
    /// Display height in pixels (rotation applied).
    pub height: u32,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Container creation time as local wall-clock time.
    pub creation_time: Option<NaiveDateTime>,
    /// Recorded position.
    pub location: Option<GeoPoint>,
}

/// Reads video metadata; implemented on top of `ffprobe` by `lb-video` and faked in tests.
pub trait VideoProber: Send + Sync + std::fmt::Debug {
    /// Reads the metadata of the video at `path`.
    ///
    /// # Errors
    /// Fails when the file cannot be read or is not a video.
    fn probe(&self, path: &Path) -> Result<VideoMetadata>;
}
