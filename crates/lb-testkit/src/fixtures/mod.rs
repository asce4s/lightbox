//! Deterministic fixture generation (SPEC §12.3). Driven by `cargo xtask fixtures`.

pub mod exif_writer;
pub mod manifest;
pub mod pattern;

use anyhow::{Context, Result, bail, ensure};
use image::codecs::gif::{GifEncoder, Repeat};
use image::codecs::{
    bmp::BmpEncoder, jpeg::JpegEncoder, png::PngEncoder, tiff::TiffEncoder, webp::WebPEncoder,
};
use image::{Delay, ExtendedColorType, Frame, ImageEncoder, RgbaImage};
use manifest::{FileSpec, Kind, Manifest, Pattern};
use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub use manifest::{default_generated_dir, workspace_root};

/// Marker file proving a directory was created by the generator (safe to replace).
pub const MARKER: &str = ".lb-fixtures";

fn dims(spec: &FileSpec) -> Result<(u32, u32)> {
    match (spec.width, spec.height) {
        (Some(w), Some(h)) if w > 0 && h > 0 => Ok((w, h)),
        _ => bail!("{}: width and height are required", spec.path),
    }
}

fn pixels(spec: &FileSpec) -> Result<RgbaImage> {
    let (w, h) = dims(spec)?;
    let pattern = spec.pattern.unwrap_or(Pattern::Scene);
    Ok(pattern::render(pattern, spec.seed, w, h))
}

fn rgb_bytes(img: &RgbaImage) -> Vec<u8> {
    img.pixels()
        .flat_map(|p| [p.0[0], p.0[1], p.0[2]])
        .collect()
}

fn encode_jpeg(spec: &FileSpec) -> Result<Vec<u8>> {
    let img = pixels(spec)?;
    let mut buf = Vec::new();
    JpegEncoder::new_with_quality(&mut buf, spec.quality.unwrap_or(90)).write_image(
        &rgb_bytes(&img),
        img.width(),
        img.height(),
        ExtendedColorType::Rgb8,
    )?;
    match &spec.exif {
        Some(exif) => exif_writer::insert_exif(&buf, exif),
        None => Ok(buf),
    }
}

fn encode_gif(spec: &FileSpec) -> Result<Vec<u8>> {
    let (w, h) = dims(spec)?;
    let frames = spec.frames.unwrap_or(3);
    let mut buf = Vec::new();
    {
        let mut enc = GifEncoder::new(&mut buf);
        enc.set_repeat(Repeat::Infinite)?;
        for i in 0..frames {
            let img = pattern::render(
                spec.pattern.unwrap_or(Pattern::Scene),
                spec.seed + u64::from(i),
                w,
                h,
            );
            enc.encode_frame(Frame::from_parts(
                img,
                0,
                0,
                Delay::from_numer_denom_ms(100, 1),
            ))?;
        }
    }
    Ok(buf)
}

fn encode(spec: &FileSpec) -> Result<Vec<u8>> {
    match spec.kind {
        Kind::Jpeg => encode_jpeg(spec),
        Kind::TruncatedJpeg => {
            let full = encode_jpeg(spec)?;
            Ok(full[..full.len() * 2 / 5].to_vec())
        }
        Kind::Png => {
            let img = pixels(spec)?;
            let mut buf = Vec::new();
            PngEncoder::new(&mut buf).write_image(
                img.as_raw(),
                img.width(),
                img.height(),
                ExtendedColorType::Rgba8,
            )?;
            Ok(buf)
        }
        Kind::Webp => {
            let img = pixels(spec)?;
            let mut buf = Vec::new();
            WebPEncoder::new_lossless(&mut buf).write_image(
                img.as_raw(),
                img.width(),
                img.height(),
                ExtendedColorType::Rgba8,
            )?;
            Ok(buf)
        }
        Kind::Bmp => {
            let img = pixels(spec)?;
            let mut buf = Vec::new();
            BmpEncoder::new(&mut buf).write_image(
                &rgb_bytes(&img),
                img.width(),
                img.height(),
                ExtendedColorType::Rgb8,
            )?;
            Ok(buf)
        }
        Kind::Tiff => {
            let img = pixels(spec)?;
            let mut cur = Cursor::new(Vec::new());
            TiffEncoder::new(&mut cur).write_image(
                &rgb_bytes(&img),
                img.width(),
                img.height(),
                ExtendedColorType::Rgb8,
            )?;
            Ok(cur.into_inner())
        }
        Kind::Gif => encode_gif(spec),
        Kind::ZeroByte => Ok(Vec::new()),
        Kind::Text => Ok(spec
            .contents
            .clone()
            .context("text fixture needs `contents`")?
            .into_bytes()),
        Kind::Copy => bail!("copy is resolved by the caller"),
    }
}

