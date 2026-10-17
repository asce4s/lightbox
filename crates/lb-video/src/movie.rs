//! Rendering a slideshow to an MP4 through the `ffmpeg` command line (SPEC §6.12.3).
//!
//! Frames are piped to ffmpeg as raw RGB24 and encoded as H.264 `yuv420p`.

use crate::command::CommandSpec;
use crate::tools::Tools;
use crate::{VideoError, VideoTools};
use image::{DynamicImage, RgbImage, imageops::FilterType};
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Largest output size (1080p).
pub const MAX_SIZE: (u32, u32) = (1920, 1080);

/// Frame rate range.
pub const FPS_RANGE: (u32, u32) = (1, 60);

/// Seconds per photo range.
pub const SECONDS_RANGE: (f64, f64) = (0.2, 60.0);

/// What the movie looks like.
#[derive(Debug, Clone, PartialEq)]
pub struct MovieSettings {
    /// Frames per second.
    pub fps: u32,
    /// Output width in pixels (even, at most 1920).
    pub width: u32,
    /// Output height in pixels (even, at most 1080).
    pub height: u32,
    /// How long each photo is shown.
    pub seconds_per_photo: f64,
    /// Soundtrack, cut to the length of the movie.
    pub audio: Option<PathBuf>,
}

impl Default for MovieSettings {
    fn default() -> Self {
        Self {
            fps: 30,
            width: MAX_SIZE.0,
            height: MAX_SIZE.1,
            seconds_per_photo: 3.0,
            audio: None,
        }
    }
}

impl MovieSettings {
    /// The same settings forced into the supported ranges: at most 1080p, even dimensions (H.264
    /// `yuv420p` needs them), 1 to 60 fps.
    pub fn normalized(&self) -> Self {
        let even = |v: u32, max: u32| (v.clamp(16, max) / 2 * 2).max(16);
        Self {
            fps: self.fps.clamp(FPS_RANGE.0, FPS_RANGE.1),
            width: even(self.width, MAX_SIZE.0),
            height: even(self.height, MAX_SIZE.1),
            seconds_per_photo: self
                .seconds_per_photo
                .clamp(SECONDS_RANGE.0, SECONDS_RANGE.1),
            audio: self.audio.clone(),
        }
    }

    /// How many frames each photo occupies (at least one).
    pub fn frames_per_photo(&self) -> u32 {
        ((self.seconds_per_photo * f64::from(self.fps)).round() as u32).max(1)
    }

    /// Bytes of one raw RGB24 frame.
    pub fn frame_len(&self) -> usize {
        self.width as usize * self.height as usize * 3
    }
}

/// Fits `img` inside `width` x `height`, centred on black, as raw RGB24 bytes.
pub fn letterbox(img: &DynamicImage, width: u32, height: u32) -> Vec<u8> {
    let (w, h) = (img.width().max(1), img.height().max(1));
    let scale = (f64::from(width) / f64::from(w)).min(f64::from(height) / f64::from(h));
    let fit_w = ((f64::from(w) * scale).round() as u32).clamp(1, width);
    let fit_h = ((f64::from(h) * scale).round() as u32).clamp(1, height);
    let scaled = img
        .resize_exact(fit_w, fit_h, FilterType::Triangle)
        .to_rgb8();
    let mut canvas = RgbImage::new(width, height);
    image::imageops::replace(
        &mut canvas,
        &scaled,
        i64::from((width - fit_w) / 2),
        i64::from((height - fit_h) / 2),
    );
    canvas.into_raw()
}

