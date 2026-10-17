//! Conversions between 8-bit images and the `Rgba32F` working format (SPEC §6.6.2).

use image::{DynamicImage, Rgba32FImage, RgbaImage};

/// Converts to the working format: sRGB-encoded channels scaled to `[0, 1]`, no colour
/// management (embedded ICC profiles are ignored in P0).
pub fn to_working(img: &DynamicImage) -> Rgba32FImage {
    let rgba = img.to_rgba8();
    let (w, h) = rgba.dimensions();
    let data: Vec<f32> = rgba
        .as_raw()
        .iter()
        .map(|&v| f32::from(v) / 255.0)
        .collect();
    Rgba32FImage::from_raw(w, h, data).unwrap_or_else(|| Rgba32FImage::new(w, h))
}

/// Quantizes the working format back to 8 bits with round-to-nearest.
pub fn to_rgba8(img: &Rgba32FImage) -> RgbaImage {
    let data: Vec<u8> = img
        .as_raw()
        .iter()
        .map(|&v| (crate::color::clamp01(v) * 255.0 + 0.5) as u8)
        .collect();
    RgbaImage::from_raw(img.width(), img.height(), data)
        .unwrap_or_else(|| RgbaImage::new(img.width(), img.height()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;

    #[test]
    fn eight_bit_roundtrip_is_exact() {
        let mut img = RgbaImage::new(16, 16);
        for (i, p) in img.pixels_mut().enumerate() {
            let v = (i % 256) as u8;
            *p = Rgba([v, 255 - v, v / 2, 200]);
        }
        let back = to_rgba8(&to_working(&DynamicImage::ImageRgba8(img.clone())));
        assert_eq!(back, img);
    }

    #[test]
    fn quantizing_clamps_out_of_range_values() {
        let img = Rgba32FImage::from_pixel(1, 1, Rgba([-1.0, 2.0, f32::NAN, 0.5]));
        assert_eq!(to_rgba8(&img).get_pixel(0, 0).0, [0, 255, 0, 128]);
    }
}
