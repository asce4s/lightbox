//! Image comparison with diff artifacts (SPEC §12.2, §12.4).

use image::{Rgba, RgbaImage};
use std::fmt;
use std::path::{Path, PathBuf};

/// Pixel-difference statistics between two images of equal size.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ImageDiff {
    /// Largest absolute per-channel difference (0..=255).
    pub max_diff: u8,
    /// Peak signal-to-noise ratio in dB over all RGBA channels; `INFINITY` when identical.
    pub psnr: f64,
}

/// Why two images were judged different.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageMismatch {
    /// Human readable explanation including the measured numbers.
    pub reason: String,
    /// Directory the `actual/expected/diff` PNGs were written to, if writing succeeded.
    pub artifacts: Option<PathBuf>,
}

impl fmt::Display for ImageMismatch {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.reason)?;
        if let Some(dir) = &self.artifacts {
            write!(f, " (artifacts: {})", dir.display())?;
        }
        Ok(())
    }
}

/// Computes difference statistics; `None` when the dimensions differ.
pub fn compare_images(actual: &RgbaImage, expected: &RgbaImage) -> Option<ImageDiff> {
    if actual.dimensions() != expected.dimensions() {
        return None;
    }
    let mut max_diff = 0u8;
    let mut sq_sum = 0f64;
    for (a, e) in actual.as_raw().iter().zip(expected.as_raw()) {
        let d = a.abs_diff(*e);
        max_diff = max_diff.max(d);
        sq_sum += f64::from(d) * f64::from(d);
    }
    let n = actual.as_raw().len().max(1) as f64;
    let mse = sq_sum / n;
    let psnr = if mse == 0.0 {
        f64::INFINITY
    } else {
        10.0 * (255.0f64 * 255.0 / mse).log10()
    };
    Some(ImageDiff { max_diff, psnr })
}

fn diff_image(actual: &RgbaImage, expected: &RgbaImage) -> RgbaImage {
    let (w, h) = (
        actual.width().max(expected.width()),
        actual.height().max(expected.height()),
    );
    RgbaImage::from_fn(w, h, |x, y| {
        match (
            actual.get_pixel_checked(x, y),
            expected.get_pixel_checked(x, y),
        ) {
            (Some(a), Some(e)) => {
                let amp = |i: usize| a.0[i].abs_diff(e.0[i]).saturating_mul(8);
                Rgba([amp(0), amp(1), amp(2), 255])
            }
            _ => Rgba([255, 0, 255, 255]),
        }
    })
}

fn write_artifacts(dir: &Path, actual: &RgbaImage, expected: &RgbaImage) -> Option<PathBuf> {
    let result = (|| -> image::ImageResult<()> {
        std::fs::create_dir_all(dir)?;
        actual.save(dir.join("actual.png"))?;
        expected.save(dir.join("expected.png"))?;
        diff_image(actual, expected).save(dir.join("diff.png"))
    })();
    match result {
        Ok(()) => Some(dir.to_path_buf()),
        Err(e) => {
            tracing::warn!(dir = %dir.display(), error = %e, "could not write image diff artifacts");
            None
        }
    }
}

/// Checks `actual` against `expected`; on mismatch writes `actual.png`, `expected.png` and
/// `diff.png` into `artifact_dir`.
///
/// Passes when the largest per-channel difference is `<= max_diff` and PSNR is `>= min_psnr`.
pub fn check_images_close(
    actual: &RgbaImage,
    expected: &RgbaImage,
    max_diff: u8,
    min_psnr: f64,
    artifact_dir: &Path,
) -> Result<(), ImageMismatch> {
    let reason = match compare_images(actual, expected) {
        None => format!(
            "image dimensions differ: actual {:?} vs expected {:?}",
            actual.dimensions(),
            expected.dimensions()
        ),
        Some(d) if d.max_diff > max_diff => {
            format!(
                "max channel diff {} exceeds {max_diff} (PSNR {:.2} dB)",
                d.max_diff, d.psnr
            )
        }
        Some(d) if d.psnr < min_psnr => {
            format!(
                "PSNR {:.2} dB below {min_psnr} dB (max diff {})",
                d.psnr, d.max_diff
            )
        }
        Some(_) => return Ok(()),
    };
    Err(ImageMismatch {
        reason,
        artifacts: write_artifacts(artifact_dir, actual, expected),
    })
}

