//! Fixture manifest model (`tests/fixtures/manifest.toml`, SPEC §12.3).

use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

/// The parsed manifest: the single source of truth for fixture contents and expectations.
#[derive(Debug, Clone, Deserialize)]
pub struct Manifest {
    /// Manifest format version.
    pub version: u32,
    /// Modification time (Unix seconds) applied to every generated file.
    pub mtime_unix: u64,
    /// All fixture files.
    #[serde(rename = "file")]
    pub files: Vec<FileSpec>,
}

/// How to generate one fixture file.
#[derive(Debug, Clone, Deserialize)]
pub struct FileSpec {
    /// Path relative to the generated root (forward slashes).
    pub path: String,
    /// Kind of content to write.
    pub kind: Kind,
    /// Procedural pattern for image kinds.
    pub pattern: Option<Pattern>,
    /// Pixel width for image kinds.
    pub width: Option<u32>,
    /// Pixel height for image kinds.
    pub height: Option<u32>,
    /// Pattern seed.
    #[serde(default)]
    pub seed: u64,
    /// JPEG quality (1..=100).
    pub quality: Option<u8>,
    /// Frame count for animated GIFs.
    pub frames: Option<u32>,
    /// Source path for `copy`.
    pub of: Option<String>,
    /// Literal contents for `text`.
    pub contents: Option<String>,
    /// EXIF fields to embed (JPEG only).
    pub exif: Option<ExifSpec>,
    /// What scanners/readers are expected to observe.
    pub expect: Expect,
}

/// Kind of fixture content.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// Baseline JPEG, optionally with EXIF.
    Jpeg,
    /// PNG.
    Png,
    /// Animated GIF.
    Gif,
    /// Lossless WebP.
    Webp,
    /// BMP.
    Bmp,
    /// TIFF.
    Tiff,
    /// Empty file.
    ZeroByte,
    /// A valid JPEG cut short.
    TruncatedJpeg,
    /// Literal text.
    Text,
    /// Byte-identical copy of another fixture (`of`).
    Copy,
}

/// Resolution-independent procedural patterns.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pattern {
    /// Smooth colour gradient.
    Gradient,
    /// Grid of flat colour patches.
    Patches,
    /// Checkerboard.
    Checker,
    /// Gradient background with a few seeded shapes.
    Scene,
}

/// EXIF values written into a fixture JPEG.
#[derive(Debug, Clone, Default, Deserialize)]
pub struct ExifSpec {
    /// `Make`.
    pub make: Option<String>,
    /// `Model`.
    pub model: Option<String>,
    /// `LensModel`.
    pub lens: Option<String>,
    /// `ISOSpeedRatings`.
    pub iso: Option<u16>,
    /// `FNumber`.
    pub f_number: Option<f64>,
    /// `ExposureTime` as `"1/250"` or decimal seconds.
    pub exposure: Option<String>,
    /// `FocalLength` in millimetres.
    pub focal_mm: Option<f64>,
    /// `DateTimeOriginal` as `YYYY-MM-DDTHH:MM:SS`.
    pub taken: Option<String>,
    /// `OffsetTimeOriginal`, e.g. `+01:00`.
    pub offset: Option<String>,
    /// `Orientation` 1..=8.
    pub orientation: Option<u16>,
    /// `ImageDescription`.
    pub description: Option<String>,
    /// GPS latitude/longitude in signed decimal degrees.
    pub gps: Option<[f64; 2]>,
    /// GPS altitude in metres (negative = below sea level).
    pub altitude: Option<f64>,
}

/// Expected observations for one fixture file.
#[derive(Debug, Clone, Deserialize)]
pub struct Expect {
    /// Scan outcome.
    pub status: Status,
    /// Width after EXIF orientation.
    pub width: Option<u32>,
    /// Height after EXIF orientation.
    pub height: Option<u32>,
    /// Expected `taken_at` as naive local `YYYY-MM-DDTHH:MM:SS`; absent when it comes from mtime.
    pub taken_at: Option<String>,
    /// Expected `taken_at_src`: `exif`, `filename` or `mtime`.
    pub taken_at_src: Option<String>,
    /// Expected GPS latitude/longitude.
    pub gps: Option<[f64; 2]>,
}

/// Expected scan outcome of a fixture file.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Status {
    /// Indexed and readable.
    Ok,
    /// Indexed but undecodable.
    Unreadable,
    /// Must not be indexed (hidden/system dir) or is not a media file.
    Ignored,
}

impl Manifest {
    /// Parses manifest TOML.
    pub fn parse(text: &str) -> Result<Self> {
        let m: Self = toml::from_str(text).context("parsing fixture manifest")?;
        anyhow::ensure!(m.version == 1, "unsupported manifest version {}", m.version);
        Ok(m)
    }

    /// Loads a manifest from disk.
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        Self::parse(&text)
    }

    /// Path of the committed manifest inside this repository.
    pub fn default_path() -> PathBuf {
        workspace_root().join("tests/fixtures/manifest.toml")
    }

    /// Loads the committed manifest.
    pub fn load_default() -> Result<Self> {
        Self::load(&Self::default_path())
    }

    /// Files under `prefix` (e.g. `basic/2021/beach/`) with the given status.
    pub fn files_with<'a>(
        &'a self,
        prefix: &'a str,
        status: Status,
    ) -> impl Iterator<Item = &'a FileSpec> {
        self.files
            .iter()
            .filter(move |f| f.path.starts_with(prefix) && f.expect.status == status)
    }
}

/// Root of the Cargo workspace (two levels above this crate).
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

/// Directory generated fixtures are written to by default.
pub fn default_generated_dir() -> PathBuf {
    workspace_root().join("tests/fixtures/generated")
}
