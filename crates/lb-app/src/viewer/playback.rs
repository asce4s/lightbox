//! Animated GIF and WebP frames, clipboard pixels, and the transparency checkerboard
//! (SPEC-VIEWER §V9, F31).

use std::path::Path;
use std::time::Duration;

use image::codecs::gif::GifDecoder;
use image::codecs::png::PngEncoder;
use image::codecs::webp::WebPDecoder;
use image::{AnimationDecoder, ExtendedColorType, ImageEncoder, RgbaImage};
use std::io::BufReader;

use crate::error::{AppError, Result};

/// Dark square of the viewer checkerboard (`#2e2e2e`).
pub const CHECKER_DARK: [u8; 3] = [0x2e, 0x2e, 0x2e];
/// Light square of the viewer checkerboard (`#3a3a3a`).
pub const CHECKER_LIGHT: [u8; 3] = [0x3a, 0x3a, 0x3a];
/// Checker square size in image pixels.
pub const CHECKER_PX: u32 = 8;

/// Colour of the checkerboard under image pixel `(x, y)`.
#[must_use]
pub fn checker_rgb(x: u32, y: u32) -> [u8; 3] {
    let cell = x / CHECKER_PX + y / CHECKER_PX;
    if cell.is_multiple_of(2) {
        CHECKER_DARK
    } else {
        CHECKER_LIGHT
    }
}

/// Draws `image` over the checkerboard. Opaque pixels keep their colour.
#[must_use]
pub fn composite_on_checker(image: &RgbaImage) -> image::RgbImage {
    image::RgbImage::from_fn(image.width(), image.height(), |x, y| {
        let px = image.get_pixel(x, y).0;
        let under = checker_rgb(x, y);
        image::Rgb(over(px, under))
    })
}

fn over(src: [u8; 4], dst: [u8; 3]) -> [u8; 3] {
    let a = u32::from(src[3]);
    let inv = 255 - a;
    let mix =
        |s: u8, d: u8| u8::try_from((u32::from(s) * a + u32::from(d) * inv) / 255).unwrap_or(d);
    [
        mix(src[0], dst[0]),
        mix(src[1], dst[1]),
        mix(src[2], dst[2]),
    ]
}

/// What `copy image` places on the clipboard.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClipboardBody {
    /// PNG bytes. Used when the UI toolkit can put an image on the clipboard.
    Png(Vec<u8>),
    /// The file path, when the toolkit clipboard is text only.
    Path(std::path::PathBuf),
}

/// Encodes `image` as PNG, or returns `source` when image clipboard is unavailable.
pub fn image_for_clipboard(
    image: &RgbaImage,
    source: &Path,
    image_clipboard: bool,
) -> Result<ClipboardBody> {
    if !image_clipboard {
        return Ok(ClipboardBody::Path(source.to_path_buf()));
    }
    let mut bytes = Vec::new();
    PngEncoder::new(&mut bytes)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgba8,
        )
        .map_err(|error| {
            AppError::Invalid(format!("could not encode the clipboard image: {error}"))
        })?;
    Ok(ClipboardBody::Png(bytes))
}

/// One looping animation.
#[derive(Debug, Clone)]
pub struct Clip {
    delays: Vec<Duration>,
    frames: Vec<RgbaImage>,
    origin: Duration,
    paused_at: Option<Duration>,
}

impl Clip {
    /// Playback of `frames` and their delays, starting at `now`.
    #[must_use]
    pub fn new(frames: Vec<RgbaImage>, delays: Vec<Duration>, now: Duration) -> Self {
        Self {
            delays,
            frames,
            origin: now,
            paused_at: None,
        }
    }

    /// Whether `P` is holding the current frame.
    #[must_use]
    pub fn is_paused(&self) -> bool {
        self.paused_at.is_some()
    }

    /// Frame shown at `now`. Delays loop.
    #[must_use]
    pub fn index(&self, now: Duration) -> usize {
        frame_index(&self.delays, self.played(now))
    }

    /// The pixels of [`Self::index`].
    #[must_use]
    pub fn frame(&self, now: Duration) -> Option<&RgbaImage> {
        self.frames.get(self.index(now))
    }

