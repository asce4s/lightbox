//! The scanner: walk a root, index new and changed files, mark vanished ones missing
//! (SPEC §6.2.1).

use crate::Result;
use crate::rules::{has_hidden_attribute, is_ignored_name, is_under_any};
use chrono::NaiveDateTime;
use lb_catalog::{Catalog, IndexedFile, NewFolder, PhotoUpsert};
use lb_core::{FolderId, Format, GeoPoint, PhotoId, PhotoStatus, Root, RootId, RootMode};
use lb_media::{Capabilities, DateSources, ImageMetadata, MediaError};
use rayon::prelude::*;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant, UNIX_EPOCH};
use walkdir::WalkDir;

/// Rows per catalog transaction (SPEC §6.2.1 step 4).
pub const DEFAULT_BATCH: usize = 500;

/// Scanner configuration.
#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// Which optional format families are indexed.
    pub caps: Capabilities,
    /// Rows per upsert transaction.
    pub batch_size: usize,
    /// Minimum time between [`ScanEvent::Progress`] events (SPEC §6.2.1: 100 ms).
    pub progress_interval: Duration,
    /// Directories that are never scanned (Lightbox's own data and cache directories).
    pub ignored_dirs: Vec<PathBuf>,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            caps: Capabilities::default(),
            batch_size: DEFAULT_BATCH,
            progress_interval: Duration::from_millis(100),
            ignored_dirs: Vec::new(),
        }
    }
}

/// What a scan changed.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ScanSummary {
    /// Rows inserted.
    pub added: usize,
    /// Existing rows whose file changed.
    pub updated: usize,
    /// Files skipped because size and mtime matched.
    pub unchanged: usize,
    /// Rows newly marked missing.
    pub missing: usize,
    /// Rows that could not be read (unreadable files, walk errors).
    pub errors: usize,
    /// Rows relinked to a moved or renamed file (SPEC §6.3.2).
    pub moved: usize,
    /// Whether the scan stopped early because of cancellation.
    pub cancelled: bool,
}

/// Progress notifications of a running scan.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScanEvent {
    /// `done` of `total_estimate` files processed (SPEC §7.3 `ScanProgress`).
    Progress {
        /// Root being scanned.
        root: RootId,
        /// Files processed so far.
        done: usize,
        /// Files found by the walk.
        total_estimate: usize,
    },
    /// The scan ended (SPEC §7.3 `ScanFinished`).
    Finished {
        /// Root that was scanned.
        root: RootId,
        /// Outcome.
        summary: ScanSummary,
    },
}

/// A supported file found on disk.
#[derive(Debug, Clone)]
pub(crate) struct Candidate {
    pub path: PathBuf,
    pub dir: PathBuf,
    pub file_name: String,
    pub format: Format,
    pub size: u64,
    pub mtime: i64,
}

type Clock = Arc<dyn Fn() -> NaiveDateTime + Send + Sync>;

/// Indexes folders into the catalog.
#[derive(Clone)]
pub struct Scanner {
    pub(crate) catalog: Catalog,
    pub(crate) opts: ScanOptions,
    now: Clock,
}

pub(crate) fn mtime_unix(meta: &std::fs::Metadata) -> i64 {
    match meta.modified().map(|m| m.duration_since(UNIX_EPOCH)) {
        Ok(Ok(d)) => i64::try_from(d.as_secs()).unwrap_or(i64::MAX),
        Ok(Err(e)) => -i64::try_from(e.duration().as_secs()).unwrap_or(i64::MAX),
        Err(_) => 0,
    }
}

impl Scanner {
    /// A scanner writing to `catalog`, timestamping rows with the local clock.
    pub fn new(catalog: Catalog, opts: ScanOptions) -> Self {
        Self::with_clock(catalog, opts, || chrono::Local::now().naive_local())
    }

    /// A scanner with an injected clock (for deterministic `indexed_at`).
    pub fn with_clock(
        catalog: Catalog,
        opts: ScanOptions,
        now: impl Fn() -> NaiveDateTime + Send + Sync + 'static,
    ) -> Self {
        Self {
            catalog,
            opts,
            now: Arc::new(now),
        }
    }

    /// The catalog this scanner writes to.
    pub fn catalog(&self) -> &Catalog {
        &self.catalog
    }

    /// The scanner configuration.
    pub fn options(&self) -> &ScanOptions {
        &self.opts
    }

    pub(crate) fn now(&self) -> NaiveDateTime {
        (self.now)()
    }