fn safe_relative(path: &str) -> Result<PathBuf> {
    let p = Path::new(path);
    ensure!(
        p.components()
            .all(|c| matches!(c, std::path::Component::Normal(_))),
        "manifest path {path:?} must be relative without `..`"
    );
    Ok(p.to_path_buf())
}

fn write_file(root: &Path, spec: &FileSpec, bytes: &[u8], mtime: SystemTime) -> Result<()> {
    let dest = root.join(safe_relative(&spec.path)?);
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&dest, bytes).with_context(|| format!("writing {}", dest.display()))?;
    std::fs::File::options()
        .write(true)
        .open(&dest)?
        .set_modified(mtime)?;
    Ok(())
}

/// Generates every file of `manifest` into `out_dir`, replacing a previous generation.
///
/// `out_dir` is only wiped when it is empty, missing, or carries the [`MARKER`] file.
pub fn generate(manifest: &Manifest, out_dir: &Path) -> Result<()> {
    if out_dir.exists() {
        let empty = std::fs::read_dir(out_dir)?.next().is_none();
        ensure!(
            empty || out_dir.join(MARKER).is_file(),
            "refusing to replace {}: not created by the fixture generator",
            out_dir.display()
        );
        std::fs::remove_dir_all(out_dir)?;
    }
    std::fs::create_dir_all(out_dir)?;
    let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(manifest.mtime_unix);

    let mut copies = Vec::new();
    for spec in &manifest.files {
        if spec.kind == Kind::Copy {
            copies.push(spec);
            continue;
        }
        let bytes = encode(spec).with_context(|| format!("generating {}", spec.path))?;
        write_file(out_dir, spec, &bytes, mtime)?;
    }
    for spec in copies {
        let of = spec
            .of
            .as_deref()
            .with_context(|| format!("{}: copy needs `of`", spec.path))?;
        let bytes = std::fs::read(out_dir.join(safe_relative(of)?))
            .with_context(|| format!("{}: source {of} not generated", spec.path))?;
        write_file(out_dir, spec, &bytes, mtime)?;
    }
    std::fs::write(out_dir.join(MARKER), "generated by lb-testkit fixtures\n")?;
    Ok(())
}

/// Loads the committed manifest and generates into the default directory
/// (`tests/fixtures/generated`). Returns that directory.
pub fn generate_default() -> Result<PathBuf> {
    let manifest = Manifest::load_default()?;
    let dir = default_generated_dir();
    generate(&manifest, &dir)?;
    Ok(dir)
}

/// Recursively copies `src` to `dst` (created if needed), preserving file modification times,
/// so scanner tests can mutate a private copy of the generated fixtures.
pub fn copy_tree(src: &Path, dst: &Path) -> Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src).with_context(|| format!("reading {}", src.display()))? {
        let entry = entry?;
        let (from, to) = (entry.path(), dst.join(entry.file_name()));
        if entry.file_type()?.is_dir() {
            copy_tree(&from, &to)?;
        } else {
            std::fs::copy(&from, &to)?;
            let mtime = std::fs::metadata(&from)?.modified()?;
            std::fs::File::options()
                .write(true)
                .open(&to)?
                .set_modified(mtime)?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests;
