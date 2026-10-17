//! Colour and focus effects (SPEC §6.6.3).

use super::tuning;
use crate::blur::gaussian_blur;
use crate::color::{clamp01, luma, map_pixels, map_pixels_xy, mix, smoothstep};
use image::Rgba32FImage;
use lb_core::NormRect;
use rand_core::{Rng, SeedableRng};
use rand_pcg::Pcg32;

/// Image geometry shared by the spatial effects: pixel centres map to normalized coordinates
/// and distances are measured in units of the long edge, so a proxy renders like the full image.
#[derive(Clone, Copy)]
struct Frame {
    w: f32,
    h: f32,
    long: f32,
}

impl Frame {
    fn of(img: &Rgba32FImage) -> Self {
        let (w, h) = (img.width() as f32, img.height() as f32);
        Self {
            w,
            h,
            long: w.max(h).max(1.0),
        }
    }

    fn uv(self, x: u32, y: u32) -> (f32, f32) {
        ((x as f32 + 0.5) / self.w, (y as f32 + 0.5) / self.h)
    }

    /// Distance of pixel `(x, y)` from the normalized centre `(cx, cy)` in long-edge units.
    fn distance(self, x: u32, y: u32, cx: f32, cy: f32) -> f32 {
        let (u, v) = self.uv(x, y);
        let (dx, dy) = ((u - cx) * self.w / self.long, (v - cy) * self.h / self.long);
        (dx * dx + dy * dy).sqrt()
    }
}

/// Unsharp mask with a σ = 1 Gaussian: `v' = v + a·(v − blur)`.
pub fn sharpen(img: &mut Rgba32FImage, amount: f32) {
    if amount == 0.0 {
        return;
    }
    let blurred = gaussian_blur(img, 1.0);
    for (px, bl) in img.pixels_mut().zip(blurred.pixels()) {
        for c in 0..3 {
            px.0[c] = clamp01(px.0[c] + amount * (px.0[c] - bl.0[c]));
        }
    }
}

/// `r = g = b = L`.
pub fn black_and_white(img: &mut Rgba32FImage) {
    map_pixels(img, |p| {
        let l = luma(p[0], p[1], p[2]);
        p[..3].fill(l);
    });
}

/// The classic sepia matrix.
pub fn sepia(img: &mut Rgba32FImage) {
    map_pixels(img, |p| {
        let (r, g, b) = (p[0], p[1], p[2]);
        p[0] = 0.393 * r + 0.769 * g + 0.189 * b;
        p[1] = 0.349 * r + 0.686 * g + 0.168 * b;
        p[2] = 0.272 * r + 0.534 * g + 0.131 * b;
    });
}

/// `v' = L + (v − L)(1 + s)`.
pub fn saturation(img: &mut Rgba32FImage, s: f32) {
    if s == 0.0 {
        return;
    }
    map_pixels(img, |p| {
        let l = luma(p[0], p[1], p[2]);
        for c in &mut p[..3] {
            *c = l + (*c - l) * (1.0 + s);
        }
    });
}

/// Temperature +0.4 then saturation +0.15.
pub fn warmify(img: &mut Rgba32FImage) {
    tuning::temperature(img, 0.4);
    saturation(img, 0.15);
}

/// `v' = p·v + (1 − p)·C·L`.
pub fn tint(img: &mut Rgba32FImage, color: [f32; 3], preserve: f32) {
    map_pixels(img, |p| {
        let l = luma(p[0], p[1], p[2]);
        for c in 0..3 {
            p[c] = preserve * p[c] + (1.0 - preserve) * color[c] * l;
        }
    });
}

/// First eight bytes of a quick key (zero padded), little endian: the grain seed base.
pub fn seed_from_quick_key(quick_key: &str) -> u64 {
    let mut bytes = [0_u8; 8];
    for (b, k) in bytes.iter_mut().zip(quick_key.bytes()) {
        *b = k;
    }
    u64::from_le_bytes(bytes)
}

/// Seeded film grain: `v' = v + a·0.12·n·(1 − |2L − 1|)` with `n ~ U(−1, 1)` shared by the
/// channels. Noise is drawn row by row from `Pcg32` seeded with `seed`.
pub fn film_grain(img: &mut Rgba32FImage, amount: f32, seed: u64) {
    if amount == 0.0 {
        return;
    }
    let mut rng = Pcg32::seed_from_u64(seed);
    let noise: Vec<f32> = (0..img.width() as usize * img.height() as usize)
        .map(|_| (rng.next_u32() as f32 / 4_294_967_296.0) * 2.0 - 1.0)
        .collect();
    let w = img.width();
    map_pixels_xy(img, |x, y, p| {
        let n = noise[y as usize * w as usize + x as usize];
        let l = luma(p[0], p[1], p[2]);
        let strength = amount * 0.12 * n * (1.0 - (2.0 * l - 1.0).abs());
        for c in &mut p[..3] {
            *c += strength;
        }
    });
}

