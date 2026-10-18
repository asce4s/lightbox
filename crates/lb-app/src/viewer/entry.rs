//! What the viewer can show, and the set it can move through (SPEC-VIEWER §V1.2).

use serde::{Deserialize, Serialize};
use std::fs::{self, Metadata};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

use crossbeam_channel::{Receiver, TryRecvError};
use lb_core::{MediaType, PhotoId, PhotoStatus, natural_cmp_os};
use lb_media::{Capabilities, format_for_extension};
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

use crate::error::{AppError, Result};
use crate::lightbox::Lightbox;

/// How a folder's files are ordered. The default is natural file name.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum FolderSort {
    /// `img2` before `img02` before `img10`.
    #[default]
    NameNatural,
    /// Oldest modification time first.
    DateModified,
    /// EXIF capture time, then modification time.
    DateTaken,
}

/// Where the current list of entries came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NavSource {
    /// The library view, in the order the grid showed.
    Library {
        /// Photo ids in filmstrip order.
        ids: Vec<PhotoId>,
    },
    /// Supported files in one directory, non-recursive.
    Folder {
        /// The directory that was listed.
        dir: PathBuf,
        /// Order used for the listing.
        sort: FolderSort,
    },
    /// Exactly these paths, in this order. Siblings are not included.
    Files(Vec<PathBuf>),
}

/// One thing the viewer can show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewerEntry {
    /// Absolute path. Always set.
    pub path: PathBuf,
    /// Set when this path is an indexed catalog photo that is not missing or unreadable.
    pub photo: Option<PhotoId>,
    /// From the catalog, otherwise from the extension.
    pub media_type: MediaType,
}

/// What a folder refresh did to the current entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FolderRefresh {
    /// The current path is still in the list.
    Kept,
    /// The current path disappeared; the viewer moved to a neighbour.
    Advanced,
    /// Nothing left to show.
    Empty,
}

/// The entries the viewer can navigate, plus which one is current.
#[derive(Debug, Clone)]
pub struct NavSet {
    entries: Vec<ViewerEntry>,
    index: usize,
    source: NavSource,
}

impl NavSet {
    /// Library order. `index` selects the starting photo.
    #[must_use]
    pub fn library(entries: Vec<ViewerEntry>, index: usize, ids: Vec<PhotoId>) -> Self {
        Self {
            index: index.min(entries.len().saturating_sub(1)),
            entries,
            source: NavSource::Library { ids },
        }
    }

    /// Opens `paths` (SPEC-VIEWER §V2.2).
    ///
    /// One directory lists that folder and starts at the first file. One file lists its parent
    /// folder and starts at that file. Several paths are shown in the given order only.
    pub fn open_paths(lb: &Lightbox, paths: &[PathBuf]) -> Result<Self> {
        if paths.is_empty() {
            return Err(AppError::Invalid("no files to open".into()));
        }
        if paths.len() == 1 {
            return open_one(lb, &paths[0]);
        }
        let entries = paths
            .iter()
            .map(|p| resolve_entry(lb, p))
            .collect::<Result<Vec<_>>>()?;
        Ok(Self {
            entries,
            index: 0,
            source: NavSource::Files(paths.to_vec()),
        })
    }

    /// The current entry.
    #[must_use]
    pub fn current(&self) -> &ViewerEntry {
        &self.entries[self.index]
    }

    /// Every entry, in navigation order.
    #[must_use]
    pub fn entries(&self) -> &[ViewerEntry] {
        &self.entries
    }

    /// Index of [`Self::current`].
    #[must_use]
    pub fn index(&self) -> usize {
        self.index
    }

    /// Where this list came from.
    #[must_use]
    pub fn source(&self) -> &NavSource {
        &self.source
    }

    /// Directory being listed, when this set follows a folder.
    #[must_use]
    pub fn source_dir(&self) -> Option<&Path> {
        match &self.source {
            NavSource::Folder { dir, .. } => Some(dir),
            NavSource::Library { .. } | NavSource::Files(_) => None,
        }
    }

    /// Drops entries whose catalog id is in `removed`.
    pub fn retain_photos(&mut self, removed: &[PhotoId]) {
        self.entries
            .retain(|e| e.photo.is_none_or(|id| !removed.contains(&id)));
    }

