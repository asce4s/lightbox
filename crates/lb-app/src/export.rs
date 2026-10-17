//! Exporting photos to a folder (SPEC §6.10).

use crate::error::{AppError, Result};
use crate::events::AppEvent;
use crate::lightbox::Lightbox;
use image::codecs::jpeg::JpegEncoder;
use image::{DynamicImage, ExtendedColorType, ImageEncoder, RgbaImage};
use lb_core::{EditStack, Format, MediaType, Photo, PhotoId, PhotoStatus};
use lb_imaging::convert::to_rgba8;
use lb_imaging::{RenderCtx, RenderTarget, render_cancellable, text::draw_watermark};
use lb_jobs::{Job, JobCtx, JobError, JobHandle, JobResult, Priority};
use lb_media::{ExifRewrite, fit_within, resize_exact};
use parking_lot::Mutex;
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// Output size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportSize {
    /// Full resolution.
    Original,
    /// Scale down so the long edge is at most this many pixels (never upscales).
    LongEdge(u32),
}

/// Output file format.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportFormat {
    /// JPEG.
    Jpeg,
    /// PNG (no metadata is embedded).
    Png,
    /// An untouched JPEG keeps its pixels byte for byte; everything else is written as PNG when
    /// the source is PNG and as JPEG otherwise.
    KeepOriginal,
}

/// Which metadata the output carries.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetadataPolicy {
    /// EXIF without the GPS block (default).
    KeepExifNoGps,
    /// All EXIF.
    KeepAll,
    /// No EXIF.
    StripAll,
}

/// What to do when the destination file exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CollisionPolicy {
    /// Write ` (1)`, ` (2)`, ... before the extension.
    Rename,
    /// Replace the existing file.
    Overwrite,
    /// Leave the existing file and skip the photo.
    Skip,
}

/// Export settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportOptions {
    /// Destination folder; created when missing.
    pub dest: PathBuf,
    /// Output size.
    pub size: ExportSize,
    /// JPEG quality 1 to 100.
    pub quality: u8,
    /// Output format.
    pub format: ExportFormat,
    /// Bake the edit stack into the output.
    pub apply_edits: bool,
    /// Metadata handling.
    pub metadata: MetadataPolicy,
    /// Watermark text, bottom-right.
    pub watermark: Option<String>,
    /// Collision handling.
    pub collision: CollisionPolicy,
}

impl ExportOptions {
    /// Defaults of SPEC §6.10 for `dest`.
    pub fn new(dest: impl Into<PathBuf>) -> Self {
        Self {
            dest: dest.into(),
            size: ExportSize::Original,
            quality: 85,
            format: ExportFormat::Jpeg,
            apply_edits: true,
            metadata: MetadataPolicy::KeepExifNoGps,
            watermark: None,
            collision: CollisionPolicy::Rename,
        }
    }

    /// Checks the settings a dialog could get wrong.
    pub fn validate(&self) -> Result<()> {
        if !(1..=100).contains(&self.quality) {
            return Err(AppError::Invalid("JPEG quality must be 1 to 100".into()));
        }
        if self.size == ExportSize::LongEdge(0) {
            return Err(AppError::Invalid("size must be at least 1 pixel".into()));
        }
        if self.dest.as_os_str().is_empty() {
            return Err(AppError::Invalid("choose a destination folder".into()));
        }
        Ok(())
    }
}

/// One exported file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExportedFile {
    /// Source photo.
    pub id: PhotoId,
    /// Written file.
    pub path: PathBuf,
}

/// Outcome of an export.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ExportReport {
    /// Files written.
    pub written: Vec<ExportedFile>,
    /// Photos skipped because the destination existed.
    pub skipped: Vec<PhotoId>,
    /// Photos that could not be exported, with the reason.
    pub failed: Vec<(PhotoId, String)>,
    /// The export was cancelled before it covered every photo.
    pub cancelled: bool,
}

/// A running export job.
pub struct ExportHandle {
    job: JobHandle,
    report: Arc<Mutex<Option<ExportReport>>>,
}