impl Tools {
    /// ffmpeg reading raw RGB24 frames of the (normalized) size from standard input and writing
    /// an H.264 `yuv420p` MP4 to `out`, with the optional soundtrack cut to the video length.
    pub fn movie_command(&self, out: &Path, settings: &MovieSettings) -> CommandSpec {
        let s = settings.normalized();
        let mut args: Vec<OsString> = ["-v", "error", "-y"].iter().map(OsString::from).collect();
        args.extend(
            [
                "-f",
                "rawvideo",
                "-pix_fmt",
                "rgb24",
                "-s",
                &format!("{}x{}", s.width, s.height),
                "-framerate",
                &s.fps.to_string(),
                "-i",
                "pipe:0",
            ]
            .iter()
            .map(OsString::from),
        );
        if let Some(audio) = &s.audio {
            args.extend(["-i".into(), audio.as_os_str().to_owned()]);
            args.extend(
                ["-map", "0:v:0", "-map", "1:a:0"]
                    .iter()
                    .map(OsString::from),
            );
            args.extend(["-c:a", "aac", "-shortest"].iter().map(OsString::from));
        }
        args.extend(
            [
                "-c:v",
                "libx264",
                "-pix_fmt",
                "yuv420p",
                "-r",
                &s.fps.to_string(),
                "-movflags",
                "+faststart",
                "-f",
                "mp4",
            ]
            .iter()
            .map(OsString::from),
        );
        args.push(out.as_os_str().to_owned());
        CommandSpec {
            program: self.ffmpeg.clone(),
            args,
        }
    }
}

impl VideoTools {
    /// Encodes `photos` into the MP4 at `out`, each shown for
    /// [`MovieSettings::frames_per_photo`] frames, and returns the number of photos written.
    /// `photos` is read lazily so large libraries are never held in memory; ending it early
    /// ends the movie there.
    ///
    /// # Errors
    /// [`VideoError::Tool`] when ffmpeg rejects the input or the settings, and
    /// [`VideoError::Io`] when it cannot be started.
    pub fn make_movie(
        &self,
        out: &Path,
        settings: &MovieSettings,
        photos: &mut dyn Iterator<Item = DynamicImage>,
    ) -> Result<usize, VideoError> {
        let settings = settings.normalized();
        let spec = self.tools().movie_command(out, &settings);
        let repeats = settings.frames_per_photo();
        let mut written = 0usize;
        let output = self.runner().run_with_input(&spec, &mut |stdin| {
            for photo in &mut *photos {
                let frame = letterbox(&photo, settings.width, settings.height);
                for _ in 0..repeats {
                    stdin.write_all(&frame)?;
                }
                written += 1;
            }
            Ok(())
        })?;
        if !output.success {
            return Err(VideoError::Tool {
                tool: "ffmpeg",
                message: String::from_utf8_lossy(&output.stderr).trim().to_owned(),
            });
        }
        Ok(written)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgb;

    #[test]
    fn settings_are_forced_into_range() {
        let s = MovieSettings {
            fps: 500,
            width: 4001,
            height: 7,
            seconds_per_photo: 0.0,
            audio: None,
        }
        .normalized();
        assert_eq!((s.fps, s.width, s.height), (60, 1920, 16));
        assert!((s.seconds_per_photo - 0.2).abs() < 1e-9);
        let odd = MovieSettings {
            width: 641,
            height: 359,
            ..MovieSettings::default()
        }
        .normalized();
        assert_eq!((odd.width, odd.height), (640, 358));
    }

    #[test]
    fn frames_per_photo_follows_fps_and_duration() {
        let s = MovieSettings::default();
        assert_eq!(s.frames_per_photo(), 90);
        let quick = MovieSettings {
            fps: 10,
            seconds_per_photo: 0.2,
            ..MovieSettings::default()
        };
        assert_eq!(quick.frames_per_photo(), 2);
        assert_eq!(quick.frame_len(), 1920 * 1080 * 3);
    }

    #[test]
    fn letterbox_centres_on_black() {
        let wide = DynamicImage::ImageRgb8(RgbImage::from_pixel(40, 10, Rgb([200, 100, 50])));
        let bytes = letterbox(&wide, 20, 20);
        assert_eq!(bytes.len(), 20 * 20 * 3);
        let px = |x: usize, y: usize| &bytes[(y * 20 + x) * 3..][..3];
        assert_eq!(px(10, 0), [0, 0, 0], "black bar on top");
        assert_eq!(px(10, 10), [200, 100, 50], "picture in the middle");
        assert_eq!(px(10, 19), [0, 0, 0], "black bar below");
    }
}
