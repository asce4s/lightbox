//! Importing photos from a folder or removable volume (SPEC §6.9).

use crate::error::{AppError, Result};
use crate::events::AppEvent;
use crate::lightbox::Lightbox;
use chrono::{Datelike, NaiveDateTime};
use lb_core::PhotoId;
use lb_jobs::{Job, JobCtx, JobError, JobHandle, JobResult, Priority};
use lb_media::{DateSources, fallback_taken_at, format_for_extension};
use parking_lot::Mutex;
use std::collections::HashSet;
use std::fs::File;
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use walkdir::WalkDir;

/// Suffix of a file being copied; removed on failure and cancel.
pub const IMPORT_TEMP_SUFFIX: &str = ".lbimport-tmp";

/// How destination sub-folders are named.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum FolderNaming {
    /// `{yyyy}/{yyyy-mm-dd}` (default).
    #[default]
    Date,
    /// `{yyyy-mm-dd} {title}`.
    DateTitle(String),
}

impl FolderNaming {
    /// The folder below the destination root for a photo taken at `at`.
    pub fn folder_for(&self, at: NaiveDateTime) -> PathBuf {
        let day = at.format("%Y-%m-%d").to_string();
        match self {
            Self::Date => PathBuf::from(format!("{:04}", at.year())).join(day),
            Self::DateTitle(title) => {
                let title: String = title
                    .chars()
                    .filter(|c| {
                        !c.is_control()
                            && !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|')
                    })
                    .collect();
                let title = title.trim();
                if title.is_empty() {
                    PathBuf::from(day)
                } else {
                    PathBuf::from(format!("{day} {title}"))
                }
            }
        }
    }
}

/// Import settings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportOptions {
    /// Destination root; created when missing and added to the library when it is not under one.
    pub dest: PathBuf,
    /// Sub-folder naming.
    pub naming: FolderNaming,
    /// Skip files whose content already is in the catalog or earlier in this import.
    pub skip_duplicates: bool,
    /// Delete each source file after its copy was verified.
    pub delete_source: bool,
}

impl ImportOptions {
    /// Defaults of SPEC §6.9 for `dest`.
    pub fn new(dest: impl Into<PathBuf>) -> Self {
        Self {
            dest: dest.into(),
            naming: FolderNaming::Date,
            skip_duplicates: true,
            delete_source: false,
        }
    }

    /// Checks the settings a dialog could get wrong.
    pub fn validate(&self) -> Result<()> {
        if self.dest.as_os_str().is_empty() {
            return Err(AppError::Invalid("choose a destination folder".into()));
        }
        Ok(())
    }
}

/// A file offered for import.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportCandidate {
    /// Source file.
    pub path: PathBuf,
    /// Size in bytes.
    pub size: u64,
    /// Capture time that decides the destination folder.
    pub taken_at: NaiveDateTime,
}

/// One imported file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ImportedFile {
    /// Where it came from.
    pub source: PathBuf,
    /// The verified copy.
    pub dest: PathBuf,
    /// Catalog id after indexing.
    pub id: Option<PhotoId>,
}

/// Outcome of an import.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    /// Files copied and verified.
    pub imported: Vec<ImportedFile>,
    /// Sources skipped because their content already exists.
    pub duplicates: Vec<PathBuf>,
    /// Sources that could not be imported, with the reason.
    pub failed: Vec<(PathBuf, String)>,
    /// Sources removed after a verified copy.
    pub deleted_sources: Vec<PathBuf>,
    /// The import was cancelled before it covered every file.
    pub cancelled: bool,
}

/// Copies one file; replaceable so tests can inject faults.
pub trait FileCopier: Send + Sync {
    /// Copies `from` to `to`, polling `is_cancelled`; returns `false` when cancelled (the caller
    /// removes the partial file).
    fn copy(&self, from: &Path, to: &Path, is_cancelled: &dyn Fn() -> bool) -> io::Result<bool>;
}

/// Chunked copy with a cancel check between chunks.
#[derive(Debug, Default, Clone, Copy)]
pub struct StdCopier;

impl FileCopier for StdCopier {
    fn copy(&self, from: &Path, to: &Path, is_cancelled: &dyn Fn() -> bool) -> io::Result<bool> {
        let mut src = File::open(from)?;
        let mut dst = File::create_new(to)?;
        let mut buf = vec![0_u8; 256 * 1024];
        loop {
            if is_cancelled() {
                return Ok(false);
            }
            let n = src.read(&mut buf)?;
            if n == 0 {
                break;
            }
            dst.write_all(&buf[..n])?;
        }
        dst.sync_all()?;
        Ok(true)
    }
}

/// A running import job.
pub struct ImportHandle {
    job: JobHandle,
    report: Arc<Mutex<Option<ImportReport>>>,
}

impl ImportHandle {
    /// Asks the job to stop; the file in progress is removed, finished files stay.
    pub fn cancel(&self) {
        self.job.cancel();
    }

    /// Whether the job ended.
    pub fn is_finished(&self) -> bool {
        self.job.is_finished()
    }

