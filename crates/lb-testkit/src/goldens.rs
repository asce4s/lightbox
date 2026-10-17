//! Golden-image comparison (SPEC §12.4).
//!
//! Goldens live in `tests/goldens/<suite>/<name>.png`. Setting `LB_UPDATE_GOLDENS` to `all`, a
//! suite name or a golden name (comma separated) rewrites those goldens instead of comparing.
//! Use `cargo xtask goldens --update <suite|name>` and read AGENTS.md §4 first.

use crate::{assert_images_close, fixtures::workspace_root};
use image::RgbaImage;
use std::path::PathBuf;

/// Environment variable that selects goldens to regenerate.
pub const UPDATE_ENV: &str = "LB_UPDATE_GOLDENS";
/// Largest allowed per-channel difference (SPEC §6.6.4).
pub const MAX_DIFF: u8 = 2;
/// Smallest allowed PSNR in dB (SPEC §6.6.4).
pub const MIN_PSNR: f64 = 45.0;

/// Where a golden lives.
pub fn golden_path(suite: &str, name: &str) -> PathBuf {
    workspace_root()
        .join("tests/goldens")
        .join(suite)
        .join(format!("{name}.png"))
}

fn update_requested(suite: &str, name: &str) -> bool {
    std::env::var(UPDATE_ENV).is_ok_and(|v| {
        v.split(',')
            .map(str::trim)
            .any(|t| t == "all" || t == suite || t == name)
    })
}

/// Compares `actual` with the golden `suite/name`, panicking with artifacts on mismatch. When
/// the golden is selected by [`UPDATE_ENV`] it is rewritten instead.
#[track_caller]
pub fn assert_golden(suite: &str, name: &str, actual: &RgbaImage) {
    let path = golden_path(suite, name);
    if update_requested(suite, name) {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).unwrap();
        }
        actual.save(&path).unwrap();
        eprintln!("updated golden {}", path.display());
        return;
    }
    let expected = image::open(&path)
        .unwrap_or_else(|e| {
            panic!(
                "missing golden {} ({e}); create it with `cargo xtask goldens --update {name}`",
                path.display()
            )
        })
        .to_rgba8();
    assert_images_close(actual, &expected, MAX_DIFF, MIN_PSNR);
}