/// Blurs outside a circle: `m = smoothstep(radius, radius + 0.25, d)·amount`,
/// `v' = mix(v, blur, m)` with σ = 0.01·long edge.
pub fn soft_focus(img: &mut Rgba32FImage, cx: f32, cy: f32, radius: f32, amount: f32) {
    if amount == 0.0 {
        return;
    }
    let frame = Frame::of(img);
    let blurred = gaussian_blur(img, 0.01 * frame.long);
    map_pixels_xy(img, |x, y, p| {
        let m = smoothstep(radius, radius + 0.25, frame.distance(x, y, cx, cy)) * amount;
        let b = blurred.get_pixel(x, y).0;
        for c in 0..3 {
            p[c] = mix(p[c], b[c], m);
        }
    });
}

/// Bloom: the bright part `max(0, v − 0.5)·2` is blurred (σ = r·0.02·long edge) and screened
/// back: `v' = 1 − (1 − v)(1 − i·B)`.
pub fn glow(img: &mut Rgba32FImage, intensity: f32, radius: f32) {
    if intensity == 0.0 {
        return;
    }
    let frame = Frame::of(img);
    let mut bright = img.clone();
    map_pixels(&mut bright, |p| {
        for c in &mut p[..3] {
            *c = (*c - 0.5).max(0.0) * 2.0;
        }
    });
    let glow = gaussian_blur(&bright, radius * 0.02 * frame.long);
    map_pixels_xy(img, |x, y, p| {
        let b = glow.get_pixel(x, y).0;
        for c in 0..3 {
            p[c] = 1.0 - (1.0 - p[c]) * (1.0 - intensity * b[c]);
        }
    });
}

/// Colour inside a circle, grey outside: `m = smoothstep(radius, radius + feather, d)` with
/// `feather = 0.3·(1 − sharpness) + 0.01`, `v' = mix(v, L, m)`.
pub fn focal_black_and_white(
    img: &mut Rgba32FImage,
    cx: f32,
    cy: f32,
    radius: f32,
    sharpness: f32,
) {
    let frame = Frame::of(img);
    let feather = 0.3 * (1.0 - sharpness) + 0.01;
    map_pixels_xy(img, |x, y, p| {
        let m = smoothstep(radius, radius + feather, frame.distance(x, y, cx, cy));
        let l = luma(p[0], p[1], p[2]);
        for c in &mut p[..3] {
            *c = mix(*c, l, m);
        }
    });
}

/// Tint fading out downwards: `m = amount·(1 − smoothstep(y − feather, y + feather, py))`,
/// blended towards `C·L` (a `Tint` with `preserve = 0`).
pub fn graduated_tint(img: &mut Rgba32FImage, color: [f32; 3], y: f32, feather: f32, amount: f32) {
    let frame = Frame::of(img);
    map_pixels_xy(img, |px, py, p| {
        let (_, v) = frame.uv(px, py);
        let m = amount * (1.0 - smoothstep(y - feather, y + feather, v));
        let l = luma(p[0], p[1], p[2]);
        for c in 0..3 {
            p[c] = mix(p[c], color[c] * l, m);
        }
    });
}

/// `v' = v·(1 − a·smoothstep(r, 1, d·√2))` with `d` the elliptical distance from the centre
/// in normalized coordinates (so the corners are at `d·√2 = 1`).
pub fn vignette(img: &mut Rgba32FImage, amount: f32, radius: f32) {
    if amount == 0.0 {
        return;
    }
    let frame = Frame::of(img);
    map_pixels_xy(img, |x, y, p| {
        let (u, v) = frame.uv(x, y);
        let d = ((u - 0.5) * (u - 0.5) + (v - 0.5) * (v - 0.5)).sqrt();
        let k = 1.0 - amount * smoothstep(radius, 1.0, d * std::f32::consts::SQRT_2);
        for c in &mut p[..3] {
            *c *= k;
        }
    });
}

/// Inside each region's inscribed ellipse, pixels with `r > 1.5·max(g, b)` and `r > 0.2` get
/// `r' = (g + b)/2`.
pub fn red_eye(img: &mut Rgba32FImage, regions: &[NormRect]) {
    let frame = Frame::of(img);
    map_pixels_xy(img, |x, y, p| {
        let (u, v) = frame.uv(x, y);
        let in_region = regions.iter().any(|r| {
            let (rx, ry) = (r.w / 2.0, r.h / 2.0);
            if rx <= 0.0 || ry <= 0.0 {
                return false;
            }
            let (dx, dy) = ((u - (r.x + rx)) / rx, (v - (r.y + ry)) / ry);
            dx * dx + dy * dy <= 1.0
        });
        if in_region && p[0] > 1.5 * p[1].max(p[2]) && p[0] > 0.2 {
            p[0] = (p[1] + p[2]) / 2.0;
        }
    });
}