    /// Starts the loop at `now` (used after a file is loaded).
    pub fn restart(&mut self, now: Duration) {
        self.origin = now;
        self.paused_at = None;
    }

    /// Pauses or resumes at `now`.
    pub fn toggle(&mut self, now: Duration) {
        if let Some(at) = self.paused_at.take() {
            let played = at.saturating_sub(self.origin);
            self.origin = now.saturating_sub(played);
        } else {
            self.paused_at = Some(now);
        }
    }

    fn played(&self, now: Duration) -> Duration {
        let end = self.paused_at.unwrap_or(now);
        end.saturating_sub(self.origin)
    }
}

/// Index of the frame that should be on screen after `elapsed` of playback.
#[must_use]
pub fn frame_index(delays: &[Duration], elapsed: Duration) -> usize {
    if delays.is_empty() {
        return 0;
    }
    let total = delays.iter().map(|delay| delay.as_millis()).sum::<u128>();
    if total == 0 {
        return 0;
    }
    let mut t = elapsed.as_millis() % total;
    for (index, delay) in delays.iter().enumerate() {
        let ms = delay.as_millis();
        if t < ms || index + 1 == delays.len() {
            return index;
        }
        t -= ms;
    }
    0
}

/// Loads a multi-frame GIF or WebP. A still image is `Ok(None)`.
pub fn read_animation(path: &Path) -> Result<Option<Clip>> {
    let ext = path
        .extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let file = std::fs::File::open(path)?;
    let reader = BufReader::new(file);
    let frames = match ext.as_str() {
        "gif" => decode_frames(GifDecoder::new(reader).map_err(image_error)?)?,
        "webp" => decode_frames(WebPDecoder::new(reader).map_err(image_error)?)?,
        _ => return Ok(None),
    };
    if frames.len() < 2 {
        return Ok(None);
    }
    let (buffers, delays): (Vec<_>, Vec<_>) = frames.into_iter().unzip();
    Ok(Some(Clip::new(buffers, delays, Duration::ZERO)))
}

fn decode_frames<'a>(decoder: impl AnimationDecoder<'a>) -> Result<Vec<(RgbaImage, Duration)>> {
    let frames = decoder
        .into_frames()
        .collect_frames()
        .map_err(image_error)?;
    Ok(frames
        .into_iter()
        .map(|frame| {
            let (numer, denom) = frame.delay().numer_denom_ms();
            let ms = numer.checked_div(denom).unwrap_or(0);
            (frame.into_buffer(), Duration::from_millis(u64::from(ms)))
        })
        .collect())
}

fn image_error(error: image::ImageError) -> AppError {
    AppError::Invalid(error.to_string())
}

/// Decodes a still image (the first frame of an animation).
pub fn decode_rgba(path: &Path) -> Result<RgbaImage> {
    let image =
        lb_media::decode(path, None).map_err(|error| AppError::Invalid(error.to_string()))?;
    Ok(image.to_rgba8())
}

/// PNG bytes of `path` for the clipboard. Image clipboard is supported by GPUI.
pub fn clipboard_for_path(path: &Path) -> Result<ClipboardBody> {
    let image = decode_rgba(path)?;
    image_for_clipboard(&image, path, true)
}

/// Writes `image` as a PNG file.
pub fn save_rgba_png(path: &Path, image: &RgbaImage) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut bytes = Vec::new();
    PngEncoder::new(&mut bytes)
        .write_image(
            image.as_raw(),
            image.width(),
            image.height(),
            ExtendedColorType::Rgba8,
        )
        .map_err(image_error)?;
    crate::atomic::write_bytes_atomic(path, &bytes)?;
    Ok(())
}

/// Writes `image` composited on the checkerboard.
pub fn save_checker_png(path: &Path, image: &RgbaImage) -> Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let rgb = composite_on_checker(image);
    let mut bytes = Vec::new();
    PngEncoder::new(&mut bytes)
        .write_image(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            ExtendedColorType::Rgb8,
        )
        .map_err(image_error)?;
    crate::atomic::write_bytes_atomic(path, &bytes)?;
    Ok(())
}

/// Whether any pixel is not fully opaque.
#[must_use]
pub fn has_alpha(image: &RgbaImage) -> bool {
    image.pixels().any(|pixel| pixel.0[3] < 255)
}

