//! Shared fixtures for the imaging tests. Images are generated in memory and losslessly so
//! goldens do not depend on a JPEG decoder.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use image::{DynamicImage, Rgba, RgbaImage};
use lb_imaging::{Rgba32FImage, convert};
use lb_testkit::fixtures::{manifest::Pattern, pattern};

/// `gradient_rgb_512.png`: red ramps left to right, green top to bottom, blue is the inverse
/// of red.
pub fn gradient_rgb_512() -> Rgba32FImage {
    let (w, h) = (512_u32, 512_u32);
    let img = RgbaImage::from_fn(w, h, |x, y| {
        let r = (f64::from(x) / f64::from(w - 1) * 255.0).round() as u8;
        let g = (f64::from(y) / f64::from(h - 1) * 255.0).round() as u8;
        Rgba([r, g, 255 - r, 255])
    });
    convert::to_working(&DynamicImage::ImageRgba8(img))
}

/// `portrait_patches_640x480`: a 6×4 grid of flat colour patches.
pub fn portrait_patches_640x480() -> Rgba32FImage {
    let img = pattern::render(Pattern::Patches, 31, 640, 480);
    convert::to_working(&DynamicImage::ImageRgba8(img))
}

/// `low_contrast_400.png`: a scene squeezed into the 0.35..0.60 range.
pub fn low_contrast_400() -> Rgba32FImage {
    let mut img = pattern::render(Pattern::Scene, 17, 400, 400);
    for p in img.pixels_mut() {
        for c in &mut p.0[..3] {
            *c = (89.0 + f32::from(*c) * 0.2).round() as u8;
        }
    }
    convert::to_working(&DynamicImage::ImageRgba8(img))
}

/// A small random-looking opaque image for property tests.
pub fn noise(w: u32, h: u32, seed: u64) -> Rgba32FImage {
    let img = pattern::render(Pattern::Scene, seed, w, h);
    convert::to_working(&DynamicImage::ImageRgba8(img))
}

/// `red_eye_320x240`: skin-coloured background with two eyes whose irises are red.
pub fn red_eye_320x240() -> Rgba32FImage {
    let (w, h) = (320_u32, 240_u32);
    let eyes = [(100.0_f32, 110.0_f32), (220.0, 110.0)];
    let img = RgbaImage::from_fn(w, h, |x, y| {
        let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
        let mut rgb = [
            (214.0 + 20.0 * fy / h as f32) as u8,
            (166.0 + 10.0 * fx / w as f32) as u8,
            140,
        ];
        for (cx, cy) in eyes {
            let (dx, dy) = (fx - cx, fy - cy);
            let d2 = dx * dx + dy * dy;
            if (dx / 36.0).powi(2) + (dy / 22.0).powi(2) <= 1.0 {
                rgb = [245, 245, 240];
            }
            if d2 <= 18.0 * 18.0 {
                rgb = [215, 30, 28];
            }
            if d2 <= 8.0 * 8.0 {
                rgb = [30, 6, 6];
            }
        }
        Rgba([rgb[0], rgb[1], rgb[2], 255])
    });
    convert::to_working(&DynamicImage::ImageRgba8(img))
}

/// Top-left 160×120 crop of a fixture, used where noisy output would bloat golden files.
pub fn crop_160x120(img: &Rgba32FImage) -> Rgba32FImage {
    image::imageops::crop_imm(img, 0, 0, 160, 120).to_image()
}
