//! Separable Gaussian blur with edge clamping (SPEC §6.6.3).

use image::Rgba32FImage;
use rayon::prelude::*;

/// Kernel radius used for a given sigma: `ceil(3σ)`, at least 1.
pub fn kernel_radius(sigma: f32) -> usize {
    (3.0 * sigma.max(0.0)).ceil().max(1.0) as usize
}

/// Normalized 1-D Gaussian kernel with `2·radius + 1` taps that sums to 1.
pub fn gaussian_kernel(sigma: f32, radius: usize) -> Vec<f32> {
    let sigma = f64::from(sigma.max(1e-3));
    let mut weights: Vec<f64> = (0..=2 * radius)
        .map(|i| {
            let d = i as f64 - radius as f64;
            (-(d * d) / (2.0 * sigma * sigma)).exp()
        })
        .collect();
    let sum: f64 = weights.iter().sum();
    for w in &mut weights {
        *w /= sum;
    }
    weights.into_iter().map(|w| w as f32).collect()
}

/// Blurs the colour channels of `img` with a Gaussian of standard deviation `sigma` pixels and
/// kernel radius `ceil(3σ)`. Alpha is copied unchanged. `sigma <= 0` returns a copy.
pub fn gaussian_blur(img: &Rgba32FImage, sigma: f32) -> Rgba32FImage {
    gaussian_blur_radius(img, sigma, kernel_radius(sigma))
}

/// [`gaussian_blur`] with an explicit kernel radius.
pub fn gaussian_blur_radius(img: &Rgba32FImage, sigma: f32, radius: usize) -> Rgba32FImage {
    let (w, h) = (img.width() as usize, img.height() as usize);
    if sigma <= 0.0 || w == 0 || h == 0 {
        return img.clone();
    }
    let kernel = gaussian_kernel(sigma, radius);
    let src: &[f32] = img.as_raw();
    let mut tmp = vec![0.0_f32; src.len()];
    tmp.par_chunks_mut(w * 4)
        .zip(src.par_chunks(w * 4))
        .for_each(|(out, row)| horizontal(row, out, w, radius, &kernel));
    let mut out = vec![0.0_f32; src.len()];
    out.par_chunks_mut(w * 4)
        .enumerate()
        .for_each(|(y, out_row)| vertical(&tmp, out_row, y, w, h, radius, &kernel));
    for (o, s) in out.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        o[3] = s[3];
    }
    Rgba32FImage::from_raw(img.width(), img.height(), out).unwrap_or_else(|| img.clone())
}

fn horizontal(row: &[f32], out: &mut [f32], w: usize, radius: usize, kernel: &[f32]) {
    for x in 0..w {
        let mut acc = [0.0_f32; 3];
        for (k, weight) in kernel.iter().enumerate() {
            let sx = (x + k).saturating_sub(radius).min(w - 1);
            let px = &row[sx * 4..sx * 4 + 3];
            acc[0] += weight * px[0];
            acc[1] += weight * px[1];
            acc[2] += weight * px[2];
        }
        out[x * 4..x * 4 + 3].copy_from_slice(&acc);
    }
}

fn vertical(
    tmp: &[f32],
    out_row: &mut [f32],
    y: usize,
    w: usize,
    h: usize,
    radius: usize,
    kernel: &[f32],
) {
    for (k, weight) in kernel.iter().enumerate() {
        let sy = (y + k).saturating_sub(radius).min(h - 1);
        let src = &tmp[sy * w * 4..(sy + 1) * w * 4];
        for x in 0..w {
            let i = x * 4;
            out_row[i] += weight * src[i];
            out_row[i + 1] += weight * src[i + 1];
            out_row[i + 2] += weight * src[i + 2];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn gaussian_kernel_sums_to_one() {
        for sigma in [0.5_f32, 1.0, 2.5, 10.0, 60.0] {
            let radius = kernel_radius(sigma);
            let kernel = gaussian_kernel(sigma, radius);
            assert_eq!(kernel.len(), 2 * radius + 1);
            let sum: f32 = kernel.iter().sum();
            assert!((sum - 1.0).abs() < 1e-5, "sigma {sigma}: sum {sum}");
            for i in 0..radius {
                assert_eq!(kernel[i], kernel[2 * radius - i], "symmetric");
            }
            assert!(kernel[radius] >= kernel[radius - 1], "peaks in the middle");
        }
        assert_eq!(kernel_radius(1.0), 3, "sharpen uses radius 3 at sigma 1");
    }

    #[test]
    fn blur_constant_image_unchanged() {
        let img = Rgba32FImage::from_pixel(23, 17, Rgba([0.3, 0.6, 0.9, 0.5]));
        for sigma in [0.7_f32, 1.0, 4.0] {
            let out = gaussian_blur(&img, sigma);
            for (a, b) in out.as_raw().iter().zip(img.as_raw()) {
                assert!((a - b).abs() < 1e-6, "sigma {sigma}: {a} vs {b}");
            }
        }
    }

    #[test]
    fn blur_spreads_an_impulse_symmetrically_and_conserves_energy() {
        let mut img = Rgba32FImage::new(21, 21);
        img.put_pixel(10, 10, Rgba([1.0, 1.0, 1.0, 1.0]));
        let out = gaussian_blur(&img, 1.5);
        let total: f32 = out.pixels().map(|p| p.0[0]).sum();
        assert!((total - 1.0).abs() < 1e-4, "energy {total}");
        assert_eq!(out.get_pixel(9, 10).0[0], out.get_pixel(11, 10).0[0]);
        assert_eq!(out.get_pixel(10, 9).0[0], out.get_pixel(10, 11).0[0]);
        assert!(out.get_pixel(10, 10).0[0] < 1.0);
        assert!(out.get_pixel(10, 10).0[0] > out.get_pixel(11, 10).0[0]);
    }

    #[test]
    fn blur_keeps_alpha_and_handles_edges_and_degenerate_input() {
        let mut img = Rgba32FImage::from_pixel(3, 2, Rgba([0.2, 0.2, 0.2, 0.25]));
        img.put_pixel(0, 0, Rgba([1.0, 1.0, 1.0, 0.75]));
        let out = gaussian_blur(&img, 8.0);
        assert_eq!(out.get_pixel(0, 0).0[3], 0.75);
        assert_eq!(out.get_pixel(2, 1).0[3], 0.25);
        assert_eq!(gaussian_blur(&img, 0.0), img);
        let empty = Rgba32FImage::new(0, 0);
        assert_eq!(gaussian_blur(&empty, 2.0).dimensions(), (0, 0));
        let one = Rgba32FImage::from_pixel(1, 1, Rgba([0.4, 0.5, 0.6, 1.0]));
        let out = gaussian_blur(&one, 3.0);
        assert!((out.get_pixel(0, 0).0[1] - 0.5).abs() < 1e-6);
    }

    #[test]
    fn blur_is_bit_identical_across_runs() {
        let mut img = Rgba32FImage::new(64, 48);
        for (i, p) in img.pixels_mut().enumerate() {
            let v = ((i * 2_654_435_761) % 1000) as f32 / 1000.0;
            *p = Rgba([v, 1.0 - v, v * v, 1.0]);
        }
        assert_eq!(gaussian_blur(&img, 2.0), gaussian_blur(&img, 2.0));
    }
}