    /// Candidate for a single path, or `None` when the name is ignored, the extension is not
    /// indexed, or the path is not a regular file.
    pub(crate) fn candidate_for(&self, path: &Path) -> Option<Candidate> {
        let file_name = path.file_name()?.to_str()?.to_owned();
        if is_ignored_name(&file_name) {
            return None;
        }
        let format = lb_media::format_for_extension(path.extension()?.to_str()?, self.opts.caps)?;
        let meta = std::fs::metadata(path).ok()?;
        if !meta.is_file() || has_hidden_attribute(&meta) {
            return None;
        }
        Some(Candidate {
            path: path.to_owned(),
            dir: path.parent()?.to_owned(),
            file_name,
            format,
            size: meta.len(),
            mtime: mtime_unix(&meta),
        })
    }

    fn walk(&self, root: &Root, other_roots: &HashSet<PathBuf>) -> (Vec<Candidate>, usize) {
        let mut found = Vec::new();
        let mut errors = 0;
        let ignored = &self.opts.ignored_dirs;
        let walker = WalkDir::new(&root.path)
            .follow_links(true)
            .sort_by_file_name()
            .into_iter()
            .filter_entry(|e| {
                if e.depth() == 0 {
                    return true;
                }
                let Some(name) = e.file_name().to_str() else {
                    return false;
                };
                if is_ignored_name(name) {
                    return false;
                }
                if e.file_type().is_dir() {
                    return !other_roots.contains(e.path()) && !is_under_any(e.path(), ignored);
                }
                true
            });
        for entry in walker {
            let entry = match entry {
                Ok(e) => e,
                Err(e) => {
                    tracing::warn!(error = %e, "skipping unreadable path during scan");
                    errors += 1;
                    continue;
                }
            };
            if entry.depth() == 0 || !entry.file_type().is_file() {
                continue;
            }
            if let Some(c) = self.candidate_for(entry.path()) {
                found.push(c);
            }
        }
        (found, errors)
    }

    /// Scans `root` (SPEC §6.2.1): walks it, indexes new and changed files in batches, then
    /// marks rows whose file vanished as missing (after relinking moved files, §6.3.2).
    /// Roots in `exclude` mode index nothing. Nested roots are left to their own scan.
    pub fn scan_root(
        &self,
        root: &Root,
        cancel: &AtomicBool,
        on_event: &mut dyn FnMut(ScanEvent),
    ) -> Result<ScanSummary> {
        let mut summary = ScanSummary::default();
        if root.mode == RootMode::Exclude {
            on_event(ScanEvent::Finished {
                root: root.id,
                summary,
            });
            return Ok(summary);
        }
        let other_roots: HashSet<PathBuf> = self
            .catalog
            .list_roots()?
            .into_iter()
            .filter(|r| r.id != root.id)
            .map(|r| r.path)
            .collect();
        let (candidates, walk_errors) = self.walk(root, &other_roots);
        summary.errors += walk_errors;

        let existing: HashMap<PathBuf, IndexedFile> = self
            .catalog
            .indexed_files_under(&root.path)?
            .into_iter()
            .filter(|f| f.root_id == root.id)
            .map(|f| (f.path.clone(), f))
            .collect();

        let total = candidates.len();
        let mut done = 0;
        let mut last_progress = Instant::now();
        let mut folders: HashMap<PathBuf, FolderId> = HashMap::new();
        let mut seen: HashSet<PathBuf> = HashSet::with_capacity(total);
        let mut changed: Vec<Candidate> = Vec::new();
        for c in candidates {
            seen.insert(c.path.clone());
            let unchanged = existing.get(&c.path).is_some_and(|e| {
                e.size == c.size && e.mtime == c.mtime && e.status != PhotoStatus::Missing
            });
            if unchanged {
                summary.unchanged += 1;
                done += 1;
            } else {
                changed.push(c);
            }
        }
        let progress =
            |done: usize, last: &mut Instant, on_event: &mut dyn FnMut(ScanEvent), force: bool| {
                if force || last.elapsed() >= self.opts.progress_interval {
                    *last = Instant::now();
                    on_event(ScanEvent::Progress {
                        root: root.id,
                        done,
                        total_estimate: total,
                    });
                }
            };
        progress(done, &mut last_progress, on_event, false);

        let mut new_files: Vec<(Candidate, PhotoId)> = Vec::new();
        for batch in changed.chunks(self.opts.batch_size.max(1)) {
            if cancel.load(Ordering::Relaxed) {
                summary.cancelled = true;
                break;
            }
            let (outcome, inserted) = self.index_batch(root, batch, &mut folders)?;
            summary.added += outcome.added;
            summary.updated += outcome.updated;
            summary.errors += outcome.errors;
            new_files.extend(inserted);
            done += batch.len();
            progress(done, &mut last_progress, on_event, false);
        }

        summary.cancelled |= cancel.load(Ordering::Relaxed);
        if !summary.cancelled {
            let vanished: Vec<&IndexedFile> = existing
                .values()
                .filter(|f| f.status != PhotoStatus::Missing && !seen.contains(&f.path))
                .collect();
            let mut vanished_ids: Vec<PhotoId> = vanished.iter().map(|f| f.id).collect();
            vanished_ids.sort();
            summary.missing += self.catalog.set_photos_status(
                &vanished_ids,
                PhotoStatus::Missing,
                Some("file not found"),
            )?;
        }
        self.catalog.refresh_folder_sort_dates(root.id)?;
        progress(done, &mut last_progress, on_event, true);
        on_event(ScanEvent::Finished {
            root: root.id,
            summary,
        });
        Ok(summary)
    }