    /// Sets the current index. Callers clamp it.
    pub fn set_index(&mut self, index: usize) {
        if self.entries.is_empty() {
            self.index = 0;
        } else {
            self.index = index.min(self.entries.len() - 1);
        }
    }

    /// Looks up the current path in the catalog again (after the folder is added).
    pub fn reresolve_current(&mut self, lb: &Lightbox) -> Result<()> {
        if self.entries.is_empty() {
            return Ok(());
        }
        let path = self.current().path.clone();
        let index = self.index;
        self.entries[index] = resolve_entry(lb, &path)?;
        Ok(())
    }

    /// Moves to `index`. Same index and out-of-range indexes do nothing.
    pub fn go(&mut self, index: usize) -> bool {
        if index == self.index || index >= self.entries.len() {
            return false;
        }
        self.index = index;
        true
    }

    /// Re-reads a folder (or checks that explicit files are still there).
    ///
    /// A new sibling is inserted in sort order and the current index stays on the same path.
    /// If the current file has gone, the viewer advances to the next entry, or the previous
    /// one when it was last.
    pub fn refresh(&mut self, lb: &Lightbox) -> Result<FolderRefresh> {
        let NavSource::Folder { dir, sort } = &self.source else {
            return Ok(FolderRefresh::Kept);
        };
        let dir = dir.clone();
        let sort = *sort;
        let fresh = list_resolved(lb, &dir, sort)?;
        self.apply_listing(fresh)
    }

    fn apply_listing(&mut self, fresh: Vec<ViewerEntry>) -> Result<FolderRefresh> {
        let current = self.current().path.clone();
        if let Some(index) = fresh.iter().position(|e| e.path == current) {
            self.entries = fresh;
            self.index = index;
            return Ok(FolderRefresh::Kept);
        }
        if fresh.is_empty() {
            self.entries.clear();
            self.index = 0;
            return Ok(FolderRefresh::Empty);
        }
        self.index = self.index.min(fresh.len() - 1);
        self.entries = fresh;
        Ok(FolderRefresh::Advanced)
    }
}

/// Non-recursive directory watch. `poll` reports whether anything changed.
pub struct FolderWatch {
    _watcher: RecommendedWatcher,
    rx: Receiver<Result<Event, notify::Error>>,
}

impl FolderWatch {
    /// Watches `dir` itself, not its children.
    pub fn start(dir: &Path) -> std::result::Result<Self, notify::Error> {
        let (tx, rx) = crossbeam_channel::unbounded();
        let mut watcher = notify::recommended_watcher(move |res| {
            let _ = tx.send(res);
        })?;
        watcher.watch(dir, RecursiveMode::NonRecursive)?;
        Ok(Self {
            _watcher: watcher,
            rx,
        })
    }

    /// Drains queued events. `true` when at least one arrived.
    #[must_use]
    pub fn poll(&self) -> bool {
        let mut any = false;
        loop {
            match self.rx.try_recv() {
                Ok(_) => any = true,
                Err(TryRecvError::Empty | TryRecvError::Disconnected) => return any,
            }
        }
    }
}

fn open_one(lb: &Lightbox, path: &Path) -> Result<NavSet> {
    if path.is_dir() {
        let entries = list_resolved(lb, path, FolderSort::NameNatural)?;
        if entries.is_empty() {
            return Err(AppError::Invalid("folder has no supported files".into()));
        }
        return Ok(NavSet {
            entries,
            index: 0,
            source: NavSource::Folder {
                dir: path.to_path_buf(),
                sort: FolderSort::NameNatural,
            },
        });
    }
    let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) else {
        return single(lb, path);
    };
    let mut entries = list_resolved(lb, dir, FolderSort::NameNatural)?;
    let index = if let Some(i) = entries.iter().position(|e| e.path == path) {
        i
    } else {
        entries.push(resolve_entry(lb, path)?);
        entries.len() - 1
    };
    Ok(NavSet {
        entries,
        index,
        source: NavSource::Folder {
            dir: dir.to_path_buf(),
            sort: FolderSort::NameNatural,
        },
    })
}

fn single(lb: &Lightbox, path: &Path) -> Result<NavSet> {
    Ok(NavSet {
        entries: vec![resolve_entry(lb, path)?],
        index: 0,
        source: NavSource::Files(vec![path.to_path_buf()]),
    })
}

