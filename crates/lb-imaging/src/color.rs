//! Colour helpers for the working colour model (SPEC §6.6.2).

use image::Rgba32FImage;
use rayon::prelude::*;

/// Threshold below which a luma is treated as black ("eps" in SPEC §6.6.3).
pub const EPS: f32 = 1e-6;

/// Clamps to `[0, 1]`, mapping NaN to 0.
#[inline]
pub fn clamp01(v: f32) -> f32 {
    if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) }
}

/// Exact sRGB electro-optical transfer: encoded value to linear light.
#[inline]
pub fn to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

/// Exact sRGB opto-electronic transfer: linear light to encoded value.
#[inline]
pub fn to_srgb(v: f32) -> f32 {
    if v <= 0.003_130_8 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// `0.2126 r + 0.7152 g + 0.0722 b` on sRGB-encoded values.
#[inline]
pub fn luma(r: f32, g: f32, b: f32) -> f32 {
    0.2126 * r + 0.7152 * g + 0.0722 * b
}

/// Cubic Hermite interpolation between `e0` and `e1`, clamped. A degenerate range acts as a
/// step at `e1`.
#[inline]
pub fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    if e1 <= e0 {
        return if x >= e1 { 1.0 } else { 0.0 };
    }
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Linear interpolation `a·(1−t) + b·t`.
#[inline]
pub fn mix(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

/// "Scale by luma" (SPEC §6.6.3): moves a pixel to luma `target_l`, scaling its channels when
/// it has any luma and otherwise making it grey. The result is clamped to `[0, 1]`.
#[inline]
pub fn scale_by_luma(rgb: [f32; 3], target_l: f32) -> [f32; 3] {
    let l = luma(rgb[0], rgb[1], rgb[2]);
    if l > EPS {
        let k = target_l / l;
        [
            clamp01(rgb[0] * k),
            clamp01(rgb[1] * k),
            clamp01(rgb[2] * k),
        ]
    } else {
        let g = clamp01(target_l);
        [g, g, g]
    }
}

/// Applies `f` to every pixel in parallel over rows. Each pixel is independent, so the result
/// does not depend on thread scheduling. Output is clamped to `[0, 1]` (alpha untouched by
/// the clamp only if `f` leaves it in range).
pub fn map_pixels(img: &mut Rgba32FImage, f: impl Fn(&mut [f32; 4]) + Sync) {
    let width = img.width() as usize;
    if width == 0 {
        return;
    }
    img.par_chunks_mut(width * 4).for_each(|row| {
        for px in row.chunks_exact_mut(4) {
            let mut p = [px[0], px[1], px[2], px[3]];
            f(&mut p);
            px[0] = clamp01(p[0]);
            px[1] = clamp01(p[1]);
            px[2] = clamp01(p[2]);
            px[3] = clamp01(p[3]);
        }
    });
}

/// Like [`map_pixels`] but also passes the pixel coordinates.
pub fn map_pixels_xy(img: &mut Rgba32FImage, f: impl Fn(u32, u32, &mut [f32; 4]) + Sync) {
    let width = img.width() as usize;
    if width == 0 {
        return;
    }
    img.par_chunks_mut(width * 4)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, px) in row.chunks_exact_mut(4).enumerate() {
                let mut p = [px[0], px[1], px[2], px[3]];
                f(x as u32, y as u32, &mut p);
                px[0] = clamp01(p[0]);
                px[1] = clamp01(p[1]);
                px[2] = clamp01(p[2]);
                px[3] = clamp01(p[3]);
            }
        });
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn srgb_roundtrip(v in 0.0_f32..=1.0) {
            let back = to_srgb(to_linear(v));
            prop_assert!((back - v).abs() < 1e-5, "{v} -> {back}");
            let lin = to_linear(to_srgb(v));
            prop_assert!((lin - v).abs() < 1e-5, "{v} -> {lin}");
        }

        #[test]
        fn smoothstep_is_monotonic_and_bounded(
            e0 in 0.0_f32..1.0, width in 0.01_f32..1.0, a in -1.0_f32..2.0, b in -1.0_f32..2.0,
        ) {
            let e1 = e0 + width;
            let (lo, hi) = if a <= b { (a, b) } else { (b, a) };
            let (s_lo, s_hi) = (smoothstep(e0, e1, lo), smoothstep(e0, e1, hi));
            prop_assert!((0.0..=1.0).contains(&s_lo) && (0.0..=1.0).contains(&s_hi));
            prop_assert!(s_lo <= s_hi);
        }
    }

    #[test]
    fn srgb_known_values() {
        assert_eq!(to_linear(0.0), 0.0);
        assert!((to_linear(1.0) - 1.0).abs() < 1e-6);
        assert!((to_linear(0.5) - 0.214_041_14).abs() < 1e-6);
        assert!((to_srgb(0.214_041_14) - 0.5).abs() < 1e-6);
        assert!(
            (to_linear(0.04045) - 0.003_130_8).abs() < 1e-6,
            "continuous at the knee"
        );
        assert!((to_srgb(0.003_130_8) - 0.04045).abs() < 1e-5);
    }

    #[test]
    fn luma_weights() {
        assert!((luma(1.0, 1.0, 1.0) - 1.0).abs() < 1e-6);
        assert!((luma(1.0, 0.0, 0.0) - 0.2126).abs() < 1e-7);
        assert!((luma(0.0, 1.0, 0.0) - 0.7152).abs() < 1e-7);
        assert!((luma(0.0, 0.0, 1.0) - 0.0722).abs() < 1e-7);
    }

    #[test]
    fn smoothstep_endpoints_and_midpoint() {
        assert_eq!(smoothstep(0.2, 0.6, 0.0), 0.0);
        assert_eq!(smoothstep(0.2, 0.6, 0.2), 0.0);
        assert!((smoothstep(0.2, 0.6, 0.4) - 0.5).abs() < 1e-6);
        assert_eq!(smoothstep(0.2, 0.6, 0.6), 1.0);
        assert_eq!(smoothstep(0.2, 0.6, 9.0), 1.0);
        assert_eq!(smoothstep(0.5, 0.5, 0.4), 0.0);
        assert_eq!(smoothstep(0.5, 0.5, 0.5), 1.0);
    }

    #[test]
    fn scale_by_luma_cases() {
        let out = scale_by_luma([0.2, 0.4, 0.1], 0.5);
        let l = luma(out[0], out[1], out[2]);
        assert!((l - 0.5).abs() < 1e-5);
        assert!((out[0] / out[1] - 0.5).abs() < 1e-5, "channel ratios kept");
        assert_eq!(scale_by_luma([0.0, 0.0, 0.0], 0.3), [0.3, 0.3, 0.3]);
        assert_eq!(
            scale_by_luma([1.0, 1.0, 1.0], 3.0),
            [1.0, 1.0, 1.0],
            "clamped"
        );
    }

    #[test]
    fn map_pixels_clamps_and_visits_every_pixel() {
        let mut img = Rgba32FImage::from_pixel(5, 3, image::Rgba([0.5, 0.5, 0.5, 1.0]));
        map_pixels(&mut img, |p| {
            p[0] = 2.0;
            p[1] = -1.0;
            p[2] = f32::NAN;
        });
        for px in img.pixels() {
            assert_eq!(px.0, [1.0, 0.0, 0.0, 1.0]);
        }
        let mut img = Rgba32FImage::new(4, 2);
        map_pixels_xy(&mut img, |x, y, p| p[0] = (x + y * 4) as f32 / 8.0);
        assert_eq!(img.get_pixel(3, 1).0[0], 7.0 / 8.0);
    }
}
