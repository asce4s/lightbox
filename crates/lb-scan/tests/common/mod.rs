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

use lb_scan::{FolderManager, WatchConfig, WatchService};
use std::sync::{Arc, Mutex};
use std::time::SystemTime;

pub const WAIT: Duration = Duration::from_secs(10);

pub fn fast_watch() -> WatchConfig {
    WatchConfig {
        debounce: Duration::from_millis(50),
        settle: Duration::from_millis(20),
    }
}

pub struct Watching {
    pub manager: FolderManager,
    pub service: WatchService,
    pub events: Arc<Mutex<Vec<ScanEvent>>>,
}

pub fn watch(env: &Env, cfg: WatchConfig) -> Watching {
    let manager = FolderManager::new(env.scanner.clone());
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    let service =
        WatchService::start(manager.clone(), cfg, move |e| sink.lock().unwrap().push(e)).unwrap();
    Watching {
        manager,
        service,
        events,
    }
}

/// Copies `src` to `dst` and sets a deterministic mtime (`base + offset_s`), so size-and-mtime
/// change detection never depends on wall-clock resolution.
pub fn put(src: &Path, dst: &Path, offset_s: u64) {
    if let Some(parent) = dst.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::copy(src, dst).unwrap();
    let mtime = SystemTime::UNIX_EPOCH + Duration::from_secs(1_577_836_800 + offset_s);
    std::fs::File::options()
        .write(true)
        .open(dst)
        .unwrap()
        .set_modified(mtime)
        .unwrap();
}

pub fn add_watch_root(w: &Watching, dir: &Path) -> Root {
    let change = w
        .manager
        .add_root(
            dir,
            RootMode::Watch,
            chrono::NaiveDateTime::default(),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    w.service.sync_roots().unwrap();
    change.root.unwrap()
}
