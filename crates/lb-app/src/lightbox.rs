//! The facade the UI and CLI talk to (SPEC §7.1).

use crate::error::Result;
use crate::events::{AppEvent, EventBus};
use crate::history::HistoryService;
use crate::paths::AppPaths;
use crossbeam_channel::Receiver;
use lb_catalog::Catalog;
use lb_core::{Clock, SystemClock};
use lb_jobs::JobQueue;
use lb_scan::{FolderManager, ScanOptions, Scanner, WatchService};
use lb_thumbs::ThumbCache;
use parking_lot::Mutex;
use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;

/// Injectable dependencies. The defaults are right for production; tests replace what they need.
#[derive(Clone)]
pub struct LightboxOptions {
    /// Time source for timestamps and event coalescing.
    pub clock: Arc<dyn Clock>,
    /// Whether to start the background thread that delivers coalesced events. Tests that drive
    /// time by hand turn this off and call [`EventBus::tick`].
    pub spawn_event_ticker: bool,
}

impl Default for LightboxOptions {
    fn default() -> Self {
        Self {
            clock: Arc::new(SystemClock::new()),
            spawn_event_ticker: true,
        }
    }
}

pub(crate) struct Inner {
    pub(crate) paths: AppPaths,
    pub(crate) catalog: Catalog,
    pub(crate) jobs: JobQueue,
    pub(crate) bus: EventBus,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) thumbs: ThumbCache,
    pub(crate) folders: FolderManager,
    pub(crate) history: HistoryService,
    pub(crate) scan_cancel: AtomicBool,
    pub(crate) edit_lock: Mutex<()>,
    pub(crate) watcher: Mutex<Option<WatchService>>,
}

/// An open Lightbox library. Cheap to clone; all clones share the same state.
#[derive(Clone)]
pub struct Lightbox {
    pub(crate) inner: Arc<Inner>,
}

impl std::fmt::Debug for Lightbox {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Lightbox")
            .field("paths", &self.inner.paths)
            .finish_non_exhaustive()
    }
}

impl Lightbox {
    /// Opens (creating if needed) the library whose data lives under `<home>/data` and
    /// `<home>/cache`.
    pub fn open(home: &Path, jobs: JobQueue) -> Result<Self> {
        Self::open_with(home, jobs, LightboxOptions::default())
    }

    /// Alias of [`Lightbox::open`] used by tests with a temporary directory.
    pub fn open_in(home: &Path, jobs: JobQueue) -> Result<Self> {
        Self::open(home, jobs)
    }

    /// Opens the library the running process should use: `LIGHTBOX_HOME` or the per-user
    /// directories.
    pub fn open_default(jobs: JobQueue) -> Result<Self> {
        Self::open_paths(AppPaths::resolve()?, jobs, LightboxOptions::default())
    }

    /// Like [`Lightbox::open`] with explicit dependencies.
    pub fn open_with(home: &Path, jobs: JobQueue, options: LightboxOptions) -> Result<Self> {
        Self::open_paths(AppPaths::under(home), jobs, options)
    }

    /// Opens a library at explicit directories.
    pub fn open_paths(paths: AppPaths, jobs: JobQueue, options: LightboxOptions) -> Result<Self> {
        paths.ensure()?;
        let catalog = Catalog::open(&paths.catalog())?;
        let thumbs = ThumbCache::open(&paths.cache)?;
        let clock_for_scan = options.clock.clone();
        let scanner = Scanner::with_clock(catalog.clone(), ScanOptions::default(), move || {
            clock_for_scan.now()
        });
        let bus = EventBus::new(options.clock.clone());
        if options.spawn_event_ticker {
            bus.spawn_ticker()?;
        }
        let history = HistoryService::new(catalog.clone(), bus.clone());
        tracing::info!(data = %paths.data.display(), "library opened");
        Ok(Self {
            inner: Arc::new(Inner {
                paths,
                catalog,
                jobs,
                bus,
                clock: options.clock,
                thumbs,
                folders: FolderManager::new(scanner),
                history,
                scan_cancel: AtomicBool::new(false),
                edit_lock: Mutex::new(()),
                watcher: Mutex::new(None),
            }),
        })
    }

    /// A new receiver of application events.
    pub fn events(&self) -> Receiver<AppEvent> {
        self.inner.bus.subscribe()
    }

    /// The event bus, for publishing and for tests that tick it manually.
    pub fn bus(&self) -> &EventBus {
        &self.inner.bus
    }

    /// Where this library keeps its files.
    pub fn paths(&self) -> &AppPaths {
        &self.inner.paths
    }

    /// The job queue background work runs on.
    pub fn jobs(&self) -> &JobQueue {
        &self.inner.jobs
    }

    /// The clock in use.
    pub fn clock(&self) -> &Arc<dyn Clock> {
        &self.inner.clock
    }

    /// App-level undo for organize actions (SPEC §7.4).
    pub fn history(&self) -> HistoryService {
        self.inner.history.clone()
    }

    /// The catalog handle, for tests and history closures. UI code must go through services.
    #[doc(hidden)]
    pub fn catalog_handle(&self) -> &Catalog {
        &self.inner.catalog
    }

    pub(crate) fn catalog(&self) -> &Catalog {
        &self.inner.catalog
    }

    pub(crate) fn emit(&self, event: AppEvent) {
        self.inner.bus.emit(event);
    }
}
