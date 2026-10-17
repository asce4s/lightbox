//! Text drawn onto images: export watermarks and collage captions (SPEC §6.10, §6.13).

use ab_glyph::{Font, FontRef, PxScale, ScaleFont, point};
use image::RgbaImage;

const FONT_BYTES: &[u8] = include_bytes!("../assets/Inter-Regular.ttf");

/// Watermark font size as a fraction of the image's long edge.
pub const WATERMARK_SIZE_RATIO: f32 = 0.03;
/// Opacity of the watermark text.
pub const WATERMARK_OPACITY: f32 = 0.6;

fn font() -> Option<FontRef<'static>> {
    FontRef::try_from_slice(FONT_BYTES).ok()
}

/// Width in pixels of `text` at font size `px`.
pub fn text_width(text: &str, px: f32) -> f32 {
    let Some(font) = font() else {
        return 0.0;
    };
    let scaled = font.as_scaled(PxScale::from(px));
    let mut width = 0.0;
    let mut prev = None;
    for c in text.chars() {
        let id = scaled.glyph_id(c);
        if let Some(p) = prev {
            width += scaled.kern(p, id);
        }
        width += scaled.h_advance(id);
        prev = Some(id);
    }
    width
}

fn blend(img: &mut RgbaImage, x: i64, y: i64, color: [u8; 3], alpha: f32) {
    let (Ok(x), Ok(y)) = (u32::try_from(x), u32::try_from(y)) else {
        return;
    };
    if x >= img.width() || y >= img.height() || alpha <= 0.0 {
        return;
    }
    let dst = img.get_pixel_mut(x, y);
    let a = alpha.clamp(0.0, 1.0);
    for c in 0..3 {
        let v = f32::from(dst[c]) * (1.0 - a) + f32::from(color[c]) * a;
        dst[c] = v.round().clamp(0.0, 255.0) as u8;
    }
    dst[3] = dst[3].max((a * 255.0).round() as u8);
}

/// Draws `text` with its baseline-left at `(x, baseline_y)` at size `px`, blending `color` with
/// `opacity` (0..=1).
pub fn draw_text(
    img: &mut RgbaImage,
    text: &str,
    x: f32,
    baseline_y: f32,
    px: f32,
    color: [u8; 3],
    opacity: f32,
) {
    let Some(font) = font() else {
        return;
    };
    let scaled = font.as_scaled(PxScale::from(px));
    let mut pen = x;
    let mut prev = None;
    for c in text.chars() {
        let id = scaled.glyph_id(c);
        if let Some(p) = prev {
            pen += scaled.kern(p, id);
        }
        let glyph = id.with_scale_and_position(PxScale::from(px), point(pen, baseline_y));
        pen += scaled.h_advance(id);
        prev = Some(id);
        if let Some(outline) = font.outline_glyph(glyph) {
            let bounds = outline.px_bounds();
            outline.draw(|gx, gy, coverage| {
                blend(
                    img,
                    bounds.min.x as i64 + i64::from(gx),
                    bounds.min.y as i64 + i64::from(gy),
                    color,
                    coverage * opacity,
                );
            });
        }
    }
}

/// Draws the export watermark: white text at 60% opacity with a 1 px dark shadow, bottom-right,
/// sized at 3% of the long edge (SPEC §6.10).
pub fn draw_watermark(img: &mut RgbaImage, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    let long = img.width().max(img.height()) as f32;
    let px = (long * WATERMARK_SIZE_RATIO).max(8.0);
    let width = text_width(text, px);
    let margin = px * 0.6;
    let x = img.width() as f32 - width - margin;
    let Some(font) = font() else {
        return;
    };
    let baseline = img.height() as f32 - margin - font.as_scaled(PxScale::from(px)).descent().abs();
    draw_text(
        img,
        text,
        x + 1.0,
        baseline + 1.0,
        px,
        [0, 0, 0],
        WATERMARK_OPACITY,
    );
    draw_text(
        img,
        text,
        x,
        baseline,
        px,
        [255, 255, 255],
        WATERMARK_OPACITY,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    fn opaque(r: u8, g: u8, b: u8) -> Rgba<u8> {
        Rgba([r, g, b, 255])
    }

    #[test]
    fn watermark_changes_bottom_right_only() {
        let base = RgbaImage::from_pixel(400, 300, opaque(20, 20, 20));
        let mut marked = base.clone();
        draw_watermark(&mut marked, "© Lightbox");
        let changed: Vec<(u32, u32)> = base
            .enumerate_pixels()
            .filter(|(x, y, p)| marked.get_pixel(*x, *y) != *p)
            .map(|(x, y, _)| (x, y))
            .collect();
        assert!(!changed.is_empty());
        assert!(changed.iter().all(|&(x, y)| x > 200 && y > 200));
        let brightest = changed
            .iter()
            .map(|&(x, y)| marked.get_pixel(x, y)[0])
            .max()
            .unwrap();
        assert!(
            brightest > 120 && brightest < 200,
            "60% white over dark grey: {brightest}"
        );
    }

    #[test]
    fn empty_text_and_tiny_images_are_safe() {
        let mut img = RgbaImage::from_pixel(4, 4, opaque(0, 0, 0));
        let before = img.clone();
        draw_watermark(&mut img, "   ");
        assert_eq!(img, before);
        draw_watermark(&mut img, "long text that does not fit");
        assert!(text_width("abc", 20.0) > 0.0);
    }
}
