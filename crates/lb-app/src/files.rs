//! File operations on originals: rename, move, delete to trash, reveal, relink (SPEC §6.7.4).
//!
//! These are the only places Lightbox changes user files. They are not undoable through
//! history; the trash is the safety net.

use crate::error::{AppError, Result};
use crate::events::AppEvent;
use crate::library::LibraryService;
use crate::photos::PhotoService;
use lb_catalog::NewFolder;
use lb_core::{Folder, FolderId, Photo, PhotoId, PhotoStatus, RootMode};
use std::collections::HashSet;
use std::io;
use std::path::Path;

/// Outcome of a batch move.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MoveReport {
    /// Photos now in the destination folder.
    pub moved: Vec<PhotoId>,
    /// Photos that were already there.
    pub unchanged: Vec<PhotoId>,
    /// Photos that could not be moved, with the reason.
    pub failed: Vec<(PhotoId, String)>,
}

/// Outcome of a batch delete.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DeleteReport {
    /// Photos moved to the trash and removed from the catalog.
    pub deleted: Vec<PhotoId>,
    /// Photos that stayed because the trash refused them, with the reason.
    pub failed: Vec<(PhotoId, String)>,
}

const MAX_NAME_BYTES: usize = 255;
const WINDOWS_RESERVED: [&str; 22] = [
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn invalid(msg: impl Into<String>) -> AppError {
    AppError::Invalid(msg.into())
}

/// Checks a single path component typed by the user.
pub(crate) fn validate_file_name(name: &str) -> Result<()> {
    if name.trim().is_empty() {
        return Err(invalid("the name is empty"));
    }
    if name.contains(['/', '\\', '\0']) {
        return Err(invalid("the name must not contain path separators"));
    }
    if name == "." || name == ".." {
        return Err(invalid("the name is reserved"));
    }
    if name.starts_with('.') {
        return Err(invalid(
            "names starting with a dot are hidden and would leave the library",
        ));
    }
    if name.len() > MAX_NAME_BYTES {
        return Err(invalid("the name is too long"));
    }
    if cfg!(windows) {
        let stem = name.split('.').next().unwrap_or(name).to_ascii_uppercase();
        if WINDOWS_RESERVED.contains(&stem.as_str()) {
            return Err(invalid(format!("{stem} is a reserved name on Windows")));
        }
        if name.ends_with([' ', '.']) || name.contains([':', '*', '?', '"', '<', '>', '|']) {
            return Err(invalid(
                "the name contains characters Windows does not allow",
            ));
        }
    }
    Ok(())
}

/// Moves `src` to `dst` (which must not exist). Renames when possible; across volumes copies,
/// verifies the content hash, then deletes the source.
pub(crate) fn move_file(src: &Path, dst: &Path, force_copy: bool) -> io::Result<()> {
    if dst.exists() {
        return Err(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "destination exists",
        ));
    }
    if !force_copy {
        match std::fs::rename(src, dst) {
            Ok(()) => return Ok(()),
            Err(e) if e.kind() == io::ErrorKind::CrossesDevices => {}
            Err(e) => return Err(e),
        }
    }
    let tmp = dst.with_extension(format!(
        "{}.lbtmp",
        dst.extension().and_then(|e| e.to_str()).unwrap_or("")
    ));
    let result = (|| {
        std::fs::copy(src, &tmp)?;
        let (a, b) = (lb_media::content_hash(src), lb_media::content_hash(&tmp));
        if a.is_err() || b.is_err() || a.ok() != b.ok() {
            return Err(io::Error::other(
                "copy verification failed; source left in place",
            ));
        }
        if let Ok(mtime) = std::fs::metadata(src).and_then(|m| m.modified()) {
            std::fs::File::options()
                .write(true)
                .open(&tmp)?
                .set_modified(mtime)?;
        }
        std::fs::rename(&tmp, dst)
    })();
    match result {
        Ok(()) => std::fs::remove_file(src),
        Err(e) => {
            let _ = std::fs::remove_file(&tmp);
            Err(e)
        }
    }
}

impl PhotoService {
    fn existing(&self, id: PhotoId) -> Result<Photo> {
        self.get(id)?.ok_or(AppError::NotFound("photo"))
    }

    /// Renames the file of `id`. A name without a dot keeps the current extension; one with a
    /// dot replaces it and must be a supported media extension.
    pub fn rename(&self, id: PhotoId, new_name: &str) -> Result<Photo> {
        let photo = self.existing(id)?;
        let new_name = new_name.trim();
        validate_file_name(new_name)?;
        let name = if new_name.contains('.') {
            new_name.to_owned()
        } else {
            match photo.path.extension().and_then(|e| e.to_str()) {
                Some(ext) => format!("{new_name}.{ext}"),
                None => new_name.to_owned(),
            }
        };
        let ext = Path::new(&name)
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("");
        if !lb_media::is_supported_extension(ext, self.lb.inner.folders.scanner().options().caps) {
            return Err(invalid(format!("\".{ext}\" is not a supported file type")));
        }
        let target = photo.path.with_file_name(&name);
        if target == photo.path {
            return Ok(photo);
        }
        if target.exists() {
            return Err(AppError::AlreadyExists(target));
        }
        std::fs::rename(&photo.path, &target)?;
        if let Err(e) =
            self.lb
                .catalog()
                .relocate_photo(id, photo.folder_id, &name, self.lb.clock().now())
        {
            let _ = std::fs::rename(&target, &photo.path);
            return Err(e.into());
        }
        self.lb.emit(AppEvent::PhotosChanged { ids: vec![id] });
        self.existing(id)
    }

