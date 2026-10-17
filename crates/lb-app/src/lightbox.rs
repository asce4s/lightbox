//! The facade the UI and CLI talk to (SPEC §7.1).

use crate::error::Result;
use crate::events::{AppEvent, EventBus};
use crate::history::HistoryService;
use crate::paths::AppPaths;
use crate::system::{SystemOpener, SystemTrash};
use crossbeam_channel::Receiver;
use lb_catalog::Catalog;
use lb_core::{Clock, Opener, SystemClock, TrashBackend};
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
    /// Where deleted photos go.
    pub trash: Arc<dyn TrashBackend>,
    /// How files are opened and revealed.
    pub opener: Arc<dyn Opener>,
    /// Source of map tiles; `None` fetches from the URL in the settings. Tests inject a local one.
    #[cfg(feature = "map")]
    pub tile_provider: Option<Arc<dyn lb_geo::TileProvider>>,
    /// Face detector and embedder; `None` loads the models from the models directory. Tests
    /// inject the mocks.
    #[cfg(feature = "faces")]
    pub face_pipeline: Option<Arc<lb_faces::FacePipeline>>,
}

impl Default for LightboxOptions {
    fn default() -> Self {
        Self {
            clock: Arc::new(SystemClock::new()),
            spawn_event_ticker: true,
            trash: Arc::new(SystemTrash),
            opener: Arc::new(SystemOpener),
            #[cfg(feature = "map")]
            tile_provider: None,
            #[cfg(feature = "faces")]
            face_pipeline: None,
        }
    }
}

pub(crate) struct Inner {
    pub(crate) paths: AppPaths,
    pub(crate) catalog: Catalog,
    pub(crate) jobs: JobQueue,
    pub(crate) bus: EventBus,
    pub(crate) clock: Arc<dyn Clock>,
    pub(crate) trash: Arc<dyn TrashBackend>,
    pub(crate) opener: Arc<dyn Opener>,
    pub(crate) thumbs: ThumbCache,
    pub(crate) folders: FolderManager,
    pub(crate) history: HistoryService,
    pub(crate) scan_cancel: AtomicBool,
    pub(crate) edit_lock: Mutex<()>,
    pub(crate) watcher: Mutex<Option<WatchService>>,
    #[cfg(feature = "map")]
    pub(crate) map: crate::map::MapState,
    #[cfg(feature = "faces")]
    pub(crate) faces: crate::faces::FaceState,
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
        #[cfg(feature = "video")]
        let video = crate::video::detect(&catalog);
        #[cfg(feature = "video")]
        let thumbs = match &video {
            Some(tools) => ThumbCache::with_decoder(
                &paths.cache,
                Arc::new(lb_video::VideoDecoder::new(tools.clone())),
            )?,
            None => ThumbCache::open(&paths.cache)?,
        };
        #[cfg(not(feature = "video"))]
        let thumbs = ThumbCache::open(&paths.cache)?;
        #[allow(unused_mut)]
        let mut scan_options = ScanOptions::default();
        #[cfg(feature = "video")]
        {
            scan_options.caps.video = true;
            scan_options.video_prober = video
                .clone()
                .map(|t| Arc::new(t) as Arc<dyn lb_media::VideoProber>);
        }
        let clock_for_scan = options.clock.clone();
        let scanner =
            Scanner::with_clock(catalog.clone(), scan_options, move || clock_for_scan.now());
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
                trash: options.trash,
                opener: options.opener,
                thumbs,
                folders: FolderManager::new(scanner),
                history,
                scan_cancel: AtomicBool::new(false),
                edit_lock: Mutex::new(()),
                watcher: Mutex::new(None),
                #[cfg(feature = "map")]
                map: crate::map::MapState::new(options.tile_provider),
                #[cfg(feature = "faces")]
                faces: crate::faces::FaceState::new(options.face_pipeline),
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
