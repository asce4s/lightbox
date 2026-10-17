//! Resizing helpers built on `fast_image_resize`.

use fast_image_resize::images::Image;
use fast_image_resize::{PixelType, Resizer};
use image::{DynamicImage, RgbImage, RgbaImage};

/// Dimensions that fit `(w, h)` inside a square of `max_edge`, preserving aspect ratio and
/// never upscaling. The long edge equals `max_edge` exactly when the image is larger.
pub fn fit_within(w: u32, h: u32, max_edge: u32) -> (u32, u32) {
    let long = w.max(h);
    if long <= max_edge || max_edge == 0 || w == 0 || h == 0 {
        return (w.max(1), h.max(1));
    }
    let scale = |v: u32| {
        let scaled = (u64::from(v) * u64::from(max_edge) + u64::from(long) / 2) / u64::from(long);
        u32::try_from(scaled).unwrap_or(max_edge).max(1)
    };
    if w >= h {
        (max_edge, scale(h))
    } else {
        (scale(w), max_edge)
    }
}

/// Resizes to exactly `w × h` with a Lanczos3 filter. Images with an alpha channel keep it;
/// everything else is converted to RGB8.
pub fn resize_exact(img: &DynamicImage, w: u32, h: u32) -> DynamicImage {
    let (w, h) = (w.max(1), h.max(1));
    if img.width() == w && img.height() == h {
        return img.clone();
    }
    let has_alpha = img.color().has_alpha();
    let (src_pixels, kind) = if has_alpha {
        (img.to_rgba8().into_raw(), PixelType::U8x4)
    } else {
        (img.to_rgb8().into_raw(), PixelType::U8x3)
    };
    let Ok(src) = Image::from_vec_u8(img.width(), img.height(), src_pixels, kind) else {
        return img.resize_exact(w, h, image::imageops::FilterType::Triangle);
    };
    let mut dst = Image::new(w, h, kind);
    if Resizer::new().resize(&src, &mut dst, None).is_err() {
        return img.resize_exact(w, h, image::imageops::FilterType::Triangle);
    }
    let buf = dst.into_vec();
    if has_alpha {
        RgbaImage::from_raw(w, h, buf).map_or_else(|| img.clone(), DynamicImage::ImageRgba8)
    } else {
        RgbImage::from_raw(w, h, buf).map_or_else(|| img.clone(), DynamicImage::ImageRgb8)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fit_within_table() {
        assert_eq!(fit_within(640, 480, 160), (160, 120));
        assert_eq!(fit_within(480, 640, 160), (120, 160));
        assert_eq!(fit_within(6000, 4000, 160), (160, 107));
        assert_eq!(fit_within(100, 50, 160), (100, 50), "never upscales");
        assert_eq!(fit_within(1, 1, 160), (1, 1));
        assert_eq!(
            fit_within(10_000, 1, 100),
            (100, 1),
            "thin images keep one pixel"
        );
    }

    #[test]
    fn resize_exact_gives_requested_size_and_keeps_flat_colour() {
        let img = DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 48, image::Rgb([10, 200, 30])));
        let out = resize_exact(&img, 16, 12);
        assert_eq!((out.width(), out.height()), (16, 12));
        assert_eq!(out.to_rgb8().get_pixel(8, 6).0, [10, 200, 30]);
        let rgba =
            DynamicImage::ImageRgba8(RgbaImage::from_pixel(8, 8, image::Rgba([1, 2, 3, 128])));
        let out = resize_exact(&rgba, 4, 4);
        assert_eq!(out.to_rgba8().get_pixel(1, 1).0[3], 128);
    }
}