fn list_resolved(lb: &Lightbox, dir: &Path, sort: FolderSort) -> Result<Vec<ViewerEntry>> {
    list_folder(dir, sort)?
        .into_iter()
        .map(|path| resolve_entry(lb, &path))
        .collect()
}

/// Supported, non-hidden files in `dir` (not its subfolders), in `sort` order.
pub fn list_folder(dir: &Path, sort: FolderSort) -> Result<Vec<PathBuf>> {
    let caps = viewer_caps();
    let mut files: Vec<(PathBuf, Metadata)> = fs::read_dir(dir)?
        .filter_map(|entry| entry.ok())
        .filter_map(|entry| {
            let name = entry.file_name();
            if lb_scan::is_ignored_name(&name.to_string_lossy()) {
                return None;
            }
            let ext = Path::new(&name)
                .extension()
                .and_then(|e| e.to_str())
                .unwrap_or("");
            format_for_extension(ext, caps)?;
            let meta = entry.metadata().ok()?;
            if !meta.is_file() {
                return None;
            }
            Some((entry.path(), meta))
        })
        .collect();
    files.sort_by(|a, b| sort_files(sort, a, b));
    Ok(files.into_iter().map(|(path, _)| path).collect())
}

fn sort_files(
    sort: FolderSort,
    a: &(PathBuf, Metadata),
    b: &(PathBuf, Metadata),
) -> std::cmp::Ordering {
    let by_name = natural_cmp_os(
        a.0.file_name().unwrap_or_default(),
        b.0.file_name().unwrap_or_default(),
    );
    match sort {
        FolderSort::NameNatural => by_name,
        FolderSort::DateModified => mtime(&a.1).cmp(&mtime(&b.1)).then(by_name),
        FolderSort::DateTaken => taken(&a.0, &a.1).cmp(&taken(&b.0, &b.1)).then(by_name),
    }
}

fn mtime(meta: &Metadata) -> SystemTime {
    meta.modified().unwrap_or(SystemTime::UNIX_EPOCH)
}

fn taken(path: &Path, meta: &Metadata) -> i64 {
    if let Some(dt) = lb_media::read_exif(path).date_time_original {
        return dt.and_utc().timestamp();
    }
    mtime(meta)
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}

fn resolve_entry(lb: &Lightbox, path: &Path) -> Result<ViewerEntry> {
    let photo = lb.catalog().photo_by_path(path)?;
    let (id, media_type) = match photo {
        Some(photo) if photo.status == PhotoStatus::Ok => (Some(photo.id), photo.media_type),
        Some(photo) => (None, photo.media_type),
        None => (None, media_of(path)),
    };
    Ok(ViewerEntry {
        path: path.to_path_buf(),
        photo: id,
        media_type,
    })
}

/// An entry for a catalog id. Used when the library grid opens the viewer.
pub fn entry_from_id(lb: &Lightbox, id: PhotoId) -> ViewerEntry {
    match lb.photos().get(id) {
        Ok(Some(photo)) => ViewerEntry {
            path: photo.path,
            photo: (photo.status == PhotoStatus::Ok).then_some(id),
            media_type: photo.media_type,
        },
        _ => ViewerEntry {
            path: PathBuf::new(),
            photo: Some(id),
            media_type: MediaType::Image,
        },
    }
}

/// Why this path cannot be shown, if opening it should show an error card.
#[must_use]
pub fn entry_error(path: &Path) -> Option<&'static str> {
    let ext = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
    if format_for_extension(ext, viewer_caps()).is_none() {
        return Some("Lightbox can't open this kind of file");
    }
    match path.metadata() {
        Ok(meta) if meta.is_file() => None,
        Ok(_) => Some("Lightbox can't open this kind of file"),
        Err(error) if error.kind() == std::io::ErrorKind::PermissionDenied => {
            Some("Permission denied")
        }
        Err(_) => Some("File not found"),
    }
}

fn media_of(path: &Path) -> MediaType {
    path.extension()
        .and_then(|e| e.to_str())
        .and_then(|ext| format_for_extension(ext, viewer_caps()))
        .map(|format| format.media_type())
        .unwrap_or(MediaType::Image)
}

fn viewer_caps() -> Capabilities {
    Capabilities {
        heif: lb_media::heif_decoder_available(),
        raw: lb_media::raw_decoder_available(),
        video: cfg!(feature = "video"),
    }
}
