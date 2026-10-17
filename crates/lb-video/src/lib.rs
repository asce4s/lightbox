//! Video metadata and thumbnails through `ffprobe` and `ffmpeg` (SPEC §6.16).
#![warn(missing_docs)]

mod command;
mod info;
mod tools;

pub use command::{CommandOutput, CommandRunner, CommandSpec, SystemRunner};
pub use info::{VideoInfo, parse_ffprobe, parse_iso6709};
pub use tools::{Tools, thumbnail_time};

use chrono::{Local, NaiveDateTime, TimeZone};
use image::DynamicImage;
use lb_core::Format;
use lb_media::{Decoder, DefaultDecoder, MediaError, VideoMetadata, VideoProber};
use std::path::Path;
use std::sync::Arc;

/// Longest edge of frames the decoder asks ffmpeg for.
pub const FRAME_EDGE: u32 = 1280;

/// What can go wrong while talking to the tools.
#[derive(Debug, thiserror::Error)]
pub enum VideoError {
    /// ffprobe output could not be understood.
    #[error("could not read video metadata: {0}")]
    Probe(String),
    /// The tool ran but failed.
    #[error("{tool} failed: {message}")]
    Tool {
        /// Which executable.
        tool: &'static str,
        /// Its error output.
        message: String,
    },
    /// The tool could not be started.
    #[error("could not run the tool: {0}")]
    Io(#[from] std::io::Error),
}

impl From<VideoError> for MediaError {
    fn from(e: VideoError) -> Self {
        match e {
            VideoError::Io(io) => Self::Io(io),
            other => Self::Corrupt(other.to_string()),
        }
    }
}

fn utc_to_local(utc: NaiveDateTime) -> NaiveDateTime {
    Local.from_utc_datetime(&utc).naive_local()
}

/// Runs ffprobe and ffmpeg through a [`CommandRunner`].
#[derive(Debug, Clone)]
pub struct VideoTools {
    tools: Tools,
    runner: Arc<dyn CommandRunner>,
}

impl VideoTools {
    /// Uses `tools` with the real process runner.
    pub fn new(tools: Tools) -> Self {
        Self::with_runner(tools, Arc::new(SystemRunner::default()))
    }

    /// Uses `tools` with an injected runner.
    pub fn with_runner(tools: Tools, runner: Arc<dyn CommandRunner>) -> Self {
        Self { tools, runner }
    }

    /// Runs ffprobe on `path`.
    pub fn info(&self, path: &Path) -> Result<VideoInfo, VideoError> {
        let out = self.runner.run(&self.tools.probe_command(path))?;
        if !out.success {
            return Err(VideoError::Tool {
                tool: "ffprobe",
                message: String::from_utf8_lossy(&out.stderr).trim().to_owned(),
            });
        }
        parse_ffprobe(&String::from_utf8_lossy(&out.stdout))
    }

    /// A frame of `path` 10 % into the video, at most `edge` pixels on the long side. Falls back
    /// to the first frame when the seek lands past the end.
    pub fn frame(&self, path: &Path, edge: u32) -> Result<DynamicImage, VideoError> {
        let info = self.info(path)?;
        let at = thumbnail_time(info.duration_ms);
        let mut last = None;
        for t in [at, 0.0] {
            let out = self
                .runner
                .run(&self.tools.thumbnail_command(path, t, edge))?;
            if out.success && !out.stdout.is_empty() {
                return image::load_from_memory_with_format(&out.stdout, image::ImageFormat::Png)
                    .map_err(|e| VideoError::Tool {
                        tool: "ffmpeg",
                        message: e.to_string(),
                    });
            }
            last = Some(String::from_utf8_lossy(&out.stderr).trim().to_owned());
            if at == 0.0 {
                break;
            }
        }
        Err(VideoError::Tool {
            tool: "ffmpeg",
            message: last.unwrap_or_default(),
        })
    }
}

impl VideoProber for VideoTools {
    fn probe(&self, path: &Path) -> lb_media::Result<VideoMetadata> {
        let info = self.info(path)?;
        let (width, height) = info.display_size();
        Ok(VideoMetadata {
            width,
            height,
            duration_ms: info.duration_ms,
            creation_time: info.creation_time_utc.map(utc_to_local),
            location: info.location,
        })
    }
}

/// Decodes stills with the built-in decoder and video frames through ffmpeg, so thumbnails of
/// videos come out of the same cache as thumbnails of photos.
#[derive(Debug, Clone)]
pub struct VideoDecoder {
    tools: VideoTools,
}

impl VideoDecoder {
    /// A decoder that extracts video frames with `tools`.
    pub fn new(tools: VideoTools) -> Self {
        Self { tools }
    }
}

impl Decoder for VideoDecoder {
    fn decode_raw(
        &self,
        path: &Path,
        format: Format,
        max_long_edge: Option<u32>,
    ) -> lb_media::Result<DynamicImage> {
        if format.media_type() == lb_core::MediaType::Video {
            let edge = max_long_edge.unwrap_or(FRAME_EDGE).clamp(16, FRAME_EDGE);
            return Ok(self.tools.frame(path, edge)?);
        }
        DefaultDecoder.decode_raw(path, format, max_long_edge)
    }
}
