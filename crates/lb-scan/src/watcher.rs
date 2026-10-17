//! Debounced file-system watcher that keeps the catalog in sync with `watch` roots
//! (SPEC §6.3.1).

use crate::{FolderManager, Result, ScanError, ScanEvent, ScanSummary, effective_root};
use crossbeam_channel::{Receiver, Sender};
use lb_core::{Root, RootMode};
use notify::{EventKind, RecursiveMode, event::AccessKind, event::AccessMode};
use notify_debouncer_full::{DebounceEventResult, Debouncer, RecommendedCache, new_debouncer};
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, PoisonError};
use std::thread::JoinHandle;
use std::time::{Duration, Instant, SystemTime};

/// Timing of the watcher.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WatchConfig {
    /// How long file-system events are coalesced (SPEC: 500 ms).
    pub debounce: Duration,
    /// A file whose size or mtime still changes after this gap is considered an in-progress
    /// copy and is re-queued (SPEC: 250 ms).
    pub settle: Duration,
}

impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            debounce: Duration::from_millis(500),
            settle: Duration::from_millis(250),
        }
    }
}

type Handler = Arc<dyn Fn(ScanEvent) + Send + Sync>;
type FsDebouncer = Debouncer<notify::RecommendedWatcher, RecommendedCache>;

enum Msg {
    Paths(Vec<PathBuf>),
    Rescan,
    Stop,
}

/// Observes every `watch` root and rescans what changed. Stops when dropped.
pub struct WatchService {
    manager: FolderManager,
    debouncer: Mutex<FsDebouncer>,
    watched: Mutex<HashSet<PathBuf>>,
    tx: Sender<Msg>,
    worker: Option<JoinHandle<()>>,
}

type Stat = Option<(bool, u64, Option<SystemTime>)>;

fn stat(path: &std::path::Path) -> Stat {
    std::fs::metadata(path)
        .ok()
        .map(|m| (m.is_dir(), m.len(), m.modified().ok()))
}

struct Pending {
    seen: Stat,
    due: Instant,
}

fn relevant(kind: &EventKind) -> bool {
    match kind {
        EventKind::Access(AccessKind::Close(AccessMode::Write)) => true,
        EventKind::Access(_) => false,
        _ => true,
    }
}

impl WatchService {
    /// Starts watching every `watch` root of `manager`. `on_event` receives a
    /// [`ScanEvent::Finished`] whenever a batch of file-system events changed the catalog.
    pub fn start(
        manager: FolderManager,
        config: WatchConfig,
        on_event: impl Fn(ScanEvent) + Send + Sync + 'static,
    ) -> Result<Self> {
        let (tx, rx) = crossbeam_channel::unbounded();
        let handler_tx = tx.clone();
        let debouncer =
            new_debouncer(
                config.debounce,
                None,
                move |result: DebounceEventResult| match result {
                    Ok(events) => {
                        if events.iter().any(|e| e.need_rescan()) {
                            let _ = handler_tx.send(Msg::Rescan);
                        }
                        let paths: Vec<PathBuf> = events
                            .iter()
                            .filter(|e| relevant(&e.kind))
                            .flat_map(|e| e.paths.iter().cloned())
                            .collect();
                        if !paths.is_empty() {
                            let _ = handler_tx.send(Msg::Paths(paths));
                        }
                    }
                    Err(errors) => {
                        for e in errors {
                            tracing::warn!(error = %e, "watcher error");
                        }
                        let _ = handler_tx.send(Msg::Rescan);
                    }
                },
            )
            .map_err(|e| ScanError::Watch(e.to_string()))?;
        let handler: Handler = Arc::new(on_event);
        let worker_manager = manager.clone();
        let worker = std::thread::Builder::new()
            .name("lb-watcher".into())
            .spawn(move || worker_loop(&rx, &worker_manager, config, &handler))
            .map_err(|source| ScanError::Io {
                path: PathBuf::from("lb-watcher"),
                source,
            })?;
        let service = Self {
            manager,
            debouncer: Mutex::new(debouncer),
            watched: Mutex::new(HashSet::new()),
            tx,
            worker: Some(worker),
        };
        service.sync_roots()?;
        Ok(service)
    }

