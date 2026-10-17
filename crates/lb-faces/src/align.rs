//! Face alignment: warps a face onto the canonical 112x112 eye/nose/mouth layout.

use crate::detect::Detection;
use image::{Rgb, RgbImage};

/// Side length of an aligned face crop.
pub const ALIGNED_EDGE: u32 = 112;

/// Canonical landmark positions in the aligned crop (the ArcFace/SFace template).
const TEMPLATE: [(f32, f32); 5] = [
    (38.2946, 51.6963),
    (73.5318, 51.5014),
    (56.0252, 71.7366),
    (41.5493, 92.3655),
    (70.7299, 92.2041),
];

/// A similarity transform `d = c * s + t` over complex numbers `(re, im)`.
struct Similarity {
    c: (f32, f32),
    t: (f32, f32),
}

fn fit(src: &[(f32, f32); 5], dst: &[(f32, f32); 5]) -> Option<Similarity> {
    let mean = |p: &[(f32, f32); 5]| {
        let n = p.len() as f32;
        (
            p.iter().map(|q| q.0).sum::<f32>() / n,
            p.iter().map(|q| q.1).sum::<f32>() / n,
        )
    };
    let (ms, md) = (mean(src), mean(dst));
    let (mut a, mut b, mut norm) = (0.0, 0.0, 0.0);
    for (s, d) in src.iter().zip(dst) {
        let (sx, sy) = (s.0 - ms.0, s.1 - ms.1);
        let (dx, dy) = (d.0 - md.0, d.1 - md.1);
        a += sx * dx + sy * dy;
        b += sx * dy - sy * dx;
        norm += sx * sx + sy * sy;
    }
    if norm <= f32::EPSILON {
        return None;
    }
    let c = (a / norm, b / norm);
    let t = (
        md.0 - (c.0 * ms.0 - c.1 * ms.1),
        md.1 - (c.0 * ms.1 + c.1 * ms.0),
    );
    Some(Similarity { c, t })
}

fn sample(img: &RgbImage, x: f32, y: f32) -> Rgb<u8> {
    let (w, h) = (img.width() as i64, img.height() as i64);
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let at = |ix: i64, iy: i64| img.get_pixel(ix.clamp(0, w - 1) as u32, iy.clamp(0, h - 1) as u32);
    let (x0, y0) = (x0 as i64, y0 as i64);
    let mut out = [0u8; 3];
    for (ch, slot) in out.iter_mut().enumerate() {
        let top = f32::from(at(x0, y0)[ch]) * (1.0 - fx) + f32::from(at(x0 + 1, y0)[ch]) * fx;
        let bottom =
            f32::from(at(x0, y0 + 1)[ch]) * (1.0 - fx) + f32::from(at(x0 + 1, y0 + 1)[ch]) * fx;
        *slot = (top * (1.0 - fy) + bottom * fy).round().clamp(0.0, 255.0) as u8;
    }
    Rgb(out)
}

/// The face of `det` as a 112x112 crop. With landmarks the face is rotated and scaled so that
/// eyes, nose and mouth land on the canonical template; otherwise the box is cropped and
/// resized.
pub fn align_face(img: &RgbImage, det: &Detection) -> RgbImage {
    let (w, h) = (img.width() as f32, img.height() as f32);
    let mut out = RgbImage::new(ALIGNED_EDGE, ALIGNED_EDGE);
    let transform = det.landmarks.and_then(|lm| {
        let px = lm.map(|(x, y)| (x * w, y * h));
        fit(&px, &TEMPLATE)
    });
    match transform {
        Some(Similarity { c, t }) => {
            let denom = c.0 * c.0 + c.1 * c.1;
            for v in 0..ALIGNED_EDGE {
                for u in 0..ALIGNED_EDGE {
                    let (dx, dy) = (u as f32 - t.0, v as f32 - t.1);
                    let sx = (dx * c.0 + dy * c.1) / denom;
                    let sy = (dy * c.0 - dx * c.1) / denom;
                    out.put_pixel(u, v, sample(img, sx, sy));
                }
            }
        }
        None => {
            let r = det.rect;
            let edge = ALIGNED_EDGE as f32;
            for v in 0..ALIGNED_EDGE {
                for u in 0..ALIGNED_EDGE {
                    let sx = (r.x + r.w * (u as f32 + 0.5) / edge) * w - 0.5;
                    let sy = (r.y + r.h * (v as f32 + 0.5) / edge) * h - 0.5;
                    out.put_pixel(u, v, sample(img, sx, sy));
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lb_core::NormRect;

    fn detection(landmarks: Option<[(f32, f32); 5]>) -> Detection {
        Detection {
            rect: NormRect {
                x: 0.25,
                y: 0.25,
                w: 0.5,
                h: 0.5,
            },
            score: 0.9,
            landmarks,
        }
    }

    #[test]
    fn template_landmarks_map_to_themselves() {
        let mut img = RgbImage::from_pixel(112, 112, Rgb([10, 10, 10]));
        for (x, y) in TEMPLATE {
            img.put_pixel(x as u32, y as u32, Rgb([250, 250, 250]));
        }
        let norm = TEMPLATE.map(|(x, y)| (x / 112.0, y / 112.0));
        let out = align_face(&img, &detection(Some(norm)));
        assert_eq!(out.dimensions(), (ALIGNED_EDGE, ALIGNED_EDGE));
        for (x, y) in TEMPLATE {
            assert!(
                out.get_pixel(x as u32, y as u32)[0] > 200,
                "marker at ({x}, {y}) kept"
            );
        }
        assert!(out.get_pixel(5, 5)[0] < 20);
    }

    #[test]
    fn scaled_and_shifted_faces_are_normalized() {
        let mut img = RgbImage::from_pixel(448, 448, Rgb([0, 0, 0]));
        let pts: Vec<(f32, f32)> = TEMPLATE
            .iter()
            .map(|(x, y)| (x * 2.0 + 100.0, y * 2.0 + 60.0))
            .collect();
        for (x, y) in &pts {
            for dy in -2i32..=2 {
                for dx in -2i32..=2 {
                    img.put_pixel(
                        (*x as i32 + dx) as u32,
                        (*y as i32 + dy) as u32,
                        Rgb([255, 255, 255]),
                    );
                }
            }
        }
        let norm: [(f32, f32); 5] = std::array::from_fn(|i| (pts[i].0 / 448.0, pts[i].1 / 448.0));
        let out = align_face(&img, &detection(Some(norm)));
        for (x, y) in TEMPLATE {
            assert!(out.get_pixel(x as u32, y as u32)[0] > 150, "({x}, {y})");
        }
    }

    #[test]
    fn without_landmarks_the_box_is_cropped() {
        let mut img = RgbImage::from_pixel(100, 100, Rgb([0, 0, 0]));
        for y in 25..75 {
            for x in 25..75 {
                img.put_pixel(x, y, Rgb([200, 100, 50]));
            }
        }
        let out = align_face(&img, &detection(None));
        assert_eq!(out.get_pixel(56, 56), &Rgb([200, 100, 50]));
        assert_eq!(out.get_pixel(1, 1), &Rgb([200, 100, 50]));
    }

    #[test]
    fn degenerate_landmarks_fall_back_to_the_box() {
        let img = RgbImage::from_pixel(50, 50, Rgb([9, 9, 9]));
        let out = align_face(&img, &detection(Some([(0.5, 0.5); 5])));
        assert_eq!(out.get_pixel(3, 3), &Rgb([9, 9, 9]));
    }
}
