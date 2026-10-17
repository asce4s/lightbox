//! Ambient backdrop (SPEC-VIEWER §V6.2).
//!
//! A soft, darkened blur of a thumbnail: cover-crop, box-downscale, Gaussian blur, grade,
//! vignette, then a Catmull-Rom upscale with Bayer dither.

use crate::blur::gaussian_blur;
use crate::color::{clamp01, luma, smoothstep};
use image::{DynamicImage, Rgba, Rgba32FImage, RgbaImage};

/// 4×4 Bayer matrix. Thresholds are `(entry + 0.5) / 16`, centred on 0.5.
const BAYER4: [u8; 16] = [0, 8, 2, 10, 12, 4, 14, 6, 3, 11, 1, 9, 15, 7, 13, 5];

/// Parameters of [`render_ambient`]. The defaults are the normative SPEC-VIEWER §V6.2 values.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct AmbientParams {
    /// Long edge of the blurred texture, in pixels.
    pub tex_long_edge: u32,
    /// Gaussian blur sigma, in pixels of that texture.
    pub sigma: f32,
    /// Saturation added on top of 1 (`+0.25` means a factor of 1.25).
    pub saturation: f32,
    /// Multiply applied after the saturation grade.
    pub gain: f32,
    /// Offset applied after the saturation grade.
    pub lift: f32,
    /// How much the corners darken. `0` leaves the image flat.
    pub vignette: f32,
    /// The texture is drawn at `1/out_scale` of the stage; the UI stretches it back.
    pub out_scale: u32,
}

impl Default for AmbientParams {
    fn default() -> Self {
        Self {
            tex_long_edge: 64,
            sigma: 3.0,
            saturation: 0.25,
            gain: 0.40,
            lift: 0.04,
            vignette: 0.35,
            out_scale: 4,
        }
    }
}

/// Pixel size of the texture for `stage` at `out_scale` (`ceil(stage / out_scale)`).
#[must_use]
pub fn ambient_size(stage: (u32, u32), out_scale: u32) -> (u32, u32) {
    let scale = out_scale.max(1);
    (
        stage.0.max(1).div_ceil(scale),
        stage.1.max(1).div_ceil(scale),
    )
}

/// Renders the ambient backdrop for `stage` (logical pixels) from `thumb`.
///
/// Values stay in sRGB-encoded `[0, 1]` (SPEC.md §6.6.2). Channels are clamped to
/// `lift + gain` after the upscale so Catmull-Rom ringing cannot break the brightness cap.
#[must_use]
pub fn render_ambient(thumb: &RgbaImage, stage: (u32, u32), p: &AmbientParams) -> RgbaImage {
    let stage = (stage.0.max(1), stage.1.max(1));
    let (ow, oh) = ambient_size(stage, p.out_scale);
    let cropped = cover_crop(thumb, stage);
    let small = box_downscale(&cropped, p.tex_long_edge.max(1));
    let blurred = gaussian_blur(&small, p.sigma);
    let mut graded = grade_and_vignette(&blurred, p);
    let up = image::imageops::resize(&graded, ow, oh, image::imageops::FilterType::CatmullRom);
    graded = up;
    let ceiling = finite(p.lift) + finite(p.gain);
    clamp_ceiling(&mut graded, ceiling);
    dither_rgba8(&graded)
}

fn finite(v: f32) -> f32 {
    if v.is_finite() { v } else { 0.0 }
}

fn ix(v: u32) -> usize {
    usize::try_from(v).unwrap_or(0)
}

fn cover_crop(img: &RgbaImage, stage: (u32, u32)) -> RgbaImage {
    let (tw, th) = img.dimensions();
    let (sw, sh) = stage;
    if tw == 0 || th == 0 || sw == 0 || sh == 0 {
        return RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255]));
    }
    let tw_h = u64::from(tw) * u64::from(sh);
    let th_w = u64::from(th) * u64::from(sw);
    let (cw, ch) = if tw_h > th_w {
        let cw = u32::try_from(th_w / u64::from(sh)).unwrap_or(tw);
        (cw.clamp(1, tw), th)
    } else if th_w > tw_h {
        let ch = u32::try_from(tw_h / u64::from(sw)).unwrap_or(th);
        (tw, ch.clamp(1, th))
    } else {
        (tw, th)
    };
    let x = tw.saturating_sub(cw) / 2;
    let y = th.saturating_sub(ch) / 2;
    image::imageops::crop_imm(img, x, y, cw, ch).to_image()
}

fn target_size(w: u32, h: u32, long_edge: u32) -> (u32, u32) {
    let long_edge = long_edge.max(1);
    let long = w.max(h).max(1);
    let short_src = w.min(h);
    let short = u32::try_from(
        (u64::from(long_edge) * u64::from(short_src) + u64::from(long) / 2) / u64::from(long),
    )
    .unwrap_or(1)
    .max(1);
    if w >= h {
        (long_edge, short)
    } else {
        (short, long_edge)
    }
}

