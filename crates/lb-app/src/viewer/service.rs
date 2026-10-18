//! Proxies and EXIF for files that are not in the library (SPEC-VIEWER §V7.1, §V8.2).

use std::path::{Path, PathBuf};

use image::DynamicImage;
use lb_core::Photo;
use lb_media::ExifData;

use crate::atomic::write_bytes_atomic;
use crate::error::{AppError, Result};
use crate::lightbox::Lightbox;

/// Viewer work that is not the catalog edit stack.
#[derive(Clone)]
pub struct ViewerService {
    lb: Lightbox,
}

impl Lightbox {
    /// Proxies, EXIF and path lookup for the viewer.
    #[must_use]
    pub fn viewer(&self) -> ViewerService {
        ViewerService { lb: self.clone() }
    }
}

impl ViewerService {
    /// The catalog photo at `path`, if one is indexed there.
    pub fn find_by_path(&self, path: &Path) -> Result<Option<Photo>> {
        Ok(self.lb.catalog().photo_by_path(path)?)
    }

    /// Query plan of [`Self::find_by_path`]. Tests assert it uses indexes.
    pub fn find_by_path_plan(&self) -> Result<Vec<String>> {
        Ok(self.lb.catalog().photo_by_path_plan()?)
    }

    /// EXIF of a file, catalog or not. Missing EXIF is the default value, not an error.
    #[must_use]
    pub fn exif_for_path(&self, path: &Path) -> ExifData {
        lb_media::read_exif(path)
    }

    /// Screen proxy for a non-catalog file: `{cache}/viewer/files/{quick_key}-{edge}.jpg`.
    ///
    /// A second call with the same bytes and edge returns the existing file and does not
    /// decode again. A changed file has a new quick key, so it gets a new path.
    pub fn proxy_for_path(&self, path: &Path, edge: u32) -> Result<PathBuf> {
        let key = lb_media::quick_key(path).map_err(AppError::from)?;
        let dir = self.lb.paths().cache.join("viewer").join("files");
        let dest = dir.join(format!("{key}-{edge}.jpg"));
        if dest.is_file() {
            return Ok(dest);
        }
        fs_create(&dir)?;
        let img = lb_media::decode(path, Some(edge.max(1))).map_err(AppError::from)?;
        let rgb = DynamicImage::ImageRgba8(img.to_rgba8()).to_rgb8();
        let mut bytes = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut bytes, 90)
            .encode_image(&rgb)
            .map_err(|e| AppError::Invalid(format!("viewer proxy could not be encoded: {e}")))?;
        write_bytes_atomic(&dest, &bytes)?;
        Ok(dest)
    }
}

fn fs_create(dir: &Path) -> Result<()> {
    std::fs::create_dir_all(dir)?;
    Ok(())
}