    fn ensure_folder(
        &self,
        root: &Root,
        dir: &Path,
        cache: &mut HashMap<PathBuf, FolderId>,
    ) -> Result<FolderId> {
        if let Some(id) = cache.get(dir) {
            return Ok(*id);
        }
        let parent_id = if dir == root.path {
            None
        } else {
            match dir.parent() {
                Some(parent) if parent.starts_with(&root.path) => {
                    Some(self.ensure_folder(root, parent, cache)?)
                }
                _ => None,
            }
        };
        let folder = self.catalog.ensure_folder(NewFolder {
            root_id: root.id,
            path: dir.to_owned(),
            parent_id,
        })?;
        cache.insert(dir.to_owned(), folder.id);
        Ok(folder.id)
    }

    /// Extracts metadata for `batch` in parallel and upserts it in one transaction.
    pub(crate) fn index_batch(
        &self,
        root: &Root,
        batch: &[Candidate],
        folders: &mut HashMap<PathBuf, FolderId>,
    ) -> Result<(BatchOutcome, Vec<(Candidate, PhotoId)>)> {
        let extracted: Vec<_> = batch
            .par_iter()
            .map(|c| (lb_media::quick_key(&c.path), extract(c)))
            .collect();
        let mut rows = Vec::with_capacity(batch.len());
        let mut kept = Vec::with_capacity(batch.len());
        let mut outcome = BatchOutcome::default();
        for (cand, (key, meta)) in batch.iter().zip(extracted) {
            let key = match key {
                Ok(k) => k,
                Err(e) => {
                    tracing::warn!(path = %cand.path.display(), error = %e, "cannot read file");
                    outcome.errors += 1;
                    continue;
                }
            };
            let folder_id = self.ensure_folder(root, &cand.dir, folders)?;
            let row = build_row(folder_id, cand, key, meta);
            if row.status == PhotoStatus::Unreadable {
                outcome.errors += 1;
            }
            rows.push(row);
            kept.push(cand.clone());
        }
        let results = self.catalog.upsert_photos(rows, self.now())?;
        let mut inserted = Vec::new();
        for (cand, res) in kept.into_iter().zip(results) {
            if res.inserted {
                outcome.added += 1;
                inserted.push((cand, res.id));
            } else {
                outcome.updated += 1;
            }
        }
        Ok((outcome, inserted))
    }
}

#[derive(Debug, Default, Clone, Copy)]
pub(crate) struct BatchOutcome {
    pub added: usize,
    pub updated: usize,
    pub errors: usize,
}

fn extract(c: &Candidate) -> Result<ImageMetadata, MediaError> {
    if c.format.media_type() == lb_core::MediaType::Video {
        return Err(MediaError::Unsupported("video metadata".into()));
    }
    lb_media::extract_image(&c.path, &c.file_name, c.mtime)
}

fn build_row(
    folder_id: FolderId,
    c: &Candidate,
    quick_key: String,
    meta: Result<ImageMetadata, MediaError>,
) -> PhotoUpsert {
    let mut row = PhotoUpsert::new(
        folder_id,
        c.file_name.clone(),
        c.format,
        c.size,
        c.mtime,
        quick_key,
    );
    match meta {
        Ok(m) => {
            row.format = m.probe.format;
            row.media_type = m.probe.format.media_type();
            row.width = Some(m.probe.width);
            row.height = Some(m.probe.height);
            row.orientation = m.probe.orientation;
            row.taken_at = Some(m.taken_at.at);
            row.taken_at_src = Some(m.taken_at.source);
            row.camera_make = m.exif.make;
            row.camera_model = m.exif.model;
            row.lens = m.exif.lens;
            row.iso = m.exif.iso;
            row.f_number = m.exif.f_number;
            row.exposure_s = m.exif.exposure_s;
            row.focal_mm = m.exif.focal_mm;
            row.gps = m.exif.gps.map(|g| GeoPoint {
                lat: g.lat,
                lon: g.lon,
            });
            row.gps_alt = m.exif.gps_alt;
            row.import_caption = m.exif.description;
        }
        Err(e) => {
            let taken = lb_media::fallback_taken_at(&DateSources::default(), &c.file_name, c.mtime);
            row.taken_at = Some(taken.at);
            row.taken_at_src = Some(taken.source);
            row.status = PhotoStatus::Unreadable;
            row.error = Some(e.to_string());
        }
    }
    row
}
