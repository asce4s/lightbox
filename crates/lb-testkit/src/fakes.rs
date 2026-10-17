//! Fakes for operating-system integrations (SPEC §12.2).

use lb_core::{Opener, TrashBackend};
use parking_lot::Mutex;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

/// A trash can that is just a directory: files are moved into it, never deleted.
#[derive(Debug)]
pub struct FakeTrash {
    dir: PathBuf,
    trashed: Mutex<Vec<PathBuf>>,
    fail: AtomicBool,
}

impl FakeTrash {
    /// A trash that stores files under `dir` (created on demand).
    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self {
            dir: dir.into(),
            trashed: Mutex::new(Vec::new()),
            fail: AtomicBool::new(false),
        }
    }

    /// Original paths of everything trashed so far, in order.
    pub fn trashed(&self) -> Vec<PathBuf> {
        self.trashed.lock().clone()
    }

    /// Where the trash keeps its files.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Makes every following `trash` call fail, like a system without a trash can.
    pub fn set_unavailable(&self, unavailable: bool) {
        self.fail.store(unavailable, Ordering::Relaxed);
    }
}

impl TrashBackend for FakeTrash {
    fn trash(&self, path: &Path) -> io::Result<()> {
        if self.fail.load(Ordering::Relaxed) {
            return Err(io::Error::other("trash is unavailable"));
        }
        std::fs::create_dir_all(&self.dir)?;
        let name = path
            .file_name()
            .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "path has no file name"))?;
        let mut target = self.dir.join(name);
        let mut n = 1;
        while target.exists() {
            target = self.dir.join(format!("{n}-{}", name.to_string_lossy()));
            n += 1;
        }
        if std::fs::rename(path, &target).is_err() {
            std::fs::copy(path, &target)?;
            std::fs::remove_file(path)?;
        }
        self.trashed.lock().push(path.to_path_buf());
        Ok(())
    }
}

/// What a [`FakeOpener`] was asked to do.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OpenAction {
    /// `open`
    Open,
    /// `reveal`
    Reveal,
}

/// Records open and reveal requests instead of launching anything.
#[derive(Debug, Default)]
pub struct FakeOpener {
    calls: Mutex<Vec<(OpenAction, PathBuf)>>,
}

impl FakeOpener {
    /// A recorder with no calls yet.
    pub fn new() -> Self {
        Self::default()
    }

    /// Requests in the order they were made.
    pub fn calls(&self) -> Vec<(OpenAction, PathBuf)> {
        self.calls.lock().clone()
    }
}

impl Opener for FakeOpener {
    fn open(&self, path: &Path) -> io::Result<()> {
        self.calls
            .lock()
            .push((OpenAction::Open, path.to_path_buf()));
        Ok(())
    }

    fn reveal(&self, path: &Path) -> io::Result<()> {
        self.calls
            .lock()
            .push((OpenAction::Reveal, path.to_path_buf()));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fake_trash_moves_and_disambiguates() {
        let tmp = tempfile::tempdir().unwrap();
        let trash = FakeTrash::new(tmp.path().join("trash"));
        for i in 0..2 {
            let dir = tmp.path().join(format!("d{i}"));
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join("a.jpg"), [i as u8]).unwrap();
            trash.trash(&dir.join("a.jpg")).unwrap();
            assert!(!dir.join("a.jpg").exists());
        }
        assert_eq!(trash.trashed().len(), 2);
        assert_eq!(std::fs::read_dir(trash.dir()).unwrap().count(), 2);
        trash.set_unavailable(true);
        assert!(trash.trash(tmp.path()).is_err());
    }
}
