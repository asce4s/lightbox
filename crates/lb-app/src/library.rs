//! Roots, scanning and the folder tree (SPEC §6.1–§6.3).

use crate::error::{AppError, Result};
use crate::events::AppEvent;
use crate::lightbox::Lightbox;
use lb_core::{Folder, Root, RootId, RootMode};
use lb_scan::{RootChange, ScanEvent, ScanSummary, WatchConfig, WatchService};
use std::path::Path;
use std::sync::atomic::Ordering;

pub(crate) fn scan_event_to_app(event: ScanEvent) -> AppEvent {
    match event {
        ScanEvent::Progress {
            root,
            done,
            total_estimate,
        } => AppEvent::ScanProgress {
            root,
            done,
            total_estimate,
        },
        ScanEvent::Finished { root, summary } => AppEvent::ScanFinished {
            root,
            added: summary.added,
            updated: summary.updated,
            missing: summary.missing,
            errors: summary.errors,
        },
    }
}

/// Manages roots and keeps the catalog in sync with the disk.
#[derive(Clone)]
pub struct LibraryService {
    pub(crate) lb: Lightbox,
}

impl Lightbox {
    /// Roots, scanning and folders.
    pub fn library(&self) -> LibraryService {
        LibraryService { lb: self.clone() }
    }
}

impl LibraryService {
    /// Every configured root.
    pub fn roots(&self) -> Result<Vec<Root>> {
        Ok(self.lb.catalog().list_roots()?)
    }

    /// Folders indexed under `root`.
    pub fn folders(&self, root: RootId) -> Result<Vec<Folder>> {
        Ok(self.lb.catalog().folders_in_root(root)?)
    }

    /// Adds a root (or changes the mode of an existing one) and applies it, scanning
    /// synchronously for `watch` and `once`.
    pub fn add_root(&self, path: &Path, mode: RootMode) -> Result<RootChange> {
        self.lb.inner.scan_cancel.store(false, Ordering::Relaxed);
        let now = self.lb.clock().now();
        let lb = self.lb.clone();
        let change = self.lb.inner.folders.add_root(
            path,
            mode,
            now,
            &self.lb.inner.scan_cancel,
            &mut |e| lb.emit(scan_event_to_app(e)),
        )?;
        self.announce(&change);
        Ok(change)
    }

    /// Changes a root's mode and applies it.
    pub fn set_mode(&self, root: RootId, mode: RootMode) -> Result<RootChange> {
        self.lb.inner.scan_cancel.store(false, Ordering::Relaxed);
        let lb = self.lb.clone();
        let change =
            self.lb
                .inner
                .folders
                .set_mode(root, mode, &self.lb.inner.scan_cancel, &mut |e| {
                    lb.emit(scan_event_to_app(e));
                })?;
        self.announce(&change);
        Ok(change)
    }

    /// Removes a root from the library. Files on disk are never touched.
    pub fn remove_root(&self, root: RootId) -> Result<RootChange> {
        self.lb.inner.scan_cancel.store(false, Ordering::Relaxed);
        let lb = self.lb.clone();
        let change =
            self.lb
                .inner
                .folders
                .remove_root(root, &self.lb.inner.scan_cancel, &mut |e| {
                    lb.emit(scan_event_to_app(e));
                })?;
        self.announce(&change);
        Ok(change)
    }

    /// Rescans one root.
    pub fn rescan(&self, root: RootId) -> Result<ScanSummary> {
        let target = self
            .roots()?
            .into_iter()
            .find(|r| r.id == root)
            .ok_or(AppError::NotFound("root"))?;
        self.lb.inner.scan_cancel.store(false, Ordering::Relaxed);
        let lb = self.lb.clone();
        let summary = self.lb.inner.folders.scanner().scan_root(
            &target,
            &self.lb.inner.scan_cancel,
            &mut |e| lb.emit(scan_event_to_app(e)),
        )?;
        self.lb.emit(AppEvent::FoldersChanged);
        Ok(summary)
    }

    /// Rescans every `watch` and `once` root.
    pub fn rescan_all(&self) -> Result<Vec<(RootId, ScanSummary)>> {
        let mut out = Vec::new();
        for root in self
            .roots()?
            .into_iter()
            .filter(|r| r.mode != RootMode::Exclude)
        {
            out.push((root.id, self.rescan(root.id)?));
        }
        Ok(out)
    }

    /// Asks running scans to stop at the next batch boundary.
    pub fn cancel_scans(&self) {
        self.lb.inner.scan_cancel.store(true, Ordering::Relaxed);
    }

    /// Sets or clears a folder's caption.
    pub fn set_folder_caption(
        &self,
        folder: lb_core::FolderId,
        caption: Option<String>,
    ) -> Result<()> {
        if !self.lb.catalog().set_folder_caption(folder, caption)? {
            return Err(AppError::NotFound("folder"));
        }
        self.lb.emit(AppEvent::FoldersChanged);
        Ok(())
    }

    /// Starts observing `watch` roots; changes are indexed and announced as events. Idempotent.
    pub fn start_watching(&self, config: WatchConfig) -> Result<()> {
        let mut slot = self.lb.inner.watcher.lock();
        if slot.is_some() {
            return Ok(());
        }
        let lb = self.lb.clone();
        let service = WatchService::start(self.lb.inner.folders.clone(), config, move |e| {
            lb.emit(scan_event_to_app(e));
            lb.emit(AppEvent::FoldersChanged);
        })?;
        *slot = Some(service);
        Ok(())
    }

    /// Stops observing the file system.
    pub fn stop_watching(&self) {
        self.lb.inner.watcher.lock().take();
    }

    /// Re-synchronises the watcher with the configured roots after a root change.
    pub fn sync_watcher(&self) -> Result<()> {
        if let Some(w) = self.lb.inner.watcher.lock().as_ref() {
            w.sync_roots()?;
        }
        Ok(())
    }

    fn announce(&self, change: &RootChange) {
        if !change.removed.is_empty() {
            self.lb.emit(AppEvent::PhotosRemoved {
                ids: change.removed.clone(),
            });
        }
        self.lb.emit(AppEvent::FoldersChanged);
        if let Err(e) = self.sync_watcher() {
            tracing::warn!(error = %e, "could not resync file watcher");
        }
    }
}