    /// Waits up to `timeout` and returns the report once the job ended.
    pub fn wait(&self, timeout: Duration) -> Option<ImportReport> {
        let _ = self.job.wait(timeout)?;
        self.report.lock().clone()
    }

    /// The report, available when the job ended.
    pub fn report(&self) -> Option<ImportReport> {
        self.report.lock().clone()
    }
}

/// Importing photos.
#[derive(Clone)]
pub struct ImportService {
    lb: Lightbox,
}

impl Lightbox {
    /// Import of photos from folders and removable volumes.
    pub fn import(&self) -> ImportService {
        ImportService { lb: self.clone() }
    }
}

struct ImportJob {
    svc: ImportService,
    candidates: Vec<ImportCandidate>,
    opts: ImportOptions,
    out: Arc<Mutex<Option<ImportReport>>>,
}

impl Job for ImportJob {
    fn kind(&self) -> &'static str {
        "import"
    }

    fn run(self: Box<Self>, ctx: &JobCtx) -> JobResult {
        let lb = self.svc.lb.clone();
        let result = self.svc.run(
            &self.candidates,
            &self.opts,
            &StdCopier,
            &|| ctx.is_cancelled(),
            &|fraction, label| {
                ctx.progress(fraction);
                lb.emit(AppEvent::JobProgress {
                    id: ctx.id(),
                    kind: "import",
                    progress: fraction,
                    label: label.to_owned(),
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

enum Outcome {
    Imported(PathBuf),
    Duplicate,
    Cancelled,
}

impl ImportService {
    /// Mount points that look like camera cards or USB drives (SPEC §6.9).
    pub fn volumes(&self) -> Vec<PathBuf> {
        let mut dirs = vec![PathBuf::from("/Volumes")];
        if let Ok(user) = std::env::var("USER") {
            dirs.push(Path::new("/media").join(&user));
            dirs.push(Path::new("/run/media").join(&user));
        }
        let mut found: Vec<PathBuf> = dirs
            .iter()
            .filter_map(|d| std::fs::read_dir(d).ok())
            .flat_map(|rd| rd.filter_map(|e| e.ok().map(|e| e.path())))
            .filter(|p| p.is_dir())
            .collect();
        for letter in 'D'..='Z' {
            let drive = PathBuf::from(format!("{letter}:\\"));
            if drive.join("DCIM").is_dir() {
                found.push(drive);
            }
        }
        found.sort();
        found
    }

    /// Lists importable files below `source` (recursively, in path order) with their capture
    /// times. Nothing is copied or cached.
    pub fn discover(&self, source: &Path) -> Result<Vec<ImportCandidate>> {
        if !source.is_dir() {
            return Err(AppError::NotFound("source folder"));
        }
        let caps = self.lb.inner.folders.scanner().options().caps;
        let mut out = Vec::new();
        let walker = WalkDir::new(source).sort_by_file_name().into_iter();
        for entry in walker.filter_map(|e| e.ok()) {
            if !entry.file_type().is_file() {
                continue;
            }
            let path = entry.path();
            let Some(name) = path.file_name().and_then(|n| n.to_str()) else {
                continue;
            };
            if lb_scan::is_ignored_name(name) {
                continue;
            }
            let Some(ext) = path.extension().and_then(|e| e.to_str()) else {
                continue;
            };
            if format_for_extension(ext, caps).is_none() {
                continue;
            }
            let Ok(meta) = entry.metadata() else { continue };
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map_or(0, |d| i64::try_from(d.as_secs()).unwrap_or(0));
            let taken_at = match lb_media::extract_image(path, name, mtime) {
                Ok(m) => m.taken_at.at,
                Err(_) => fallback_taken_at(&DateSources::default(), name, mtime).at,
            };
            out.push(ImportCandidate {
                path: path.to_owned(),
                size: meta.len(),
                taken_at,
            });
        }
        Ok(out)
    }

    /// Starts an import as a foreground job.
    pub fn start(
        &self,
        candidates: Vec<ImportCandidate>,
        opts: ImportOptions,
    ) -> Result<ImportHandle> {
        opts.validate()?;
        let report = Arc::new(Mutex::new(None));
        let job = ImportJob {
            svc: self.clone(),
            candidates,
            opts,
            out: report.clone(),
        };
        let job = self.lb.jobs().submit(job, Priority::Foreground);
        Ok(ImportHandle { job, report })
    }

    /// Imports on the calling thread.
    pub fn import_now(
        &self,
        candidates: &[ImportCandidate],
        opts: &ImportOptions,
    ) -> Result<ImportReport> {
        self.run(candidates, opts, &StdCopier, &|| false, &|_, _| {})
    }

    /// Imports on the calling thread with an injectable copier. `progress` gets `(fraction,
    /// label)` after each file. Files already copied stay on cancel; the one in progress is
    /// removed. Everything copied is indexed before this returns.
    pub fn run(
        &self,
        candidates: &[ImportCandidate],
        opts: &ImportOptions,
        copier: &dyn FileCopier,
        is_cancelled: &dyn Fn() -> bool,
        progress: &dyn Fn(f32, &str),
    ) -> Result<ImportReport> {
        opts.validate()?;
        std::fs::create_dir_all(&opts.dest)?;
        let mut report = ImportReport::default();
        let mut seen: HashSet<String> = HashSet::new();
        let total = candidates.len().max(1);
        for (i, c) in candidates.iter().enumerate() {
            if is_cancelled() {
                report.cancelled = true;
                break;
            }
            match self.import_one(c, opts, copier, is_cancelled, &mut seen) {
                Ok(Outcome::Imported(dest)) => {
                    if opts.delete_source {
                        match std::fs::remove_file(&c.path) {
                            Ok(()) => report.deleted_sources.push(c.path.clone()),
                            Err(e) => report.failed.push((
                                c.path.clone(),
                                format!("copied, but the original could not be removed: {e}"),
                            )),
                        }
                    }
                    report.imported.push(ImportedFile {
                        source: c.path.clone(),
                        dest,
                        id: None,
                    });
                }
                Ok(Outcome::Duplicate) => report.duplicates.push(c.path.clone()),
                Ok(Outcome::Cancelled) => {
                    report.cancelled = true;
                    break;
                }
                Err(e) => report.failed.push((c.path.clone(), e.to_string())),
            }
            progress(
                (i + 1) as f32 / total as f32,
                &format!("Imported {} of {}", i + 1, candidates.len()),
            );
        }
        if !report.imported.is_empty() {
            self.index(opts, &mut report)?;
        }
        Ok(report)
    }

    fn import_one(
        &self,
        c: &ImportCandidate,
        opts: &ImportOptions,
        copier: &dyn FileCopier,
        is_cancelled: &dyn Fn() -> bool,
        seen: &mut HashSet<String>,
    ) -> Result<Outcome> {
        let source_hash = lb_media::content_hash(&c.path)?;
        if opts.skip_duplicates
            && (seen.contains(&source_hash) || self.in_catalog(&c.path, &source_hash)?)
        {
            return Ok(Outcome::Duplicate);
        }
        let name = c
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or_else(|| AppError::Invalid("file name is not valid text".into()))?;
        let dir = opts.dest.join(opts.naming.folder_for(c.taken_at));
        std::fs::create_dir_all(&dir)?;
        let dest = unique_destination(&dir, name);
        let temp = dir.join(format!(
            ".{}{IMPORT_TEMP_SUFFIX}",
            dest.file_name().and_then(|n| n.to_str()).unwrap_or("file")
        ));
        let _ = std::fs::remove_file(&temp);
        let copied = match copier.copy(&c.path, &temp, is_cancelled) {
            Ok(done) => done,
            Err(e) => {
                let _ = std::fs::remove_file(&temp);
                return Err(e.into());
            }
        };
        if !copied {
            let _ = std::fs::remove_file(&temp);
            return Ok(Outcome::Cancelled);
        }
        let copy_hash = lb_media::content_hash(&temp);
        if !matches!(&copy_hash, Ok(h) if *h == source_hash) {
            let _ = std::fs::remove_file(&temp);
            return Err(AppError::Invalid(
                "the copy did not match the original and was discarded".into(),
            ));
        }
        if let Err(e) = std::fs::rename(&temp, &dest) {
            let _ = std::fs::remove_file(&temp);
            return Err(e.into());
        }
        seen.insert(source_hash);
        Ok(Outcome::Imported(dest))
    }

    fn in_catalog(&self, path: &Path, hash: &str) -> Result<bool> {
        let catalog = self.lb.catalog();
        let key = lb_media::quick_key(path)?;
        for photo in catalog.photos_by_quick_key(&key)? {
            let known = match photo.content_hash {
                Some(h) => h,
                None => match lb_media::content_hash(&photo.path) {
                    Ok(h) => {
                        catalog.set_content_hash(photo.id, &h)?;
                        h
                    }
                    Err(_) => continue,
                },
            };
            if known == hash {
                return Ok(true);
            }
        }
        Ok(false)
    }

    fn index(&self, opts: &ImportOptions, report: &mut ImportReport) -> Result<()> {
        let library = self.lb.library();
        let dest = lb_scan::normalize_path(&opts.dest)?;
        let root = library
            .roots()?
            .into_iter()
            .find(|r| dest.starts_with(&r.path));
        match root {
            Some(root) => {
                library.rescan(root.id)?;
            }
            None => {
                library.add_root(&opts.dest, lb_core::RootMode::Watch)?;
            }
        }
        for file in &mut report.imported {
            file.id = match lb_scan::normalize_path(&file.dest) {
                Ok(path) => self.lb.catalog().photo_by_path(&path)?.map(|p| p.id),
                Err(_) => None,
            };
        }
        Ok(())
    }
}

fn unique_destination(dir: &Path, name: &str) -> PathBuf {
    let first = dir.join(name);
    if !first.exists() {
        return first;
    }
    let path = Path::new(name);
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or(name);
    let ext = path.extension().and_then(|e| e.to_str());
    (1u32..)
        .map(|n| match ext {
            Some(ext) => dir.join(format!("{stem} ({n}).{ext}")),
            None => dir.join(format!("{stem} ({n})")),
        })
        .find(|p| !p.exists())
        .unwrap_or(first)
}