    /// Moves the files of `ids` into the folder `dest`, which must lie inside a library root.
    /// Photo ids and all user data are preserved. Failures are reported per photo.
    pub fn move_to_folder(&self, ids: &[PhotoId], dest: &Path) -> Result<MoveReport> {
        self.move_to_folder_impl(ids, dest, false)
    }

    pub(crate) fn move_to_folder_impl(
        &self,
        ids: &[PhotoId],
        dest: &Path,
        force_copy: bool,
    ) -> Result<MoveReport> {
        let folder = self.ensure_folder_chain(dest)?;
        let mut report = MoveReport::default();
        let mut seen = HashSet::new();
        for &id in ids.iter().filter(|id| seen.insert(**id)) {
            let Some(photo) = self.get(id)? else {
                report.failed.push((id, "photo not found".into()));
                continue;
            };
            if photo.folder_id == folder.id {
                report.unchanged.push(id);
                continue;
            }
            let Some(name) = photo.path.file_name().and_then(|n| n.to_str()) else {
                report
                    .failed
                    .push((id, "file name is not valid UTF-8".into()));
                continue;
            };
            let target = folder.path.join(name);
            if let Err(e) = move_file(&photo.path, &target, force_copy) {
                report.failed.push((id, e.to_string()));
                continue;
            }
            match self
                .lb
                .catalog()
                .relocate_photo(id, folder.id, name, self.lb.clock().now())
            {
                Ok(()) => report.moved.push(id),
                Err(e) => {
                    let _ = move_file(&target, &photo.path, false);
                    report.failed.push((id, e.to_string()));
                }
            }
        }
        if !report.moved.is_empty() {
            self.lb.emit(AppEvent::PhotosChanged {
                ids: report.moved.clone(),
            });
            self.lb.emit(AppEvent::FoldersChanged);
        }
        Ok(report)
    }

    fn ensure_folder_chain(&self, dest: &Path) -> Result<Folder> {
        let dest = lb_scan::normalize_path(dest)
            .map_err(|_| invalid("the destination folder does not exist"))?;
        if !dest.is_dir() {
            return Err(invalid("the destination is not a folder"));
        }
        let root = self
            .lb
            .inner
            .folders
            .effective_root(&dest)?
            .filter(|r| r.mode != RootMode::Exclude)
            .ok_or_else(|| invalid("the destination is outside the photo library"))?;
        let catalog = self.lb.catalog();
        let mut current = root.path.clone();
        let relative = dest.strip_prefix(&root.path).unwrap_or(Path::new(""));
        let mut folder = catalog.ensure_folder(NewFolder {
            root_id: root.id,
            path: current.clone(),
            parent_id: None,
        })?;
        for component in relative.components() {
            let parent = folder.id;
            current.push(component);
            folder = catalog.ensure_folder(NewFolder {
                root_id: root.id,
                path: current.clone(),
                parent_id: Some(parent),
            })?;
        }
        Ok(folder)
    }

    /// Moves the files of `ids` to the system trash and removes them from the catalog. A photo
    /// stays in the library if the trash refuses its file; nothing is ever deleted permanently.
    pub fn delete(&self, ids: &[PhotoId]) -> Result<DeleteReport> {
        let mut report = DeleteReport::default();
        let mut seen = HashSet::new();
        for &id in ids.iter().filter(|id| seen.insert(**id)) {
            let Some(photo) = self.get(id)? else {
                continue;
            };
            let gone = photo.status == PhotoStatus::Missing || !photo.path.exists();
            if !gone && let Err(e) = self.lb.inner.trash.trash(&photo.path) {
                report.failed.push((id, e.to_string()));
                continue;
            }
            report.deleted.push(id);
        }
        if !report.deleted.is_empty() {
            self.lb.catalog().delete_photos(&report.deleted)?;
            self.lb.emit(AppEvent::PhotosRemoved {
                ids: report.deleted.clone(),
            });
        }
        Ok(report)
    }

    /// Shows the photo's file in the system file manager.
    pub fn reveal(&self, id: PhotoId) -> Result<()> {
        let photo = self.existing(id)?;
        Ok(self.lb.inner.opener.reveal(&photo.path)?)
    }

    /// Opens the photo's file with the default application.
    pub fn open_externally(&self, id: PhotoId) -> Result<()> {
        let photo = self.existing(id)?;
        Ok(self.lb.inner.opener.open(&photo.path)?)
    }
}

