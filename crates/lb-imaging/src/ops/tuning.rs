//! FillLight and Tuning (SPEC §6.6.3).

use crate::color::{EPS, clamp01, luma, map_pixels, scale_by_luma};
use image::Rgba32FImage;

/// Brightens shadows: scale by luma with `L' = L + 2a·L·(1−L)²`.
pub fn fill_light(img: &mut Rgba32FImage, amount: f32) {
    if amount == 0.0 {
        return;
    }
    map_pixels(img, |p| {
        let l = luma(p[0], p[1], p[2]);
        let target = l + 2.0 * amount * l * (1.0 - l) * (1.0 - l);
        [p[0], p[1], p[2]] = scale_by_luma([p[0], p[1], p[2]], target);
    });
}

/// Highlight recovery: `L' = L + 2h·L²·(1−L)`.
pub fn highlights(img: &mut Rgba32FImage, amount: f32) {
    if amount == 0.0 {
        return;
    }
    map_pixels(img, |p| {
        let l = luma(p[0], p[1], p[2]);
        let target = l + 2.0 * amount * l * l * (1.0 - l);
        [p[0], p[1], p[2]] = scale_by_luma([p[0], p[1], p[2]], target);
    });
}

/// Shadow deepening: `L' = max(0, L − 2s·L·(1−L)²)`.
pub fn shadows(img: &mut Rgba32FImage, amount: f32) {
    if amount == 0.0 {
        return;
    }
    map_pixels(img, |p| {
        let l = luma(p[0], p[1], p[2]);
        let target = (l - 2.0 * amount * l * (1.0 - l) * (1.0 - l)).max(0.0);
        [p[0], p[1], p[2]] = scale_by_luma([p[0], p[1], p[2]], target);
    });
}

/// Colour temperature `t ∈ [-1, 1]`: `r·=1+0.15t`, `b·=1−0.15t`.
pub fn temperature(img: &mut Rgba32FImage, t: f32) {
    if t == 0.0 {
        return;
    }
    map_pixels(img, |p| {
        p[0] *= 1.0 + 0.15 * t;
        p[2] *= 1.0 - 0.15 * t;
    });
}

/// Gains that make the sampled colour `n` neutral: `mean(n)/max(n_c, eps)` normalized so the
/// largest gain is 1.
pub fn neutral_gains(n: [f32; 3]) -> [f32; 3] {
    let mean = (n[0] + n[1] + n[2]) / 3.0;
    let raw = n.map(|c| mean / c.max(EPS));
    let max = raw[0].max(raw[1]).max(raw[2]).max(EPS);
    raw.map(|g| clamp01(g / max))
}

/// Multiplies each channel by the gains for the neutral colour `n`.
pub fn neutral(img: &mut Rgba32FImage, n: [f32; 3]) {
    let g = neutral_gains(n);
    map_pixels(img, |p| {
        p[0] *= g[0];
        p[1] *= g[1];
        p[2] *= g[2];
    });
}