impl ExportHandle {
    /// Asks the job to stop; files already finished stay, the file in progress is removed.
    pub fn cancel(&self) {
        self.job.cancel();
    }

    /// Whether the job ended.
    pub fn is_finished(&self) -> bool {
        self.job.is_finished()
    }

    /// Waits up to `timeout` and returns the report once the job ended.
    pub fn wait(&self, timeout: Duration) -> Option<ExportReport> {
        let _ = self.job.wait(timeout)?;
        self.report.lock().clone()
    }

    /// The report so far, available when the job ended.
    pub fn report(&self) -> Option<ExportReport> {
        self.report.lock().clone()
    }
}

/// Exports photos.
#[derive(Clone)]
pub struct ExportService {
    lb: Lightbox,
}

impl Lightbox {
    /// Export of photos to a folder.
    pub fn export(&self) -> ExportService {
        ExportService { lb: self.clone() }
    }
}

struct ExportJob {
    svc: ExportService,
    ids: Vec<PhotoId>,
    opts: ExportOptions,
    out: Arc<Mutex<Option<ExportReport>>>,
}

impl Job for ExportJob {
    fn kind(&self) -> &'static str {
        "export"
    }

    fn run(self: Box<Self>, ctx: &JobCtx) -> JobResult {
        let lb = self.svc.lb.clone();
        let total = self.ids.len();
        let result = self.svc.run(
            &self.ids,
            &self.opts,
            &|| ctx.is_cancelled(),
            &|done, label| {
                ctx.progress(done);
                lb.emit(AppEvent::JobProgress {
                    id: ctx.id(),
                    kind: "export",
                    progress: done,
                    label: format!("{label} ({total} photos)"),
                });
            },
        );
        let outcome = match result {
            Ok(report) => {
                let cancelled = report.cancelled;
                *self.out.lock() = Some(report);
                if cancelled {
                    Err(JobError::Cancelled)
                } else {
                    Ok(())
                }
            }
            Err(e) => Err(JobError::Failed(e.to_string())),
        };
        lb.emit(AppEvent::JobFinished {
            id: ctx.id(),
            result: outcome.clone().map_err(|e| e.to_string()),
        });
        outcome
    }
}

impl ExportService {
    /// Starts an export as a foreground job.
    pub fn start(&self, ids: &[PhotoId], opts: ExportOptions) -> Result<ExportHandle> {
        opts.validate()?;
        let report = Arc::new(Mutex::new(None));
        let job = ExportJob {
            svc: self.clone(),
            ids: ids.to_vec(),
            opts,
            out: report.clone(),
        };
        let handle = self.lb.jobs().submit(job, Priority::Foreground);
        Ok(ExportHandle {
            job: handle,
            report,
        })
    }

    /// Exports on the calling thread.
    pub fn export_now(&self, ids: &[PhotoId], opts: &ExportOptions) -> Result<ExportReport> {
        self.run(ids, opts, &|| false, &|_, _| {})
    }

    /// Exports on the calling thread, polling `is_cancelled` before each stage of each photo and
    /// reporting `(fraction, label)` after each photo.
    pub fn run(
        &self,
        ids: &[PhotoId],
        opts: &ExportOptions,
        is_cancelled: &dyn Fn() -> bool,
        progress: &dyn Fn(f32, &str),
    ) -> Result<ExportReport> {
        opts.validate()?;
        std::fs::create_dir_all(&opts.dest)?;
        let mut report = ExportReport::default();
        let mut claimed: HashSet<PathBuf> = HashSet::new();
        let total = ids.len().max(1);
        for (i, id) in ids.iter().enumerate() {
            if is_cancelled() {
                report.cancelled = true;
                break;
            }
            match self.export_one(*id, opts, &mut claimed, is_cancelled) {
                Ok(Outcome::Written(path)) => report.written.push(ExportedFile { id: *id, path }),
                Ok(Outcome::Skipped) => report.skipped.push(*id),
                Err(AppError::Cancelled) => {
                    report.cancelled = true;
                    break;
                }
                Err(e) => report.failed.push((*id, e.to_string())),
            }
            progress(
                (i + 1) as f32 / total as f32,
                &format!("Exported {} of {}", i + 1, ids.len()),
            );
        }
        Ok(report)
    }
}

