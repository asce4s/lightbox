//! The Folder Manager: roots with `watch`, `once` or `exclude` modes (SPEC §6.1).

use crate::{Result, ScanError, ScanEvent, ScanSummary, Scanner, normalize_path};
use chrono::NaiveDateTime;
use lb_core::{PhotoId, Root, RootId, RootMode};
use std::path::Path;
use std::sync::atomic::AtomicBool;

/// The most specific root containing `path` (a nested root overrides its ancestor).
pub fn effective_root<'a>(roots: &'a [Root], path: &Path) -> Option<&'a Root> {
    roots
        .iter()
        .filter(|r| path.starts_with(&r.path))
        .max_by_key(|r| r.path.components().count())
}

/// What a Folder Manager change did.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RootChange {
    /// The root that was added or changed (`None` after removal).
    pub root: Option<Root>,
    /// Result of the scan the change triggered, if any.
    pub scan: Option<ScanSummary>,
    /// Photos whose catalog rows were deleted (files are never touched).
    pub removed: Vec<PhotoId>,
}

/// Manages roots and keeps the catalog consistent with their modes.
#[derive(Clone)]
pub struct FolderManager {
    scanner: Scanner,
}

impl FolderManager {
    /// A manager that scans with `scanner` and stores roots in its catalog.
    pub fn new(scanner: Scanner) -> Self {
        Self { scanner }
    }

    /// The scanner used for scans.
    pub fn scanner(&self) -> &Scanner {
        &self.scanner
    }

    /// All roots ordered by path.
    pub fn roots(&self) -> Result<Vec<Root>> {
        Ok(self.scanner.catalog().list_roots()?)
    }

    /// Roots in `watch` mode: the ones the watcher must observe. `once` and `exclude` roots are
    /// never watched.
    pub fn watched_roots(&self) -> Result<Vec<Root>> {
        Ok(self
            .roots()?
            .into_iter()
            .filter(|r| r.mode == RootMode::Watch)
            .collect())
    }

    /// The root that governs `path`, if any.
    pub fn effective_root(&self, path: &Path) -> Result<Option<Root>> {
        let roots = self.roots()?;
        Ok(effective_root(&roots, path).cloned())
    }

    /// Adds `path` as a root with `mode` (or changes the mode of an existing root) and applies
    /// it: `watch`/`once` scan now, `exclude` removes the catalog rows under the path.
    pub fn add_root(
        &self,
        path: &Path,
        mode: RootMode,
        now: NaiveDateTime,
        cancel: &AtomicBool,
        on_event: &mut dyn FnMut(ScanEvent),
    ) -> Result<RootChange> {
        let normalized = normalize_path(path)?;
        let root = self.scanner.catalog().add_root(&normalized, mode, now)?;
        self.apply(root, cancel, on_event)
    }

    /// Changes a root's mode and applies it (see [`FolderManager::add_root`]).
    pub fn set_mode(
        &self,
        id: RootId,
        mode: RootMode,
        cancel: &AtomicBool,
        on_event: &mut dyn FnMut(ScanEvent),
    ) -> Result<RootChange> {
        let catalog = self.scanner.catalog();
        if !catalog.set_root_mode(id, mode)? {
            return Err(ScanError::RootNotFound);
        }
        let root = catalog
            .list_roots()?
            .into_iter()
            .find(|r| r.id == id)
            .ok_or(ScanError::RootNotFound)?;
        self.apply(root, cancel, on_event)
    }

    /// Removes a root and its catalog rows; the enclosing root, if any, rescans so photos in
    /// the freed directory stay indexed under it.
    pub fn remove_root(
        &self,
        id: RootId,
        cancel: &AtomicBool,
        on_event: &mut dyn FnMut(ScanEvent),
    ) -> Result<RootChange> {
        let catalog = self.scanner.catalog();
        let roots = catalog.list_roots()?;
        let root = roots
            .iter()
            .find(|r| r.id == id)
            .cloned()
            .ok_or(ScanError::RootNotFound)?;
        let removed: Vec<PhotoId> = catalog
            .indexed_files_under(&root.path)?
            .into_iter()
            .filter(|f| f.root_id == id)
            .map(|f| f.id)
            .collect();
        catalog.remove_root(id)?;
        let mut change = RootChange {
            removed,
            ..RootChange::default()
        };
        let parent = roots
            .iter()
            .filter(|r| r.id != id && root.path.starts_with(&r.path))
            .max_by_key(|r| r.path.components().count());
        if let Some(parent) = parent.filter(|p| p.mode != RootMode::Exclude) {
            change.scan = Some(self.scanner.scan_root(parent, cancel, on_event)?);
        }
        Ok(change)
    }

    fn apply(
        &self,
        root: Root,
        cancel: &AtomicBool,
        on_event: &mut dyn FnMut(ScanEvent),
    ) -> Result<RootChange> {
        let mut change = RootChange::default();
        match root.mode {
            RootMode::Watch | RootMode::Once => {
                change.scan = Some(self.scanner.scan_root(&root, cancel, on_event)?);
            }
            RootMode::Exclude => {
                let catalog = self.scanner.catalog();
                change.removed = catalog.remove_folder_tree(&root.path)?;
                for nested in catalog
                    .list_roots()?
                    .into_iter()
                    .filter(|r| r.id != root.id && r.path.starts_with(&root.path))
                    .filter(|r| r.mode != RootMode::Exclude)
                {
                    self.scanner.scan_root(&nested, cancel, on_event)?;
                }
            }
        }
        change.root = Some(root);
        Ok(change)
    }
}