/// The Tuning op: neutral, temperature, fill light, highlights, shadows, each clamped.
pub fn tuning(
    img: &mut Rgba32FImage,
    fill: f32,
    high: f32,
    shade: f32,
    temp: f32,
    neutral_color: Option<[f32; 3]>,
) {
    if let Some(n) = neutral_color {
        neutral(img, n);
    }
    temperature(img, temp);
    fill_light(img, fill);
    highlights(img, high);
    shadows(img, shade);
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn grey(v: f32) -> Rgba32FImage {
        Rgba32FImage::from_pixel(2, 2, Rgba([v, v, v, 1.0]))
    }

    fn first(img: &Rgba32FImage) -> [f32; 4] {
        img.get_pixel(0, 0).0
    }

    #[test]
    fn fill_light_matches_the_formula() {
        let mut img = grey(0.25);
        fill_light(&mut img, 0.5);
        let expect = 0.25 + 2.0 * 0.5 * 0.25 * 0.75 * 0.75;
        assert!((first(&img)[0] - expect).abs() < 1e-6);
        let mut black = grey(0.0);
        fill_light(&mut black, 1.0);
        assert_eq!(
            first(&black)[..3],
            [0.0, 0.0, 0.0],
            "black stays black (L(1-L)^2 = 0)"
        );
        let mut white = grey(1.0);
        fill_light(&mut white, 1.0);
        assert_eq!(first(&white)[..3], [1.0, 1.0, 1.0]);
        let mut same = grey(0.3);
        fill_light(&mut same, 0.0);
        assert_eq!(same, grey(0.3));
    }

    #[test]
    fn fill_light_keeps_hue_and_never_darkens() {
        let mut img = Rgba32FImage::from_pixel(1, 1, Rgba([0.1, 0.2, 0.05, 1.0]));
        let l0 = luma(0.1, 0.2, 0.05);
        fill_light(&mut img, 0.8);
        let p = first(&img);
        assert!(luma(p[0], p[1], p[2]) > l0);
        assert!((p[0] / p[1] - 0.5).abs() < 1e-4);
    }

    #[test]
    fn highlights_and_shadows_match_the_formulas() {
        let mut h = grey(0.8);
        highlights(&mut h, 0.5);
        let expect = 0.8 + 2.0 * 0.5 * 0.64 * 0.2;
        assert!((first(&h)[0] - expect).abs() < 1e-6);
        let mut s = grey(0.3);
        shadows(&mut s, 0.5);
        let expect = 0.3 - 2.0 * 0.5 * 0.3 * 0.49;
        assert!((first(&s)[0] - expect).abs() < 1e-6);
        let mut black = grey(0.0);
        shadows(&mut black, 1.0);
        assert_eq!(first(&black)[0], 0.0);
    }

    #[test]
    fn temperature_warms_and_cools() {
        let mut warm = grey(0.5);
        temperature(&mut warm, 1.0);
        let p = first(&warm);
        assert!(
            (p[0] - 0.575).abs() < 1e-6 && (p[1] - 0.5).abs() < 1e-6 && (p[2] - 0.425).abs() < 1e-6
        );
        let mut cool = grey(0.5);
        temperature(&mut cool, -1.0);
        assert!(first(&cool)[0] < first(&cool)[2]);
    }

    #[test]
    fn neutral_gains_normalize_to_one() {
        let g = neutral_gains([0.6, 0.5, 0.4]);
        assert!(
            (g[2] - 1.0).abs() < 1e-6,
            "least-present channel keeps gain 1: {g:?}"
        );
        assert!(g[0] < g[1] && g[1] < g[2]);
        assert!(
            (0.6 * g[0] - 0.5 * g[1]).abs() < 1e-5,
            "sampled colour becomes grey"
        );
        assert!((0.5 * g[1] - 0.4 * g[2]).abs() < 1e-5);
        let flat = neutral_gains([0.5, 0.5, 0.5]);
        assert_eq!(flat, [1.0, 1.0, 1.0]);
        let g = neutral_gains([0.0, 0.0, 0.0]);
        assert!(g.iter().all(|v| v.is_finite()));
    }

    #[test]
    fn tuning_applies_steps_in_order_and_zero_is_identity() {
        let mut id = grey(0.4);
        tuning(&mut id, 0.0, 0.0, 0.0, 0.0, None);
        assert_eq!(id, grey(0.4));

        let mut combined = grey(0.4);
        tuning(&mut combined, 0.3, 0.2, 0.1, 0.5, Some([0.6, 0.5, 0.4]));
        let mut stepwise = grey(0.4);
        neutral(&mut stepwise, [0.6, 0.5, 0.4]);
        temperature(&mut stepwise, 0.5);
        fill_light(&mut stepwise, 0.3);
        highlights(&mut stepwise, 0.2);
        shadows(&mut stepwise, 0.1);
        assert_eq!(combined, stepwise);
    }
}