    /// Aligns the set of observed directories with the roots currently in `watch` mode; call
    /// after adding, removing or re-moding a root.
    pub fn sync_roots(&self) -> Result<()> {
        let desired: HashSet<PathBuf> = self
            .manager
            .watched_roots()?
            .into_iter()
            .map(|r| r.path)
            .collect();
        let mut watched = self.watched.lock().unwrap_or_else(PoisonError::into_inner);
        let mut debouncer = self
            .debouncer
            .lock()
            .unwrap_or_else(PoisonError::into_inner);
        for gone in watched.difference(&desired).cloned().collect::<Vec<_>>() {
            if let Err(e) = debouncer.unwatch(&gone) {
                tracing::warn!(path = %gone.display(), error = %e, "unwatch failed");
            }
            watched.remove(&gone);
        }
        for new in desired.difference(&watched.clone()) {
            match debouncer.watch(new, RecursiveMode::Recursive) {
                Ok(()) => {
                    watched.insert(new.clone());
                }
                Err(e) => tracing::warn!(path = %new.display(), error = %e, "cannot watch root"),
            }
        }
        Ok(())
    }

    /// The directories currently observed.
    pub fn watched_paths(&self) -> Vec<PathBuf> {
        let mut v: Vec<_> = self
            .watched
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .iter()
            .cloned()
            .collect();
        v.sort();
        v
    }

    /// Schedules a full rescan of every watched root.
    pub fn request_rescan(&self) {
        let _ = self.tx.send(Msg::Rescan);
    }
}

impl Drop for WatchService {
    fn drop(&mut self) {
        let _ = self.tx.send(Msg::Stop);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

fn report(handler: &Handler, root: &Root, summary: ScanSummary) {
    if summary.added + summary.updated + summary.missing + summary.moved > 0 {
        handler(ScanEvent::Finished {
            root: root.id,
            summary,
        });
    }
}

fn worker_loop(
    rx: &Receiver<Msg>,
    manager: &FolderManager,
    config: WatchConfig,
    handler: &Handler,
) {
    let cancel = AtomicBool::new(false);
    let mut pending: BTreeMap<PathBuf, Pending> = BTreeMap::new();
    loop {
        let wait = pending
            .values()
            .map(|p| p.due.saturating_duration_since(Instant::now()))
            .min()
            .unwrap_or(Duration::from_secs(3600));
        match rx.recv_timeout(wait) {
            Ok(Msg::Paths(paths)) => {
                for path in paths {
                    let seen = stat(&path);
                    pending.insert(
                        path,
                        Pending {
                            seen,
                            due: Instant::now() + config.settle,
                        },
                    );
                }
            }
            Ok(Msg::Rescan) => full_rescan(manager, &cancel, handler),
            Ok(Msg::Stop) | Err(crossbeam_channel::RecvTimeoutError::Disconnected) => break,
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {}
        }
        let now = Instant::now();
        let mut ready: Vec<PathBuf> = Vec::new();
        for (path, p) in &mut pending {
            if p.due > now {
                continue;
            }
            let current = stat(path);
            if current == p.seen {
                ready.push(path.clone());
            } else {
                p.seen = current;
                p.due = now + config.settle;
            }
        }
        for path in &ready {
            pending.remove(path);
        }
        if !ready.is_empty() {
            apply(manager, &ready, &cancel, handler);
        }
    }
}

fn full_rescan(manager: &FolderManager, cancel: &AtomicBool, handler: &Handler) {
    let roots = match manager.watched_roots() {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "cannot list roots for rescan");
            return;
        }
    };
    for root in roots {
        match manager.scanner().scan_root(&root, cancel, &mut |_| {}) {
            Ok(summary) => report(handler, &root, summary),
            Err(e) => tracing::warn!(error = %e, "rescan failed"),
        }
    }
}

fn apply(manager: &FolderManager, ready: &[PathBuf], cancel: &AtomicBool, handler: &Handler) {
    let roots = match manager.roots() {
        Ok(r) => r,
        Err(e) => {
            tracing::warn!(error = %e, "cannot list roots");
            return;
        }
    };
    let mut by_root: BTreeMap<lb_core::RootId, (Root, Vec<PathBuf>)> = BTreeMap::new();
    for path in ready {
        if let Some(root) = effective_root(&roots, path).filter(|r| r.mode == RootMode::Watch) {
            by_root
                .entry(root.id)
                .or_insert_with(|| (root.clone(), Vec::new()))
                .1
                .push(path.clone());
        }
    }
    for (root, paths) in by_root.into_values() {
        match manager.scanner().sync_paths(&root, &paths, cancel) {
            Ok(summary) => report(handler, &root, summary),
            Err(e) => tracing::warn!(error = %e, "sync after file-system event failed"),
        }
    }
}
