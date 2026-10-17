//! Global invariants every integration test must leave intact (SPEC §12.6).
//!
//! Build an [`Invariants`] at the start of a test, hand it the directories and the catalog the
//! test uses, and let it drop at the end: the drop checks everything unless the test is already
//! panicking.

use lb_catalog::Catalog;
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Whether `name` is the temporary name of a half-written file (atomic writes and exports end in
/// `.lbtmp`; batch rename stages files as `.lbrename-<id>.tmp`).
pub fn is_temp_name(name: &str) -> bool {
    name.ends_with(".lbtmp") || (name.starts_with(".lbrename-") && name.ends_with(".tmp"))
}

/// How strictly a snapshotted directory must stay as it was.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Strictness {
    /// Content that is still present somewhere in the folder is intact; moves, renames and
    /// deletions are fine.
    Content,
    /// Nothing is added, removed or changed except files declared with
    /// [`Invariants::declare_changed`].
    Exact,
}

struct Snapshot {
    dir: PathBuf,
    files: BTreeMap<PathBuf, String>,
    strictness: Strictness,
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        match entry.file_type() {
            Ok(t) if t.is_dir() => walk(&path, out),
            Ok(t) if t.is_file() => out.push(path),
            _ => {}
        }
    }
}

/// Every file under `dir`, sorted.
pub fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    walk(dir, &mut out);
    out.sort();
    out
}

fn hash_file(path: &Path) -> String {
    match std::fs::read(path) {
        Ok(bytes) => blake3::hash(&bytes).to_hex().to_string(),
        Err(e) => format!("unreadable: {e}"),
    }
}

fn snapshot(dir: &Path, strictness: Strictness) -> Snapshot {
    Snapshot {
        dir: dir.to_path_buf(),
        files: files_under(dir)
            .into_iter()
            .map(|p| {
                let h = hash_file(&p);
                (p, h)
            })
            .collect(),
        strictness,
    }
}

/// Collects what to check and checks it on [`Invariants::assert_ok`] or drop.
#[derive(Default)]
pub struct Invariants {
    snapshots: Vec<Snapshot>,
    catalogs: Vec<PathBuf>,
    temp_dirs: Vec<PathBuf>,
    declared: BTreeSet<PathBuf>,
    finished: bool,
}

impl Invariants {
    /// An empty set of invariants.
    pub fn new() -> Self {
        Self::default()
    }

    /// Records the files under `dir`; the content of every file must still be found in `dir`
    /// unless the file was deleted (moves and renames are allowed). Also watches `dir` for temp files.
    pub fn originals(mut self, dir: &Path) -> Self {
        self.snapshots.push(snapshot(dir, Strictness::Content));
        self.temp_dirs.push(dir.to_path_buf());
        self
    }

    /// Like [`Invariants::originals`] but no file may appear, disappear or change unless declared.
    pub fn originals_exact(mut self, dir: &Path) -> Self {
        self.snapshots.push(snapshot(dir, Strictness::Exact));
        self.temp_dirs.push(dir.to_path_buf());
        self
    }

    /// Checks the integrity, foreign keys and full-text index of the catalog file at `path`
    /// (opened separately when the check runs, so the library may be closed or still open).
    pub fn catalog(mut self, path: &Path) -> Self {
        self.catalogs.push(path.to_path_buf());
        self
    }

    /// Also requires that no temp-named file remains under `dir` (for example the cache).
    pub fn no_temp_files_in(mut self, dir: &Path) -> Self {
        self.temp_dirs.push(dir.to_path_buf());
        self
    }

    /// States that the test itself changed, moved or deleted `path`.
    pub fn declare_changed(&mut self, path: &Path) {
        self.declared.insert(path.to_path_buf());
    }

