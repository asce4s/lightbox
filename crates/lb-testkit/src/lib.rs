//! Test support: fixtures, fakes, invariants and image comparison. Dev-dependency only.
#![allow(clippy::unwrap_used, clippy::expect_used)]

pub mod fixtures;
mod home;
mod image_cmp;
mod wait;

pub use home::{LIGHTBOX_HOME_ENV, TestHome};
pub use image_cmp::{
    ImageDiff, ImageMismatch, assert_images_close, check_images_close, compare_images,
};
pub use wait::wait_until;
