//! Building collages from photos (SPEC §6.13).

use crate::error::{AppError, Result};
use crate::export::encode_jpeg;
use crate::lightbox::Lightbox;
use image::{DynamicImage, RgbaImage};
use lb_core::PhotoId;
use lb_imaging::collage::{CollageSpec, PHOTO_LIMITS, render};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

pub use lb_imaging::collage::CollageLayout;

/// Longest edge of the live preview.
pub const PREVIEW_EDGE: u32 = 1024;
/// Largest photo decode size used for a collage.
const MAX_SOURCE_EDGE: u32 = 2048;
/// JPEG quality of saved collages.
const QUALITY: u8 = 90;

/// Output size presets (SPEC §6.13).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollageSize {
    /// 1920 x 1080.
    Hd,
    /// 3000 x 2000.
    Wide,
    /// 4 x 6 inch at 300 dpi: 1800 x 1200.
    Print4x6,
    /// 2048 x 2048.
    Square,
    /// Any size from 16 to 16384 pixels per side.
    Custom {
        /// Width in pixels.
        width: u32,
        /// Height in pixels.
        height: u32,
    },
}

impl CollageSize {
    /// Width and height in pixels.
    pub fn dimensions(self) -> (u32, u32) {
        match self {
            Self::Hd => (1920, 1080),
            Self::Wide => (3000, 2000),
            Self::Print4x6 => (1800, 1200),
            Self::Square => (2048, 2048),
            Self::Custom { width, height } => (width, height),
        }
    }

    /// Parses `WxH`.
    pub fn parse(text: &str) -> Option<Self> {
        let (w, h) = text.split_once(['x', 'X'])?;
        Some(Self::Custom {
            width: w.trim().parse().ok()?,
            height: h.trim().parse().ok()?,
        })
    }
}

/// Collage settings.
#[derive(Debug, Clone, PartialEq)]
pub struct CollageOptions {
    /// Arrangement.
    pub layout: CollageLayout,
    /// Output size.
    pub size: CollageSize,
    /// Gap between photos in pixels (at output size).
    pub spacing: u32,
    /// Background colour.
    pub background: [u8; 3],
    /// Seed of the pile's randomness; "Shuffle" picks another.
    pub seed: u64,
    /// The large photo of the frame layout (index into the photo list).
    pub frame_index: usize,
}

impl Default for CollageOptions {
    fn default() -> Self {
        Self {
            layout: CollageLayout::Grid,
            size: CollageSize::Hd,
            spacing: 8,
            background: [255, 255, 255],
            seed: 0,
            frame_index: 0,
        }
    }
}

impl CollageOptions {
    /// Checks size limits.
    pub fn validate(&self) -> Result<()> {
        let (w, h) = self.size.dimensions();
        if !(16..=16_384).contains(&w) || !(16..=16_384).contains(&h) {
            return Err(AppError::Invalid(
                "the collage size must be between 16 and 16384 pixels per side".into(),
            ));
        }
        Ok(())
    }

    fn spec(&self, scale: f32) -> CollageSpec {
        let (w, h) = self.size.dimensions();
        CollageSpec {
            layout: self.layout,
            width: ((w as f32 * scale).round() as u32).max(1),
            height: ((h as f32 * scale).round() as u32).max(1),
            spacing: (self.spacing as f32 * scale).round() as u32,
            background: self.background,
            seed: self.seed,
            frame_index: self.frame_index,
        }
    }
}

/// A saved collage.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CreatedCollage {
    /// The JPEG file.
    pub path: PathBuf,
    /// Its catalog id once indexed, when the folder belongs to the library.
    pub id: Option<PhotoId>,
}

/// Collage rendering and saving.
#[derive(Clone)]
pub struct CollageService {
    lb: Lightbox,
}

impl Lightbox {
    /// Collage creation.
    pub fn collage(&self) -> CollageService {
        CollageService { lb: self.clone() }
    }
}

impl CollageService {
    /// `{first root}/Collages`, the default save folder.
    pub fn default_dir(&self) -> Result<Option<PathBuf>> {
        Ok(self
            .lb
            .library()
            .roots()?
            .into_iter()
            .find(|r| r.mode != lb_core::RootMode::Exclude)
            .map(|r| r.path.join("Collages")))
    }

    fn load(&self, ids: &[PhotoId], edge: u32) -> Result<Vec<RgbaImage>> {
        if ids.len() < PHOTO_LIMITS.0 || ids.len() > PHOTO_LIMITS.1 {
            return Err(AppError::Invalid(format!(
                "a collage needs {} to {} photos",
                PHOTO_LIMITS.0, PHOTO_LIMITS.1
            )));
        }
        let cancel = AtomicBool::new(false);
        ids.iter()
            .map(|id| {
                let stack = self.lb.edits().stack(*id)?;
                self.lb.edits().render_stack_preview(
                    *id,
                    &stack,
                    edge.clamp(64, MAX_SOURCE_EDGE),
                    &cancel,
                )
            })
            .collect()
    }

    /// Renders the collage at full output size.
    pub fn render(&self, ids: &[PhotoId], opts: &CollageOptions) -> Result<RgbaImage> {
        opts.validate()?;
        let spec = opts.spec(1.0);
        let images = self.load(ids, spec.width.max(spec.height))?;
        Ok(render(&spec, &images))
    }

    /// Renders the live preview: the same layout scaled so the long edge is at most
    /// [`PREVIEW_EDGE`].
    pub fn preview(&self, ids: &[PhotoId], opts: &CollageOptions) -> Result<RgbaImage> {
        opts.validate()?;
        let (w, h) = opts.size.dimensions();
        let scale = (PREVIEW_EDGE as f32 / w.max(h) as f32).min(1.0);
        let spec = opts.spec(scale);
        let images = self.load(ids, spec.width.max(spec.height))?;
        Ok(render(&spec, &images))
    }

    /// Renders the collage, saves it as a JPEG at `out` (parent folders are created) and indexes
    /// it when `out` is inside a library root.
    pub fn create(
        &self,
        ids: &[PhotoId],
        opts: &CollageOptions,
        out: &Path,
    ) -> Result<CreatedCollage> {
        let image = self.render(ids, opts)?;
        let bytes = encode_jpeg(&DynamicImage::ImageRgba8(image), QUALITY)?;
        if let Some(dir) = out.parent() {
            std::fs::create_dir_all(dir)?;
        }
        crate::atomic::write_bytes_atomic(out, &bytes)?;
        let id = self.index(out)?;
        Ok(CreatedCollage {
            path: out.to_owned(),
            id,
        })
    }

    fn index(&self, path: &Path) -> Result<Option<PhotoId>> {
        let library = self.lb.library();
        let Ok(normal) = lb_scan::normalize_path(path) else {
            return Ok(None);
        };
        let root = library
            .roots()?
            .into_iter()
            .find(|r| normal.starts_with(&r.path));
        let Some(root) = root else { return Ok(None) };
        library.rescan(root.id)?;
        Ok(self.lb.catalog().photo_by_path(&normal)?.map(|p| p.id))
    }
}