    /// Everything that is wrong right now, one line each.
    pub fn violations(&self) -> Vec<String> {
        let mut out = Vec::new();
        for snap in &self.snapshots {
            let now: BTreeMap<PathBuf, String> = files_under(&snap.dir)
                .into_iter()
                .map(|p| {
                    let h = hash_file(&p);
                    (p, h)
                })
                .collect();
            let contents: BTreeSet<&String> = now.values().collect();
            for (path, before) in &snap.files {
                if self.declared.contains(path) {
                    continue;
                }
                if let Some(after) = now.get(path) {
                    // A rename chain can leave other content at the path; only content that
                    // vanished from the folder altogether counts as modified.
                    let vanished = !contents.contains(before);
                    let changed =
                        after != before && (snap.strictness == Strictness::Exact || vanished);
                    if changed {
                        out.push(format!("original changed: {}", path.display()));
                    }
                } else if snap.strictness == Strictness::Exact {
                    out.push(format!("original removed: {}", path.display()));
                }
            }
            if snap.strictness == Strictness::Exact {
                for path in now.keys() {
                    if !snap.files.contains_key(path) && !self.declared.contains(path) {
                        out.push(format!("unexpected new file: {}", path.display()));
                    }
                }
            }
        }
        for dir in &self.temp_dirs {
            for path in files_under(dir) {
                let name = path.file_name().map(|n| n.to_string_lossy().into_owned());
                if name.as_deref().is_some_and(is_temp_name) {
                    out.push(format!("temp file left behind: {}", path.display()));
                }
            }
        }
        for path in &self.catalogs {
            if !path.is_file() {
                continue;
            }
            match Catalog::open(path).and_then(|c| c.verify()) {
                Ok(problems) => out.extend(problems.into_iter().map(|p| format!("catalog: {p}"))),
                Err(e) => out.push(format!("catalog check failed: {e}")),
            }
        }
        out
    }

    /// Panics listing every violation.
    pub fn assert_ok(&mut self) {
        self.finished = true;
        let violations = self.violations();
        assert!(
            violations.is_empty(),
            "invariants violated:\n  {}",
            violations.join("\n  ")
        );
    }
}

impl Drop for Invariants {
    fn drop(&mut self) {
        if !self.finished && !std::thread::panicking() {
            self.assert_ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_problems_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("catalog.sqlite3");
        drop(Catalog::open(&path).unwrap());
        Invariants::new().catalog(&path).assert_ok();
        let missing = dir.path().join("none.sqlite3");
        Invariants::new().catalog(&missing).assert_ok();
    }

    #[test]
    fn temp_names_are_recognised() {
        assert!(is_temp_name("a.jpg.lbtmp"));
        assert!(is_temp_name(".lbrename-12.tmp"));
        assert!(!is_temp_name("a.jpg"));
        assert!(!is_temp_name("notes.tmp"));
    }

    #[test]
    fn content_changes_and_leftovers_are_reported() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        let b = dir.path().join("b.txt");
        std::fs::write(&a, "one").unwrap();
        std::fs::write(&b, "two").unwrap();
        let mut inv = Invariants::new().originals(dir.path());
        assert!(inv.violations().is_empty());

        std::fs::remove_file(&b).unwrap();
        std::fs::write(dir.path().join("c.txt"), "new").unwrap();
        assert!(inv.violations().is_empty(), "moves and deletions are fine");

        std::fs::write(&a, "ONE").unwrap();
        std::fs::write(dir.path().join("x.lbtmp"), "").unwrap();
        let v = inv.violations();
        assert_eq!(v.len(), 2, "{v:?}");
        inv.declare_changed(&a);
        std::fs::remove_file(dir.path().join("x.lbtmp")).unwrap();
        assert!(inv.violations().is_empty());
    }

    #[test]
    fn exact_mode_flags_removals_and_additions() {
        let dir = tempfile::tempdir().unwrap();
        let a = dir.path().join("a.txt");
        std::fs::write(&a, "one").unwrap();
        let mut inv = Invariants::new().originals_exact(dir.path());
        std::fs::remove_file(&a).unwrap();
        std::fs::write(dir.path().join("n.txt"), "new").unwrap();
        assert_eq!(inv.violations().len(), 2);
        inv.declare_changed(&a);
        inv.declare_changed(&dir.path().join("n.txt"));
        inv.assert_ok();
    }

    #[test]
    #[should_panic(expected = "invariants violated")]
    fn assert_ok_panics_on_violation() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(dir.path().join("a.lbtmp"), "").unwrap();
        let mut inv = Invariants::new().no_temp_files_in(dir.path());
        inv.assert_ok();
    }
}
