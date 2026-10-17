//! Rendering photos to an MP4 slideshow movie (SPEC §6.12.3, feature `video`).

use crate::atomic::temp_path;
use crate::error::{AppError, Result};
use crate::lightbox::Lightbox;
use image::DynamicImage;
use lb_core::{MediaType, PhotoId};
use lb_video::VideoError;
use std::path::{Path, PathBuf};

pub use lb_video::MovieSettings;

/// What a movie render produced.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MovieReport {
    /// The finished file; `None` when the render was cancelled.
    pub path: Option<PathBuf>,
    /// Photos in the movie.
    pub photos: usize,
    /// Photos that could not be read or are videos, and so are not in the movie.
    pub skipped: Vec<PhotoId>,
    /// Whether the render was cancelled (no file is left behind).
    pub cancelled: bool,
    /// Length of the movie in seconds.
    pub duration_secs: f64,
}

/// Turns a list of photos into a slideshow movie with `ffmpeg`.
#[derive(Clone)]
pub struct MovieService {
    lb: Lightbox,
}

impl Lightbox {
    /// The movie maker.
    pub fn movie(&self) -> MovieService {
        MovieService { lb: self.clone() }
    }
}

impl MovieService {
    /// Whether `ffmpeg` and `ffprobe` were found (configured path or `PATH`).
    pub fn available(&self) -> bool {
        crate::video::detect(self.lb.catalog()).is_some()
    }

    /// Writes the photos `ids`, in that order and with their edits applied, to `out` as an
    /// H.264 MP4 (each photo shown for `settings.seconds_per_photo`, letterboxed on black).
    /// Videos and unreadable photos are skipped. `is_cancelled` is polled between photos; a
    /// cancelled or failed render leaves no file. `progress` receives `(fraction, label)`.
    pub fn render(
        &self,
        ids: &[PhotoId],
        settings: &MovieSettings,
        out: &Path,
        is_cancelled: &dyn Fn() -> bool,
        progress: &dyn Fn(f32, &str),
    ) -> Result<MovieReport> {
        if ids.is_empty() {
            return Err(AppError::Invalid(
                "there are no photos for the movie".into(),
            ));
        }
        let tools = crate::video::detect(self.lb.catalog()).ok_or_else(|| {
            AppError::Invalid("the movie maker needs ffmpeg, which was not found".into())
        })?;
        let settings = settings.normalized();
        let edge = settings.width.max(settings.height);
        let tmp = temp_path(out);
        let mut report = MovieReport::default();
        let total = ids.len();
        let mut done = 0usize;
        let mut next = ids.iter();
        let mut frames = std::iter::from_fn(|| {
            for id in next.by_ref() {
                if is_cancelled() {
                    report.cancelled = true;
                    return None;
                }
                done += 1;
                progress(
                    done as f32 / total as f32,
                    &format!("Photo {done} of {total}"),
                );
                match self.frame(*id, edge) {
                    Some(image) => return Some(image),
                    None => report.skipped.push(*id),
                }
            }
            None
        });
        let written = tools.make_movie(&tmp, &settings, &mut frames);
        let finish = || -> Result<usize> {
            let written = written.map_err(|e| match e {
                VideoError::Io(io) => AppError::Io(io),
                other => AppError::Invalid(other.to_string()),
            })?;
            if report.cancelled || written == 0 {
                return Ok(0);
            }
            std::fs::rename(&tmp, out)?;
            Ok(written)
        };
        let written = finish();
        if report.cancelled || !matches!(written, Ok(n) if n > 0) {
            let _ = std::fs::remove_file(&tmp);
        }
        let written = written?;
        if report.cancelled {
            return Ok(report);
        }
        if written == 0 {
            return Err(AppError::Invalid("none of the photos could be read".into()));
        }
        report.photos = written;
        report.path = Some(out.to_path_buf());
        report.duration_secs = written as f64 * settings.seconds_per_photo;
        Ok(report)
    }

    fn frame(&self, id: PhotoId, edge: u32) -> Option<DynamicImage> {
        let photo = self.lb.photos().get(id).ok().flatten()?;
        if photo.media_type != MediaType::Image {
            return None;
        }
        match self.lb.edits().render_preview(id, edge) {
            Ok(image) => Some(DynamicImage::ImageRgba8(image)),
            Err(e) => {
                tracing::warn!(photo = id.get(), error = %e, "photo skipped in movie");
                None
            }
        }
    }
}