enum Outcome {
    Written(PathBuf),
    Skipped,
}

fn is_jpeg(photo: &Photo) -> bool {
    photo.format == Format::Jpeg
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Effective {
    Jpeg,
    Png,
    Original,
}

fn effective_format(format: ExportFormat, photo: &Photo, untouched: bool) -> Effective {
    match format {
        ExportFormat::Jpeg => Effective::Jpeg,
        ExportFormat::Png => Effective::Png,
        ExportFormat::KeepOriginal if untouched && is_jpeg(photo) => Effective::Original,
        ExportFormat::KeepOriginal if photo.format == Format::Png => Effective::Png,
        ExportFormat::KeepOriginal => Effective::Jpeg,
    }
}

fn destination(
    dest: &Path,
    stem: &str,
    ext: &str,
    policy: CollisionPolicy,
    claimed: &HashSet<PathBuf>,
) -> Option<PathBuf> {
    let first = dest.join(format!("{stem}.{ext}"));
    let taken_here = claimed.contains(&first);
    if !taken_here && !first.exists() {
        return Some(first);
    }
    match policy {
        CollisionPolicy::Skip if !taken_here => None,
        CollisionPolicy::Overwrite if !taken_here => Some(first),
        _ => (1u32..).find_map(|n| {
            let p = dest.join(format!("{stem} ({n}).{ext}"));
            (!p.exists() && !claimed.contains(&p)).then_some(p)
        }),
    }
}

fn temp_path(final_path: &Path) -> PathBuf {
    let name = final_path
        .file_name()
        .map_or_else(String::new, |n| n.to_string_lossy().into_owned());
    final_path.with_file_name(format!(".{name}.lbtmp"))
}

struct TempGuard(Option<PathBuf>);

impl TempGuard {
    fn commit(&mut self) {
        self.0 = None;
    }
}

impl Drop for TempGuard {
    fn drop(&mut self) {
        if let Some(p) = self.0.take() {
            let _ = std::fs::remove_file(p);
        }
    }
}

fn encode_jpeg(img: &DynamicImage, quality: u8) -> Result<Vec<u8>> {
    let rgb = img.to_rgb8();
    let mut out = Vec::new();
    JpegEncoder::new_with_quality(&mut out, quality)
        .encode(
            rgb.as_raw(),
            rgb.width(),
            rgb.height(),
            ExtendedColorType::Rgb8,
        )
        .map_err(|e| AppError::Invalid(format!("JPEG encoding failed: {e}")))?;
    Ok(out)
}

fn encode_png(img: &RgbaImage) -> Result<Vec<u8>> {
    let mut out = Vec::new();
    image::codecs::png::PngEncoder::new(&mut out)
        .write_image(
            img.as_raw(),
            img.width(),
            img.height(),
            ExtendedColorType::Rgba8,
        )
        .map_err(|e| AppError::Invalid(format!("PNG encoding failed: {e}")))?;
    Ok(out)
}

impl ExportService {
    fn export_one(
        &self,
        id: PhotoId,
        opts: &ExportOptions,
        claimed: &mut HashSet<PathBuf>,
        is_cancelled: &dyn Fn() -> bool,
    ) -> Result<Outcome> {
        let photo = self
            .lb
            .catalog()
            .get_photo(id)?
            .ok_or(AppError::NotFound("photo"))?;
        if photo.media_type == MediaType::Video {
            return Err(AppError::Invalid("videos cannot be exported yet".into()));
        }
        if photo.status != PhotoStatus::Ok {
            return Err(AppError::Invalid(
                "the file is missing or unreadable".into(),
            ));
        }
        let stack = if opts.apply_edits {
            self.lb.edits().stack(id)?
        } else {
            EditStack::default()
        };
        let watermark = opts.watermark.as_deref().filter(|w| !w.trim().is_empty());
        let untouched =
            stack.is_empty() && opts.size == ExportSize::Original && watermark.is_none();
        let effective = effective_format(opts.format, &photo, untouched);
        let ext = match effective {
            Effective::Png => "png".to_owned(),
            Effective::Jpeg => "jpg".to_owned(),
            Effective::Original => photo
                .path
                .extension()
                .map_or_else(|| "jpg".to_owned(), |e| e.to_string_lossy().into_owned()),
        };
        let stem = photo
            .path
            .file_stem()
            .map_or_else(|| "photo".to_owned(), |s| s.to_string_lossy().into_owned());
        let Some(target) = destination(&opts.dest, &stem, &ext, opts.collision, claimed) else {
            return Ok(Outcome::Skipped);
        };
        if is_cancelled() {
            return Err(AppError::Cancelled);
        }
        let bytes = match effective {
            Effective::Original => original_bytes(&photo, opts.metadata)?,
            Effective::Jpeg | Effective::Png => {
                self.render_encode(&photo, &stack, opts, watermark, effective == Effective::Png)?
            }
        };
        if is_cancelled() {
            return Err(AppError::Cancelled);
        }
        let tmp = temp_path(&target);
        let mut guard = TempGuard(Some(tmp.clone()));
        std::fs::write(&tmp, &bytes)?;
        if is_cancelled() {
            return Err(AppError::Cancelled);
        }
        if target.exists() {
            std::fs::remove_file(&target)?;
        }
        std::fs::rename(&tmp, &target)?;
        guard.commit();
        claimed.insert(target.clone());
        Ok(Outcome::Written(target))
    }

    fn render_encode(
        &self,
        photo: &Photo,
        stack: &EditStack,
        opts: &ExportOptions,
        watermark: Option<&str>,
        png: bool,
    ) -> Result<Vec<u8>> {
        let long = match opts.size {
            ExportSize::LongEdge(n) => Some(n),
            ExportSize::Original => None,
        };
        let mut img = if stack.is_empty() {
            lb_media::decode(&photo.path, long)?
        } else {
            let src = lb_media::decode(&photo.path, None)?;
            let ctx = RenderCtx::new(&photo.quick_key, RenderTarget::Full);
            let out = render_cancellable(&src, stack, &ctx, &AtomicBool::new(false))
                .map_err(|_| AppError::Cancelled)?;
            DynamicImage::ImageRgba8(to_rgba8(&out))
        };
        if let Some(n) = long {
            let (w, h) = fit_within(img.width(), img.height(), n);
            if (w, h) != (img.width(), img.height()) {
                img = resize_exact(&img, w, h);
            }
        }
        let mut rgba = img.to_rgba8();
        if let Some(text) = watermark {
            draw_watermark(&mut rgba, text);
        }
        if png {
            return encode_png(&rgba);
        }
        let jpeg = encode_jpeg(&DynamicImage::ImageRgba8(rgba.clone()), opts.quality)?;
        if opts.metadata == MetadataPolicy::StripAll {
            return Ok(jpeg);
        }
        let Some(tiff) = lb_media::raw_exif(&photo.path) else {
            return Ok(jpeg);
        };
        let rewrite = ExifRewrite {
            strip_gps: opts.metadata == MetadataPolicy::KeepExifNoGps,
            orientation: Some(1),
            dimensions: Some((rgba.width(), rgba.height())),
            drop_thumbnail: true,
        };
        Ok(lb_media::rewrite_exif(&tiff, &rewrite)
            .and_then(|t| lb_media::embed_exif(&jpeg, &t))
            .unwrap_or(jpeg))
    }
}

/// An untouched JPEG with its pixels preserved and metadata handled per `policy`.
fn original_bytes(photo: &Photo, policy: MetadataPolicy) -> Result<Vec<u8>> {
    let bytes = std::fs::read(&photo.path)?;
    let rewritten = match policy {
        MetadataPolicy::KeepAll => None,
        MetadataPolicy::StripAll => lb_media::map_jpeg_exif(&bytes, |_| None),
        MetadataPolicy::KeepExifNoGps => lb_media::map_jpeg_exif(&bytes, |tiff| {
            lb_media::rewrite_exif(
                tiff,
                &ExifRewrite {
                    strip_gps: true,
                    ..ExifRewrite::default()
                },
            )
        }),
    };
    Ok(rewritten.unwrap_or(bytes))
}