impl LibraryService {
    /// Renames a folder on disk and rewrites the paths of everything below it. Roots are
    /// renamed in the Folder Manager, not here.
    pub fn rename_folder(&self, folder: FolderId, new_name: &str) -> Result<Folder> {
        let current = self
            .lb
            .catalog()
            .get_folder(folder)?
            .ok_or(AppError::NotFound("folder"))?;
        let new_name = new_name.trim();
        validate_file_name(new_name)?;
        if self.roots()?.iter().any(|r| r.path == current.path) {
            return Err(invalid("a root folder cannot be renamed here"));
        }
        let target = current.path.with_file_name(new_name);
        if target == current.path {
            return Ok(current);
        }
        if target.exists() {
            return Err(AppError::AlreadyExists(target));
        }
        std::fs::rename(&current.path, &target)?;
        let ids = match self.lb.catalog().rename_folder_tree(&current.path, &target) {
            Ok(ids) => ids,
            Err(e) => {
                let _ = std::fs::rename(&target, &current.path);
                return Err(e.into());
            }
        };
        self.lb.emit(AppEvent::FoldersChanged);
        if !ids.is_empty() {
            self.lb.emit(AppEvent::PhotosChanged { ids });
        }
        self.lb
            .catalog()
            .get_folder(folder)?
            .ok_or(AppError::NotFound("folder"))
    }

    /// "Locate on disk": indexes `dir` (adding it as a one-off root when no root covers it) so
    /// that missing photos whose content matches a file there are relinked by quick key
    /// (SPEC §6.3.2). Returns how many photos were relinked.
    pub fn relink_missing(&self, dir: &Path) -> Result<usize> {
        let normalized =
            lb_scan::normalize_path(dir).map_err(|_| invalid("the folder does not exist"))?;
        let covering = self
            .lb
            .inner
            .folders
            .effective_root(&normalized)?
            .filter(|r| r.mode != RootMode::Exclude);
        let before = self.lb.catalog().has_missing()?;
        if !before {
            return Ok(0);
        }
        let Some(root) = covering else {
            let change = self.add_root(&normalized, RootMode::Once)?;
            return Ok(change.scan.map_or(0, |s| s.moved));
        };
        self.lb
            .inner
            .scan_cancel
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let lb = self.lb.clone();
        let summary = self.lb.inner.folders.scanner().scan_under(
            &root,
            &normalized,
            &self.lb.inner.scan_cancel,
            &mut |e| lb.emit(crate::library::scan_event_to_app(e)),
        )?;
        self.lb.emit(AppEvent::FoldersChanged);
        Ok(summary.moved)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn copy_fallback_verifies_preserves_mtime_and_removes_source() {
        let dir = tempfile::tempdir().unwrap();
        let (src, dst) = (dir.path().join("a.jpg"), dir.path().join("b.jpg"));
        std::fs::write(&src, b"pixels").unwrap();
        let mtime =
            std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_577_836_800);
        std::fs::File::options()
            .write(true)
            .open(&src)
            .unwrap()
            .set_modified(mtime)
            .unwrap();

        move_file(&src, &dst, true).unwrap();

        assert!(!src.exists());
        assert_eq!(std::fs::read(&dst).unwrap(), b"pixels");
        assert_eq!(std::fs::metadata(&dst).unwrap().modified().unwrap(), mtime);
        let leftovers: Vec<_> = std::fs::read_dir(dir.path())
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        assert_eq!(
            leftovers,
            [std::ffi::OsString::from("b.jpg")],
            "no temp file remains"
        );
    }

    #[test]
    fn move_never_overwrites_and_failed_copy_keeps_source() {
        let dir = tempfile::tempdir().unwrap();
        let (src, dst) = (dir.path().join("a.jpg"), dir.path().join("b.jpg"));
        std::fs::write(&src, b"one").unwrap();
        std::fs::write(&dst, b"two").unwrap();
        for force in [false, true] {
            let err = move_file(&src, &dst, force).unwrap_err();
            assert_eq!(err.kind(), io::ErrorKind::AlreadyExists);
        }
        assert_eq!(std::fs::read(&src).unwrap(), b"one");
        assert_eq!(std::fs::read(&dst).unwrap(), b"two");

        let missing = dir.path().join("missing.jpg");
        assert!(move_file(&missing, &dir.path().join("c.jpg"), true).is_err());
        assert!(!dir.path().join("c.jpg").exists());
        assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 2);
    }

    #[test]
    fn name_validation_table() {
        for bad in ["", "  ", "a/b", "a\\b", ".", "..", ".hidden", "nul\0.jpg"] {
            assert!(validate_file_name(bad).is_err(), "{bad:?}");
        }
        assert!(validate_file_name(&"x".repeat(256)).is_err());
        for good in [
            "a.jpg",
            "Trip 2024",
            "café ☕.png",
            "CON",
            "x".repeat(255).as_str(),
        ] {
            assert_eq!(
                validate_file_name(good).is_ok(),
                !cfg!(windows) || good != "CON",
                "{good:?}"
            );
        }
    }
}
