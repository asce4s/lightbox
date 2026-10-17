//! Test support: fixtures, fakes, invariants and image comparison. Dev-dependency only.
#![warn(missing_docs)]
#![allow(clippy::unwrap_used, clippy::expect_used)]

pub mod fixtures;
mod goldens;
mod home;
mod image_cmp;
mod wait;

pub use goldens::{MAX_DIFF, MIN_PSNR, UPDATE_ENV, assert_golden, golden_path};
pub use home::{LIGHTBOX_HOME_ENV, TestHome};
pub use image_cmp::{
    ImageDiff, ImageMismatch, assert_images_close, check_images_close, compare_images,
};
pub use wait::wait_until;