fn box_downscale(img: &RgbaImage, long_edge: u32) -> Rgba32FImage {
    let (sw, sh) = img.dimensions();
    if sw == 0 || sh == 0 {
        return Rgba32FImage::from_pixel(1, 1, Rgba([0.0, 0.0, 0.0, 1.0]));
    }
    let (dw, dh) = target_size(sw, sh, long_edge);
    if (dw, dh) == (sw, sh) {
        return crate::convert::to_working(&DynamicImage::ImageRgba8(img.clone()));
    }
    let src = img.as_raw();
    let mut out =
        vec![0.0_f32; usize::try_from(dw).unwrap_or(1) * usize::try_from(dh).unwrap_or(1) * 4];
    for y in 0..dh {
        let y0 = f64::from(y) * f64::from(sh) / f64::from(dh);
        let y1 = f64::from(y + 1) * f64::from(sh) / f64::from(dh);
        for x in 0..dw {
            let x0 = f64::from(x) * f64::from(sw) / f64::from(dw);
            let x1 = f64::from(x + 1) * f64::from(sw) / f64::from(dw);
            let mut acc = [0.0_f64; 4];
            let mut weight = 0.0_f64;
            let iy0 = y0.floor().max(0.0) as u32;
            let iy1 = (y1.ceil() as u32).min(sh).max(iy0 + 1).min(sh);
            let ix0 = x0.floor().max(0.0) as u32;
            let ix1 = (x1.ceil() as u32).min(sw).max(ix0 + 1).min(sw);
            for sy in iy0..iy1 {
                let overlap_y = (f64::from(sy + 1).min(y1) - f64::from(sy).max(y0)).max(0.0);
                if overlap_y == 0.0 {
                    continue;
                }
                for sx in ix0..ix1 {
                    let overlap_x = (f64::from(sx + 1).min(x1) - f64::from(sx).max(x0)).max(0.0);
                    let w = overlap_x * overlap_y;
                    if w == 0.0 {
                        continue;
                    }
                    let i = (ix(sy) * ix(sw) + ix(sx)) * 4;
                    for (channel, sample) in acc.iter_mut().zip(src[i..i + 4].iter()) {
                        *channel += w * f64::from(*sample);
                    }
                    weight += w;
                }
            }
            let o = (ix(y) * ix(dw) + ix(x)) * 4;
            if weight > 0.0 {
                for (channel, sample) in out[o..o + 4].iter_mut().zip(acc) {
                    *channel = (sample / weight / 255.0) as f32;
                }
            }
        }
    }
    Rgba32FImage::from_raw(dw, dh, out).unwrap_or_else(|| Rgba32FImage::new(dw, dh))
}

fn grade_and_vignette(img: &Rgba32FImage, p: &AmbientParams) -> Rgba32FImage {
    let (w, h) = (img.width(), img.height());
    let cx = w as f32 * 0.5;
    let cy = h as f32 * 0.5;
    let corner = (cx - 0.5).hypot(cy - 0.5).max(1e-6);
    let sat = 1.0 + finite(p.saturation);
    let gain = finite(p.gain);
    let lift = finite(p.lift);
    let vignette = finite(p.vignette);
    let mut out = img.clone();
    for (x, y, px) in out.enumerate_pixels_mut() {
        let ch = &mut px.0;
        let l = luma(ch[0], ch[1], ch[2]);
        for channel in &mut ch[..3] {
            let v = clamp01(l + (*channel - l) * sat);
            *channel = clamp01(lift + gain * v);
        }
        let d = (x as f32 + 0.5 - cx).hypot(y as f32 + 0.5 - cy) / corner;
        let m = 1.0 - vignette * smoothstep(0.55, 1.0, d);
        for channel in &mut ch[..3] {
            *channel *= m;
        }
        ch[3] = 1.0;
    }
    out
}

fn clamp_ceiling(img: &mut Rgba32FImage, ceiling: f32) {
    let ceiling = if ceiling.is_finite() {
        ceiling.clamp(0.0, 1.0)
    } else {
        0.0
    };
    for px in img.pixels_mut() {
        for channel in &mut px.0[..3] {
            *channel = channel.clamp(0.0, ceiling);
        }
        px.0[3] = 1.0;
    }
}

fn dither_rgba8(img: &Rgba32FImage) -> RgbaImage {
    let (w, h) = (img.width(), img.height());
    let src = img.as_raw();
    let mut data = vec![0_u8; src.len()];
    for y in 0..h {
        for x in 0..w {
            let entry = BAYER4[ix((y & 3) * 4 + (x & 3))];
            let threshold = (f32::from(entry) + 0.5) / 16.0 - 0.5;
            let add = threshold / 255.0;
            let i = (ix(y) * ix(w) + ix(x)) * 4;
            for c in 0..3 {
                let v = clamp01(src[i + c] + add);
                data[i + c] = (v * 255.0 + 0.5) as u8;
            }
            data[i + 3] = 255;
        }
    }
    RgbaImage::from_raw(w, h, data).unwrap_or_else(|| RgbaImage::new(w, h))
}
