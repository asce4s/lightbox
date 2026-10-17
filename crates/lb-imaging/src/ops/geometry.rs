//! Rotate, crop and straighten (SPEC §6.6.3).

use image::{Rgba, Rgba32FImage};
use rayon::prelude::*;

/// Exact pixel rotation by `turns` quarter turns, clockwise positive. `turns` is taken
/// modulo 4; no resampling takes place.
pub fn rotate_quarter(img: &Rgba32FImage, turns: i8) -> Rgba32FImage {
    let (w, h) = img.dimensions();
    match turns.rem_euclid(4) {
        0 => img.clone(),
        1 => Rgba32FImage::from_fn(h, w, |x, y| *img.get_pixel(y, h - 1 - x)),
        2 => Rgba32FImage::from_fn(w, h, |x, y| *img.get_pixel(w - 1 - x, h - 1 - y)),
        _ => Rgba32FImage::from_fn(h, w, |x, y| *img.get_pixel(w - 1 - y, x)),
    }
}

/// Integer pixel rectangle `(x0, y0, x1, y1)` for a normalized crop of a `w × h` image:
/// `round(x·W)` etc., clamped, at least 1×1.
pub fn crop_rect(w: u32, h: u32, x: f32, y: f32, cw: f32, ch: f32) -> (u32, u32, u32, u32) {
    let px = |v: f32, size: u32| -> u32 {
        let scaled = (f64::from(v) * f64::from(size)).round();
        scaled.clamp(0.0, f64::from(size)) as u32
    };
    let x0 = px(x, w).min(w.saturating_sub(1));
    let y0 = px(y, h).min(h.saturating_sub(1));
    let x1 = px(x + cw, w).clamp(x0 + 1, w.max(1));
    let y1 = px(y + ch, h).clamp(y0 + 1, h.max(1));
    (x0, y0, x1, y1)
}

/// Crops to the normalized rectangle `(x, y, cw, ch)`.
pub fn crop(img: &Rgba32FImage, x: f32, y: f32, cw: f32, ch: f32) -> Rgba32FImage {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return img.clone();
    }
    let (x0, y0, x1, y1) = crop_rect(w, h, x, y, cw, ch);
    image::imageops::crop_imm(img, x0, y0, x1 - x0, y1 - y0).to_image()
}

/// Output size of a straighten by `degrees` of a `w × h` image: the largest same-aspect
/// rectangle that fits inside the rotated image, `round(s·W) × round(s·H)`.
pub fn straighten_size(w: u32, h: u32, degrees: f32) -> (u32, u32) {
    let theta = f64::from(degrees).abs().to_radians();
    let (c, sn) = (theta.cos(), theta.sin());
    let (wf, hf) = (f64::from(w), f64::from(h));
    let s = (wf / (wf * c + hf * sn))
        .min(hf / (wf * sn + hf * c))
        .min(1.0);
    (
        ((s * wf).round() as u32).clamp(1, w.max(1)),
        ((s * hf).round() as u32).clamp(1, h.max(1)),
    )
}

/// Position in the source image (pixel units, `0..W`, `0..H`) that lands at output pixel
/// centre `(ox + 0.5, oy + 0.5)` when the content is rotated clockwise by `degrees` about the
/// image centre and the result is centre-cropped to `out_w × out_h`.
pub fn straighten_source_point(
    (w, h): (u32, u32),
    (out_w, out_h): (u32, u32),
    degrees: f32,
    (ox, oy): (f64, f64),
) -> (f64, f64) {
    let theta = f64::from(degrees).to_radians();
    let (c, sn) = (theta.cos(), theta.sin());
    let dx = ox + 0.5 - f64::from(out_w) / 2.0;
    let dy = oy + 0.5 - f64::from(out_h) / 2.0;
    (
        f64::from(w) / 2.0 + c * dx + sn * dy,
        f64::from(h) / 2.0 - sn * dx + c * dy,
    )
}

pub(crate) fn sample_bilinear(img: &Rgba32FImage, x: f64, y: f64) -> Rgba<f32> {
    let (w, h) = img.dimensions();
    let (fx, fy) = (x - 0.5, y - 0.5);
    let (x0f, y0f) = (fx.floor(), fy.floor());
    let (tx, ty) = ((fx - x0f) as f32, (fy - y0f) as f32);
    let clamp = |v: f64, max: u32| -> u32 { v.clamp(0.0, f64::from(max - 1)) as u32 };
    let (xa, xb) = (clamp(x0f, w), clamp(x0f + 1.0, w));
    let (ya, yb) = (clamp(y0f, h), clamp(y0f + 1.0, h));
    let (p00, p10) = (img.get_pixel(xa, ya).0, img.get_pixel(xb, ya).0);
    let (p01, p11) = (img.get_pixel(xa, yb).0, img.get_pixel(xb, yb).0);
    let mut out = [0.0_f32; 4];
    for (i, o) in out.iter_mut().enumerate() {
        let top = p00[i] + (p10[i] - p00[i]) * tx;
        let bottom = p01[i] + (p11[i] - p01[i]) * tx;
        *o = top + (bottom - top) * ty;
    }
    Rgba(out)
}

/// Rotates content about its centre by `degrees` (positive is clockwise) with bilinear
/// sampling, then centre-crops so no empty corner remains (SPEC §6.6.3).
pub fn straighten(img: &Rgba32FImage, degrees: f32) -> Rgba32FImage {
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 || degrees.abs() < 1e-4 {
        return img.clone();
    }
    let (out_w, out_h) = straighten_size(w, h, degrees);
    let mut out = vec![0.0_f32; out_w as usize * out_h as usize * 4];
    out.par_chunks_mut(out_w as usize * 4)
        .enumerate()
        .for_each(|(oy, row)| {
            for (ox, px) in row.chunks_exact_mut(4).enumerate() {
                let (sx, sy) = straighten_source_point(
                    (w, h),
                    (out_w, out_h),
                    degrees,
                    (ox as f64, oy as f64),
                );
                px.copy_from_slice(&sample_bilinear(img, sx, sy).0);
            }
        });
    Rgba32FImage::from_raw(out_w, out_h, out).unwrap_or_else(|| img.clone())
}