/// Reads PNG bytes back. Tests use this to check the clipboard encoding.
#[cfg(test)]
fn decode_png(bytes: &[u8]) -> Result<RgbaImage> {
    let cursor = std::io::Cursor::new(bytes);
    let decoder = image::codecs::png::PngDecoder::new(cursor).map_err(image_error)?;
    image::DynamicImage::from_decoder(decoder)
        .map(|image| image.to_rgba8())
        .map_err(image_error)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::codecs::gif::{GifEncoder, Repeat};
    use image::{Delay, Frame, Rgba};

    #[test]
    fn viewer_gif_frames_follow_delays() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("anim.gif");
        let mut bytes = Vec::new();
        {
            let mut enc = GifEncoder::new(&mut bytes);
            enc.set_repeat(Repeat::Infinite).unwrap();
            for (index, delay_ms) in [100_u32, 200, 50].into_iter().enumerate() {
                let mut frame = RgbaImage::new(2, 2);
                frame.put_pixel(0, 0, Rgba([u8::try_from(index).unwrap(), 0, 0, 255]));
                enc.encode_frame(Frame::from_parts(
                    frame,
                    0,
                    0,
                    Delay::from_numer_denom_ms(delay_ms, 1),
                ))
                .unwrap();
            }
        }
        std::fs::write(&path, bytes).unwrap();
        let mut clip = read_animation(&path)
            .unwrap()
            .expect("gif has three frames");
        assert_eq!(
            clip.delays,
            vec![
                Duration::from_millis(100),
                Duration::from_millis(200),
                Duration::from_millis(50),
            ]
        );
        assert_eq!(clip.frames[0].get_pixel(0, 0).0[0], 0);
        assert_eq!(clip.frames[1].get_pixel(0, 0).0[0], 1);
        assert_eq!(clip.index(Duration::from_millis(0)), 0);
        assert_eq!(clip.index(Duration::from_millis(100)), 1);
        assert_eq!(clip.index(Duration::from_millis(299)), 1);
        assert_eq!(clip.index(Duration::from_millis(300)), 2);
        assert_eq!(clip.index(Duration::from_millis(350)), 0);
        clip.toggle(Duration::from_millis(150));
        assert!(clip.is_paused());
        assert_eq!(clip.index(Duration::from_millis(10_000)), 1);
        clip.toggle(Duration::from_millis(10_000));
        assert_eq!(clip.index(Duration::from_millis(10_150)), 2);
    }

    #[test]
    fn viewer_copy_image_to_clipboard() {
        let mut image = RgbaImage::new(2, 2);
        image.put_pixel(0, 0, Rgba([9, 8, 7, 255]));
        image.put_pixel(1, 0, Rgba([1, 2, 3, 0]));
        let source = Path::new("/photos/still.png");
        let ClipboardBody::Png(bytes) = image_for_clipboard(&image, source, true).unwrap() else {
            panic!("image clipboard");
        };
        let decoded = decode_png(&bytes).unwrap();
        assert_eq!(decoded.get_pixel(0, 0).0, [9, 8, 7, 255]);
        assert_eq!(decoded.get_pixel(1, 0).0, [1, 2, 3, 0]);
        let ClipboardBody::Path(path) = image_for_clipboard(&image, source, false).unwrap() else {
            panic!("text clipboard");
        };
        assert_eq!(path, source);
    }

    #[test]
    fn viewer_checkerboard_for_alpha() {
        let mut image = RgbaImage::new(16, 8);
        image.put_pixel(0, 0, Rgba([0, 0, 0, 0]));
        image.put_pixel(8, 0, Rgba([0, 0, 0, 0]));
        image.put_pixel(1, 1, Rgba([200, 10, 10, 255]));
        let out = composite_on_checker(&image);
        assert_eq!(out.get_pixel(0, 0).0, CHECKER_DARK);
        assert_eq!(out.get_pixel(8, 0).0, CHECKER_LIGHT);
        assert_eq!(out.get_pixel(1, 1).0, [200, 10, 10]);
        assert_eq!(checker_rgb(0, 0), CHECKER_DARK);
        assert_eq!(CHECKER_PX, 8);
    }
}
