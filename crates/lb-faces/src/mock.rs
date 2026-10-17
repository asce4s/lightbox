//! Deterministic stand-ins for the neural networks, used by tests and the fixture generator.
//!
//! A "face" is a filled square in one of [`MOCK_PALETTE`]'s colors drawn with
//! [`draw_mock_face`]. [`MockDetector`] finds the squares; [`MockEmbedder`] gives every palette
//! color its own pseudo-random unit vector, so two squares of the same color are "the same
//! person" and different colors are not.

use crate::detect::{Detection, EMBEDDING_DIM, FaceDetector, FaceEmbedder};
use crate::error::Result;
use image::{Rgb, RgbImage};
use lb_core::NormRect;

/// Colors of mock faces. Far apart so that resampling and JPEG noise cannot confuse them.
pub const MOCK_PALETTE: [[u8; 3]; 8] = [
    [220, 40, 40],
    [40, 200, 60],
    [50, 70, 220],
    [230, 210, 40],
    [200, 50, 200],
    [40, 210, 210],
    [240, 130, 30],
    [120, 50, 170],
];

const TOLERANCE: i32 = 40;

/// Draws a mock face of `color` (an index into [`MOCK_PALETTE`], wrapping) as a square with its
/// top-left corner at `(x, y)`.
pub fn draw_mock_face(img: &mut RgbImage, color: usize, x: u32, y: u32, size: u32) {
    let rgb = Rgb(MOCK_PALETTE[color % MOCK_PALETTE.len()]);
    for yy in y..(y + size).min(img.height()) {
        for xx in x..(x + size).min(img.width()) {
            img.put_pixel(xx, yy, rgb);
        }
    }
}

fn matches(px: &Rgb<u8>, color: &[u8; 3]) -> bool {
    px.0.iter()
        .zip(color)
        .all(|(a, b)| (i32::from(*a) - i32::from(*b)).abs() <= TOLERANCE)
}

fn palette_index(px: &Rgb<u8>) -> Option<usize> {
    MOCK_PALETTE.iter().position(|c| matches(px, c))
}

/// Finds one square per palette color present in the image.
#[derive(Debug, Clone, Copy, Default)]
pub struct MockDetector;

impl FaceDetector for MockDetector {
    fn detect(&self, img: &RgbImage) -> Result<Vec<Detection>> {
        let (w, h) = (img.width(), img.height());
        let mut out = Vec::new();
        for color in &MOCK_PALETTE {
            let (mut x0, mut y0, mut x1, mut y1) = (u32::MAX, u32::MAX, 0, 0);
            let mut count = 0u64;
            for (x, y, px) in img.enumerate_pixels() {
                if matches(px, color) {
                    x0 = x0.min(x);
                    y0 = y0.min(y);
                    x1 = x1.max(x);
                    y1 = y1.max(y);
                    count += 1;
                }
            }
            if count == 0 {
                continue;
            }
            let area = u64::from(x1 - x0 + 1) * u64::from(y1 - y0 + 1);
            if count * 2 < area {
                continue;
            }
            out.push(Detection {
                rect: NormRect {
                    x: x0 as f32 / w as f32,
                    y: y0 as f32 / h as f32,
                    w: (x1 - x0 + 1) as f32 / w as f32,
                    h: (y1 - y0 + 1) as f32 / h as f32,
                },
                score: 0.95,
                landmarks: None,
            });
        }
        Ok(out)
    }
}

fn prng_vector(seed: u64) -> [f32; EMBEDDING_DIM] {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15).wrapping_add(1);
    let mut next = || {
        state = state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    };
    let mut v = [0.0f32; EMBEDDING_DIM];
    for slot in &mut v {
        *slot = (next() >> 40) as f32 / (1u64 << 24) as f32 - 0.5;
    }
    v
}

/// Embeds a mock face by the palette color at the center of its box.
#[derive(Debug, Clone, Copy, Default)]
pub struct MockEmbedder;

impl FaceEmbedder for MockEmbedder {
    fn embed(&self, img: &RgbImage, det: &Detection) -> Result<[f32; EMBEDDING_DIM]> {
        let cx = ((det.rect.x + det.rect.w / 2.0) * img.width() as f32) as u32;
        let cy = ((det.rect.y + det.rect.h / 2.0) * img.height() as f32) as u32;
        let px = img.get_pixel(cx.min(img.width() - 1), cy.min(img.height() - 1));
        let identity = palette_index(px).map_or(u64::MAX, |i| i as u64);
        let mut v = prng_vector(identity);
        let jitter = prng_vector(u64::from(cx) * 7919 + u64::from(cy));
        for (a, b) in v.iter_mut().zip(jitter) {
            *a += b * 0.05;
        }
        Ok(v)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cluster::{cosine, l2_normalize};

    fn canvas() -> RgbImage {
        RgbImage::from_pixel(200, 120, Rgb([128, 128, 128]))
    }

    #[test]
    fn detector_finds_drawn_squares() {
        let mut img = canvas();
        draw_mock_face(&mut img, 0, 20, 30, 40);
        draw_mock_face(&mut img, 2, 120, 10, 50);
        let found = MockDetector.detect(&img).unwrap();
        assert_eq!(found.len(), 2);
        assert!((found[0].rect.x - 0.1).abs() < 1e-6);
        assert!((found[0].rect.w - 0.2).abs() < 1e-6);
        assert!(MockDetector.detect(&canvas()).unwrap().is_empty());
    }

    #[test]
    fn same_color_is_the_same_person() {
        let mut a = canvas();
        draw_mock_face(&mut a, 3, 10, 10, 40);
        let mut b = canvas();
        draw_mock_face(&mut b, 3, 100, 50, 60);
        let mut c = canvas();
        draw_mock_face(&mut c, 4, 10, 10, 40);
        let emb = |img: &RgbImage| {
            let det = MockDetector.detect(img).unwrap()[0];
            let mut v = MockEmbedder.embed(img, &det).unwrap().to_vec();
            assert!(l2_normalize(&mut v));
            v
        };
        let (ea, eb, ec) = (emb(&a), emb(&b), emb(&c));
        assert!(cosine(&ea, &eb) > 0.95, "{}", cosine(&ea, &eb));
        assert!(cosine(&ea, &ec) < 0.4, "{}", cosine(&ea, &ec));
    }
}
