//! HEIC/HEIF/AVIF decoding through libheif (SPEC §6.2.3, feature `heif`).
//!
//! libheif is linked only when `pkg-config` finds it at build time. Otherwise
//! [`crate::heif_decoder_available`] is false and decoding returns [`MediaError::Unsupported`].

use crate::{MediaError, Result, decode::present};
use std::path::Path;

/// Decodes a HEIF still image.
pub(crate) fn decode_heif(path: &Path, max_long_edge: Option<u32>) -> Result<image::DynamicImage> {
    #[cfg(lb_heif)]
    {
        let img = linked::decode(path)?;
        present(path, img, max_long_edge)
    }
    #[cfg(not(lb_heif))]
    {
        let _ = (path, max_long_edge);
        Err(MediaError::Unsupported(
            "HEIC/HEIF/AVIF decoding needs libheif installed at build time".into(),
        ))
    }
}

#[cfg(lb_heif)]
#[allow(unsafe_code)]
mod linked {
    use super::*;
    use std::ffi::{CStr, CString, c_char, c_int, c_void};
    use std::ptr;

    const COLORSPACE_RGB: c_int = 1;
    const CHROMA_INTERLEAVED_RGB: c_int = 10;
    const CHANNEL_INTERLEAVED: c_int = 10;

    #[repr(C)]
    struct HeifError {
        code: c_int,
        subcode: c_int,
        message: *const c_char,
    }

    unsafe extern "C" {
        fn heif_context_alloc() -> *mut c_void;
        fn heif_context_free(ctx: *mut c_void);
        fn heif_context_read_from_file(
            ctx: *mut c_void,
            filename: *const c_char,
            options: *const c_void,
        ) -> HeifError;
        fn heif_context_get_primary_image_handle(
            ctx: *mut c_void,
            out: *mut *mut c_void,
        ) -> HeifError;
        fn heif_image_handle_release(handle: *const c_void);
        fn heif_image_handle_get_width(handle: *const c_void) -> c_int;
        fn heif_image_handle_get_height(handle: *const c_void) -> c_int;
        fn heif_decode_image(
            handle: *const c_void,
            out: *mut *mut c_void,
            colorspace: c_int,
            chroma: c_int,
            options: *const c_void,
        ) -> HeifError;
        fn heif_image_release(image: *const c_void);
        fn heif_image_get_plane_readonly(
            image: *const c_void,
            channel: c_int,
            stride: *mut c_int,
        ) -> *const u8;
    }

    struct Context(*mut c_void);
    impl Drop for Context {
        fn drop(&mut self) {
            if !self.0.is_null() {
                unsafe { heif_context_free(self.0) }
            }
        }
    }

    fn check(err: HeifError) -> Result<()> {
        if err.code == 0 {
            return Ok(());
        }
        let message = if err.message.is_null() {
            format!("libheif error {}", err.subcode)
        } else {
            unsafe { CStr::from_ptr(err.message) }
                .to_string_lossy()
                .into_owned()
        };
        Err(MediaError::Corrupt(message))
    }

    pub(super) fn decode(path: &Path) -> Result<image::DynamicImage> {
        let bytes = path.as_os_str().as_encoded_bytes();
        let name = CString::new(bytes)
            .map_err(|_| MediaError::Corrupt("path contains an interior NUL byte".into()))?;
        let ctx = Context(unsafe { heif_context_alloc() });
        if ctx.0.is_null() {
            return Err(MediaError::Corrupt(
                "libheif could not allocate a context".into(),
            ));
        }
        check(unsafe { heif_context_read_from_file(ctx.0, name.as_ptr(), ptr::null()) })?;
        let mut handle = ptr::null_mut();
        check(unsafe { heif_context_get_primary_image_handle(ctx.0, &mut handle) })?;
        if handle.is_null() {
            return Err(MediaError::Corrupt("HEIF file has no primary image".into()));
        }
        let width = unsafe { heif_image_handle_get_width(handle) };
        let height = unsafe { heif_image_handle_get_height(handle) };
        let mut image = ptr::null_mut();
        let decoded = check(unsafe {
            heif_decode_image(
                handle,
                &mut image,
                COLORSPACE_RGB,
                CHROMA_INTERLEAVED_RGB,
                ptr::null(),
            )
        });
        unsafe { heif_image_handle_release(handle) };
        decoded?;
        if image.is_null() {
            return Err(MediaError::Corrupt("libheif returned no image".into()));
        }
        let rgb = copy_rgb(image, width, height);
        unsafe { heif_image_release(image) };
        rgb
    }

    fn copy_rgb(image: *mut c_void, width: c_int, height: c_int) -> Result<image::DynamicImage> {
        let (Ok(width), Ok(height)) = (u32::try_from(width), u32::try_from(height)) else {
            return Err(MediaError::Corrupt(
                "HEIF image has a negative dimension".into(),
            ));
        };
        if width == 0 || height == 0 {
            return Err(MediaError::Corrupt("HEIF image is empty".into()));
        }
        let mut stride: c_int = 0;
        let plane =
            unsafe { heif_image_get_plane_readonly(image, CHANNEL_INTERLEAVED, &mut stride) };
        if plane.is_null() || stride <= 0 {
            return Err(MediaError::Corrupt(
                "HEIF image has no interleaved RGB plane".into(),
            ));
        }
        let row_bytes = (width as usize).saturating_mul(3);
        let stride = stride as usize;
        if stride < row_bytes {
            return Err(MediaError::Corrupt(
                "HEIF RGB stride is shorter than a row".into(),
            ));
        }
        let mut pixels = Vec::with_capacity(row_bytes * height as usize);
        for y in 0..height as usize {
            let row = unsafe { std::slice::from_raw_parts(plane.add(y * stride), row_bytes) };
            pixels.extend_from_slice(row);
        }
        image::RgbImage::from_raw(width, height, pixels)
            .map(image::DynamicImage::ImageRgb8)
            .ok_or_else(|| MediaError::Corrupt("HEIF pixel buffer has the wrong size".into()))
    }
}

#[cfg(test)]
mod tests {
    fn fixture() -> std::path::PathBuf {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiny_8x8.heic");
        assert!(path.is_file(), "missing {}", path.display());
        path
    }

    #[cfg(lb_heif)]
    #[test]
    fn heif_decodes_small_fixture() {
        let img = crate::decode(&fixture(), None).unwrap().to_rgb8();
        assert_eq!((img.width(), img.height()), (8, 8));
        let px = img.get_pixel(4, 4).0;
        assert!(
            px[0] > 180 && px[1] < 60 && px[2] < 60,
            "8x8 red HEIC decoded to {px:?}"
        );
        let shrunk = crate::decode(&fixture(), Some(4)).unwrap();
        assert_eq!(shrunk.width().max(shrunk.height()), 4);
    }

    /// Linux CI installs libheif, so a Linux build of this feature that cannot decode is a
    /// missing package, not a supported fallback.
    #[cfg(all(not(lb_heif), target_os = "linux"))]
    #[test]
    fn heif_decodes_small_fixture() {
        panic!("install libheif-dev and libheif-plugin-libde265, then rebuild");
    }

    /// Other platforms compile the feature without libheif and report that clearly.
    #[cfg(all(not(lb_heif), not(target_os = "linux")))]
    #[test]
    fn heif_decodes_small_fixture() {
        let err = crate::decode(&fixture(), None).unwrap_err();
        assert!(matches!(err, crate::MediaError::Unsupported(_)), "{err:?}");
    }
}
