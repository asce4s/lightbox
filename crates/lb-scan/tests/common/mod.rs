//! Shared helpers for scanner integration tests.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use lb_catalog::Catalog;
use lb_core::{Photo, Root, RootMode};
use lb_scan::{ScanEvent, ScanOptions, ScanSummary, Scanner};
use lb_testkit::fixtures;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

pub fn generated_basic() -> PathBuf {
    let dir = fixtures::default_generated_dir();
    assert!(
        dir.join(fixtures::MARKER).is_file(),
        "run `cargo xtask fixtures`"
    );
    dir.join("basic")
}

pub struct Env {
    pub dir: tempfile::TempDir,
    pub catalog: Catalog,
    pub scanner: Scanner,
}

pub fn env() -> Env {
    env_with(ScanOptions {
        progress_interval: Duration::ZERO,
        ..ScanOptions::default()
    })
}

pub fn env_with(opts: ScanOptions) -> Env {
    let dir = tempfile::tempdir().unwrap();
    let catalog = Catalog::open(&dir.path().join("catalog.sqlite3")).unwrap();
    let scanner = Scanner::new(catalog.clone(), opts);
    Env {
        dir,
        catalog,
        scanner,
    }
}

impl Env {
    /// A fresh private directory under the temp dir.
    pub fn subdir(&self, name: &str) -> PathBuf {
        let p = self.dir.path().join(name);
        std::fs::create_dir_all(&p).unwrap();
        p.canonicalize().unwrap()
    }

    pub fn add_root(&self, path: &Path, mode: RootMode) -> Root {
        self.catalog
            .add_root(path, mode, chrono::NaiveDateTime::default())
            .unwrap()
    }

    pub fn scan(&self, root: &Root) -> (ScanSummary, Vec<ScanEvent>) {
        let mut events = Vec::new();
        let summary = self
            .scanner
            .scan_root(root, &AtomicBool::new(false), &mut |e| events.push(e))
            .unwrap();
        (summary, events)
    }

    /// Every photo under `dir`, ordered by path.
    pub fn photos_under(&self, dir: &Path) -> Vec<Photo> {
        let ids: Vec<_> = self
            .catalog
            .indexed_files_under(dir)
            .unwrap()
            .into_iter()
            .map(|f| f.id)
            .collect();
        self.catalog.get_many(&ids).unwrap()
    }
}
