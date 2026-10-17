//! Embedded ICC profiles are converted to sRGB on decode (SPEC §6.2, P3).

use image::{DynamicImage, RgbImage};
use moxcms::{ColorProfile, Layout, TransformOptions};

/// Converts `img` from `profile` into sRGB. The image is returned unchanged when the profile
/// cannot be read or the conversion fails.
pub(crate) fn to_srgb(img: DynamicImage, profile: &[u8]) -> DynamicImage {
    let Ok(src) = ColorProfile::new_from_slice(profile) else {
        tracing::warn!("embedded ICC profile could not be read; pixels left as stored");
        return img;
    };
    let dst = ColorProfile::new_srgb();
    let rgb = img.to_rgb8();
    let Ok(transform) =
        src.create_transform_8bit(Layout::Rgb, &dst, Layout::Rgb, TransformOptions::default())
    else {
        tracing::warn!("no transform from the embedded ICC profile to sRGB");
        return DynamicImage::ImageRgb8(rgb);
    };
    let mut out = vec![0_u8; rgb.as_raw().len()];
    if transform.transform(rgb.as_raw(), &mut out).is_err() {
        return DynamicImage::ImageRgb8(rgb);
    }
    RgbImage::from_raw(rgb.width(), rgb.height(), out)
        .map(DynamicImage::ImageRgb8)
        .unwrap_or_else(|| DynamicImage::ImageRgb8(rgb))
}

/// Inserts `profile` as an ICC APP2 segment immediately after the JPEG SOI marker.
#[cfg(test)]
fn embed_icc(jpeg: &[u8], profile: &[u8]) -> Option<Vec<u8>> {
    if jpeg.len() < 4 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 || profile.is_empty() {
        return None;
    }
    let mut out = Vec::with_capacity(jpeg.len() + profile.len() + 64);
    out.extend_from_slice(&jpeg[..2]);
    let header = b"ICC_PROFILE\0";
    let chunks: Vec<&[u8]> = profile.chunks(60_000).collect();
    let count = u8::try_from(chunks.len()).ok()?;
    for (i, chunk) in chunks.iter().enumerate() {
        let payload = header.len() + 2 + chunk.len();
        let seg_len = u16::try_from(payload + 2).ok()?;
        out.extend_from_slice(&[0xFF, 0xE2]);
        out.extend_from_slice(&seg_len.to_be_bytes());
        out.extend_from_slice(header);
        out.push(u8::try_from(i + 1).ok()?);
        out.push(count);
        out.extend_from_slice(chunk);
    }
    out.extend_from_slice(&jpeg[2..]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::{ExtendedColorType, Rgb, RgbImage};
    use std::io::Cursor;

    #[test]
    fn icc_display_p3_converted() {
        let encoded = Rgb([220_u8, 30, 90]);
        let image = RgbImage::from_pixel(8, 8, encoded);
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 100)
            .encode(image.as_raw(), 8, 8, ExtendedColorType::Rgb8)
            .unwrap();

        let plain = decode_temp(&jpeg);
        let plain_px = plain.to_rgb8().get_pixel(3, 3).0;

        let profile = ColorProfile::new_display_p3().encode().unwrap();
        let tagged = embed_icc(&jpeg, &profile).unwrap();
        assert!(tagged.windows(12).any(|w| w == b"ICC_PROFILE\0"));
        let converted = decode_temp(&tagged);
        let px = converted.to_rgb8().get_pixel(3, 3).0;

        let mut expected = [0_u8; 3];
        let src = ColorProfile::new_from_slice(&profile).unwrap();
        let transform = src
            .create_transform_8bit(
                Layout::Rgb,
                &ColorProfile::new_srgb(),
                Layout::Rgb,
                TransformOptions::default(),
            )
            .unwrap();
        transform.transform(&plain_px, &mut expected).unwrap();

        assert_ne!(
            px, plain_px,
            "Display P3 pixels must change when converted to sRGB: {px:?}"
        );
        for (got, want) in px.iter().zip(expected) {
            assert!(
                got.abs_diff(want) <= 2,
                "decoded {px:?} vs sRGB conversion {expected:?} (stored {plain_px:?})"
            );
        }
    }

    fn decode_temp(jpeg: &[u8]) -> DynamicImage {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("p3.jpg");
        std::fs::write(&path, jpeg).unwrap();
        let img = crate::decode(&path, None).unwrap();
        assert_eq!((img.width(), img.height()), (8, 8));
        img
    }

    #[test]
    fn jpeg_without_a_profile_is_unchanged() {
        let image = RgbImage::from_pixel(4, 4, Rgb([10, 20, 30]));
        let mut jpeg = Cursor::new(Vec::new());
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, 100)
            .encode(image.as_raw(), 4, 4, ExtendedColorType::Rgb8)
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("flat.jpg");
        std::fs::write(&path, jpeg.get_ref()).unwrap();
        let img = crate::decode(&path, None).unwrap().to_rgb8();
        let direct = {
            let mut dec = jpeg_decoder::Decoder::new(std::io::Cursor::new(jpeg.get_ref()));
            let pixels = dec.decode().unwrap();
            let info = dec.info().unwrap();
            RgbImage::from_raw(u32::from(info.width), u32::from(info.height), pixels).unwrap()
        };
        assert_eq!(img.as_raw(), direct.as_raw());
    }
}