fn artifacts_root() -> PathBuf {
    match std::env::var_os("CARGO_TARGET_DIR") {
        Some(t) => PathBuf::from(t).join("test-artifacts"),
        None => Path::new(env!("CARGO_MANIFEST_DIR")).join("../../target/test-artifacts"),
    }
}

fn current_test_name() -> String {
    let name = std::thread::current()
        .name()
        .unwrap_or("unnamed")
        .to_owned();
    name.chars()
        .map(|c| {
            if c.is_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect()
}

/// Asserts two images are close; on failure writes artifacts to
/// `target/test-artifacts/<test>/{actual,expected,diff}.png` and panics.
///
/// `max_diff` is the largest allowed per-channel difference, `min_psnr` the minimum PSNR in dB.
#[track_caller]
pub fn assert_images_close(actual: &RgbaImage, expected: &RgbaImage, max_diff: u8, min_psnr: f64) {
    let dir = artifacts_root().join(current_test_name());
    if let Err(m) = check_images_close(actual, expected, max_diff, min_psnr, &dir) {
        panic!("images differ: {m}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gradient() -> RgbaImage {
        RgbaImage::from_fn(16, 8, |x, y| {
            Rgba([(x * 16) as u8, (y * 32) as u8, 128, 255])
        })
    }

    #[test]
    fn image_compare_detects_diff() {
        let a = gradient();
        let same = compare_images(&a, &a.clone()).unwrap();
        assert_eq!(same.max_diff, 0);
        assert!(same.psnr.is_infinite());

        let mut b = a.clone();
        b.put_pixel(
            3,
            2,
            Rgba([a.get_pixel(3, 2).0[0].wrapping_add(40), 0, 0, 255]),
        );
        let d = compare_images(&a, &b).unwrap();
        assert!(d.max_diff >= 40);
        assert!(d.psnr.is_finite());

        let dir = tempfile::tempdir().unwrap();
        assert!(check_images_close(&a, &a, 0, 100.0, dir.path()).is_ok());
        assert!(
            !dir.path().join("diff.png").exists(),
            "no artifacts on success"
        );

        let err = check_images_close(&a, &b, 5, 0.0, dir.path()).unwrap_err();
        assert!(err.reason.contains("max channel diff"), "{err}");
        for f in ["actual.png", "expected.png", "diff.png"] {
            assert!(dir.path().join(f).is_file(), "{f} written");
        }
        let diff = image::open(dir.path().join("diff.png")).unwrap().to_rgba8();
        assert_eq!(diff.dimensions(), a.dimensions());
        assert_ne!(
            diff.get_pixel(3, 2).0[..3],
            [0, 0, 0],
            "diff highlights the changed pixel"
        );
        assert_eq!(diff.get_pixel(0, 0).0[..3], [0, 0, 0]);
    }

    #[test]
    fn image_compare_respects_tolerances() {
        let a = gradient();
        let mut b = a.clone();
        b.put_pixel(0, 0, Rgba([a.get_pixel(0, 0).0[0] + 3, 0, 128, 255]));
        let dir = tempfile::tempdir().unwrap();
        assert!(check_images_close(&a, &b, 128, 20.0, dir.path()).is_ok());
        let low_psnr = check_images_close(&a, &b, 255, 200.0, dir.path()).unwrap_err();
        assert!(low_psnr.reason.contains("PSNR"), "{low_psnr}");
    }

    #[test]
    fn image_compare_rejects_dimension_mismatch() {
        let a = gradient();
        let b = RgbaImage::new(4, 4);
        assert!(compare_images(&a, &b).is_none());
        let dir = tempfile::tempdir().unwrap();
        let err = check_images_close(&a, &b, 255, 0.0, dir.path()).unwrap_err();
        assert!(err.reason.contains("dimensions differ"), "{err}");
        assert!(dir.path().join("diff.png").is_file());
    }

    #[test]
    #[should_panic(expected = "images differ")]
    fn assert_images_close_panics_on_mismatch() {
        let a = gradient();
        let b = RgbaImage::new(16, 8);
        assert_images_close(&a, &b, 1, 50.0);
    }
}
