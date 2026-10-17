//! AutoContrast, AutoColor and FeelingLucky (SPEC §6.6.3).

use super::hist::Histogram;
use crate::color::{EPS, clamp01, luma, map_pixels};
use image::Rgba32FImage;

/// Percentile at the dark end.
const LO_PERCENT: f64 = 0.5;
/// Percentile at the bright end.
const HI_PERCENT: f64 = 99.5;
/// Ranges narrower than this are left alone.
const MIN_RANGE: f32 = 0.02;

/// Stretches luma so the 0.5th–99.5th percentile range fills `[0, 1]`.
pub fn auto_contrast(img: &mut Rgba32FImage) {
    let mut hist = Histogram::new();
    for px in img.pixels() {
        hist.add(luma(px.0[0], px.0[1], px.0[2]));
    }
    let (lo, hi) = (
        hist.low_percentile(LO_PERCENT),
        hist.high_percentile(HI_PERCENT),
    );
    if hi - lo < MIN_RANGE {
        return;
    }
    let scale = 1.0 / (hi - lo);
    map_pixels(img, |p| {
        for c in &mut p[..3] {
            *c = (*c - lo) * scale;
        }
    });
}

/// Stretches each channel separately, then balances the mid-tone channel means.
pub fn auto_color(img: &mut Rgba32FImage) {
    let mut hists = [Histogram::new(), Histogram::new(), Histogram::new()];
    for px in img.pixels() {
        for (h, v) in hists.iter_mut().zip(&px.0[..3]) {
            h.add(*v);
        }
    }
    let ranges: [Option<(f32, f32)>; 3] = std::array::from_fn(|c| {
        let (lo, hi) = (
            hists[c].low_percentile(LO_PERCENT),
            hists[c].high_percentile(HI_PERCENT),
        );
        (hi - lo >= MIN_RANGE).then_some((lo, hi))
    });
    let stretch = |c: usize, v: f32| match ranges[c] {
        Some((lo, hi)) => clamp01((v - lo) / (hi - lo)),
        None => v,
    };

    let mut sums = [0.0_f64; 3];
    let mut count = 0_u64;
    for px in img.pixels() {
        let s = [
            stretch(0, px.0[0]),
            stretch(1, px.0[1]),
            stretch(2, px.0[2]),
        ];
        let l = luma(s[0], s[1], s[2]);
        if l > 0.25 && l < 0.75 {
            for (sum, v) in sums.iter_mut().zip(s) {
                *sum += f64::from(v);
            }
            count += 1;
        }
    }
    let mut gains = [1.0_f32; 3];
    if count > 0 {
        let means: [f32; 3] = std::array::from_fn(|c| (sums[c] / count as f64) as f32);
        let m = (means[0] + means[1] + means[2]) / 3.0;
        for c in 0..3 {
            if ranges[c].is_some() && means[c] > EPS {
                gains[c] = m / means[c];
            }
        }
    }
    map_pixels(img, |p| {
        for c in 0..3 {
            p[c] = stretch(c, p[c]) * gains[c];
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn ramp(lo: f32, hi: f32) -> Rgba32FImage {
        Rgba32FImage::from_fn(256, 8, |x, _| {
            let v = lo + (hi - lo) * x as f32 / 255.0;
            Rgba([v, v, v, 1.0])
        })
    }

    #[test]
    fn auto_contrast_stretches_a_grey_ramp_to_full_range() {
        let mut img = ramp(0.3, 0.6);
        auto_contrast(&mut img);
        assert!(img.get_pixel(0, 0).0[0] < 0.02);
        assert!(img.get_pixel(255, 0).0[0] > 0.98);
        let mid = img.get_pixel(128, 0).0[0];
        assert!(
            (mid - 0.5).abs() < 0.03,
            "midpoint stays in the middle: {mid}"
        );
        let p = img.get_pixel(64, 0).0;
        assert_eq!(p[0], p[1]);
        assert_eq!(p[3], 1.0, "alpha untouched");
    }

    #[test]
    fn auto_contrast_noop_on_flat_image() {
        for v in [0.0_f32, 0.2, 0.5, 1.0] {
            let flat = Rgba32FImage::from_pixel(32, 32, Rgba([v, v, v, 1.0]));
            let mut out = flat.clone();
            auto_contrast(&mut out);
            assert_eq!(out, flat, "flat {v}");
        }
        let mut nearly = ramp(0.5, 0.51);
        let before = nearly.clone();
        auto_contrast(&mut nearly);
        assert_eq!(nearly, before, "range below 0.02 is left alone");
    }

    #[test]
    fn auto_color_removes_a_colour_cast() {
        // Neutral ramp with a strong red cast.
        let mut img = Rgba32FImage::from_fn(256, 8, |x, _| {
            let v = 0.15 + 0.6 * x as f32 / 255.0;
            Rgba([(v + 0.2).min(1.0), v, v * 0.9, 1.0])
        });
        auto_color(&mut img);
        let mut sums = [0.0_f64; 3];
        let mut n = 0;
        for p in img.pixels() {
            let l = luma(p.0[0], p.0[1], p.0[2]);
            if l > 0.25 && l < 0.75 {
                for (sum, v) in sums.iter_mut().zip(&p.0[..3]) {
                    *sum += f64::from(*v);
                }
                n += 1;
            }
        }
        assert!(n > 0);
        let means: Vec<f64> = sums.iter().map(|s| s / f64::from(n)).collect();
        let spread = means.iter().cloned().fold(f64::MIN, f64::max)
            - means.iter().cloned().fold(f64::MAX, f64::min);
        assert!(
            spread < 0.08,
            "mid-tone channel means {means:?} should be close"
        );
    }

    #[test]
    fn auto_color_skips_flat_channels() {
        let mut img = Rgba32FImage::from_fn(64, 4, |x, _| {
            let v = 0.1 + 0.8 * x as f32 / 63.0;
            Rgba([v, 0.4, v, 1.0])
        });
        auto_color(&mut img);
        assert!(
            img.pixels().all(|p| (p.0[1] - 0.4).abs() < 1e-6),
            "flat channel untouched"
        );
    }
}
