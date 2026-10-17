//! Application events and the coalescing event bus (SPEC §7.3).

use crossbeam_channel::{Receiver, Sender};
use indexmap::{IndexMap, IndexSet};
use lb_core::{Clock, JobId, PhotoId, RootId};
use lb_thumbs::ThumbSize;
use parking_lot::Mutex;
use std::sync::{Arc, Weak};
use std::time::Duration;

/// Window within which `PhotosChanged` and `ThumbReady` events are merged.
pub const COALESCE_WINDOW: Duration = Duration::from_millis(16);

/// Something that happened in the library that the UI may want to react to.
#[derive(Debug, Clone, PartialEq)]
pub enum AppEvent {
    /// A scan is running.
    ScanProgress {
        /// Root being scanned.
        root: RootId,
        /// Files processed so far.
        done: usize,
        /// Best guess of the total.
        total_estimate: usize,
    },
    /// A scan ended.
    ScanFinished {
        /// Root that was scanned.
        root: RootId,
        /// Rows inserted.
        added: usize,
        /// Rows updated.
        updated: usize,
        /// Rows marked missing.
        missing: usize,
        /// Files that could not be read.
        errors: usize,
    },
    /// Metadata of these photos changed.
    PhotosChanged {
        /// Affected photos.
        ids: Vec<PhotoId>,
    },
    /// Photos entered the catalog.
    PhotosAdded {
        /// New photos.
        ids: Vec<PhotoId>,
    },
    /// Photos left the catalog.
    PhotosRemoved {
        /// Removed photos.
        ids: Vec<PhotoId>,
    },
    /// A photo's edit stack changed.
    PhotoEdited {
        /// The photo.
        id: PhotoId,
        /// Its new `edit_version`.
        version: u32,
    },
    /// A thumbnail finished rendering.
    ThumbReady {
        /// The photo.
        id: PhotoId,
        /// Size that is now cached.
        size: ThumbSize,
        /// `edit_version` the thumbnail reflects.
        version: u32,
    },
    /// Albums were created, renamed, deleted or their contents changed.
    AlbumsChanged,
    /// Tags were created, renamed, deleted or (un)assigned.
    TagsChanged,
    /// People changed.
    PeopleChanged,
    /// The folder tree changed.
    FoldersChanged,
    /// A background job made progress.
    JobProgress {
        /// The job.
        id: JobId,
        /// Job kind.
        kind: &'static str,
        /// Completion in `0.0..=1.0`.
        progress: f32,
        /// Human-readable description.
        label: String,
    },
    /// A background job ended.
    JobFinished {
        /// The job.
        id: JobId,
        /// `Err` carries the failure message.
        result: Result<(), String>,
    },
    /// A setting changed.
    SettingsChanged {
        /// The setting key.
        key: String,
    },
    /// Something failed in the background.
    Error {
        /// What was being attempted.
        context: String,
        /// What went wrong, for display.
        message: String,
    },
}

#[derive(Default)]
struct Pending {
    changed: IndexSet<PhotoId>,
    thumbs: IndexMap<(PhotoId, ThumbSize), u32>,
    since: Option<Duration>,
}

impl Pending {
    fn is_empty(&self) -> bool {
        self.changed.is_empty() && self.thumbs.is_empty()
    }
}

struct State {
    subscribers: Vec<Sender<AppEvent>>,
    pending: Pending,
}

impl State {
    fn send(&mut self, event: &AppEvent) {
        self.subscribers.retain(|s| s.send(event.clone()).is_ok());
    }

    fn flush(&mut self) {
        let pending = std::mem::take(&mut self.pending);
        if !pending.changed.is_empty() {
            self.send(&AppEvent::PhotosChanged {
                ids: pending.changed.into_iter().collect(),
            });
        }
        for ((id, size), version) in pending.thumbs {
            self.send(&AppEvent::ThumbReady { id, size, version });
        }
    }
}

struct Inner {
    clock: Arc<dyn Clock>,
    state: Mutex<State>,
}

impl Inner {
    fn flush_if_due(&self, state: &mut State) {
        if let Some(since) = state.pending.since
            && self.clock.elapsed().saturating_sub(since) >= COALESCE_WINDOW
        {
            state.flush();
        }
    }
}

/// Fan-out of [`AppEvent`]s to any number of subscribers.
///
/// `PhotosChanged` events are merged (union of ids) and `ThumbReady` events are deduplicated per
/// photo and size; both are delivered once the 16 ms window opened by the first of them has
/// elapsed on the injected clock, driven by [`EventBus::tick`]. Every other event is delivered
/// immediately, after any pending coalesced events so ordering is preserved.
#[derive(Clone)]
pub struct EventBus {
    inner: Arc<Inner>,
}

impl EventBus {
    /// A bus measuring windows on `clock`. Call [`EventBus::tick`] to deliver due events, or
    /// [`EventBus::spawn_ticker`] to do so on a background thread.
    pub fn new(clock: Arc<dyn Clock>) -> Self {
        Self {
            inner: Arc::new(Inner {
                clock,
                state: Mutex::new(State {
                    subscribers: Vec::new(),
                    pending: Pending::default(),
                }),
            }),
        }
    }

    /// A new receiver that sees every event emitted from now on.
    pub fn subscribe(&self) -> Receiver<AppEvent> {
        let (tx, rx) = crossbeam_channel::unbounded();
        self.inner.state.lock().subscribers.push(tx);
        rx
    }

    /// Publishes an event.
    pub fn emit(&self, event: AppEvent) {
        let mut state = self.inner.state.lock();
        match event {
            AppEvent::PhotosChanged { ids } => {
                state.pending.changed.extend(ids);
                self.open_window(&mut state);
            }
            AppEvent::ThumbReady { id, size, version } => {
                state.pending.thumbs.insert((id, size), version);
                self.open_window(&mut state);
            }
            other => {
                state.flush();
                state.send(&other);
            }
        }
        self.inner.flush_if_due(&mut state);
    }

    fn open_window(&self, state: &mut State) {
        if state.pending.since.is_none() {
            state.pending.since = Some(self.inner.clock.elapsed());
        }
    }

    /// Delivers coalesced events whose window has elapsed.
    pub fn tick(&self) {
        let mut state = self.inner.state.lock();
        self.inner.flush_if_due(&mut state);
    }

    /// Delivers every coalesced event immediately, regardless of the window.
    pub fn flush(&self) {
        self.inner.state.lock().flush();
    }

    /// Whether coalesced events are waiting for their window to elapse.
    pub fn has_pending(&self) -> bool {
        !self.inner.state.lock().pending.is_empty()
    }

    /// Starts a thread that ticks the bus every 8 ms until the last clone of the bus is dropped.
    pub fn spawn_ticker(&self) -> std::io::Result<()> {
        let weak: Weak<Inner> = Arc::downgrade(&self.inner);
        std::thread::Builder::new()
            .name("lb-event-ticker".into())
            .spawn(move || {
                while let Some(inner) = weak.upgrade() {
                    {
                        let mut state = inner.state.lock();
                        inner.flush_if_due(&mut state);
                    }
                    drop(inner);
                    std::thread::sleep(Duration::from_millis(8));
                }
            })
            .map(drop)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lb_testkit::FakeClock;

    fn bus() -> (EventBus, Arc<FakeClock>, Receiver<AppEvent>) {
        let clock = Arc::new(FakeClock::new());
        let bus = EventBus::new(clock.clone());
        let rx = bus.subscribe();
        (bus, clock, rx)
    }

    fn ids(v: &[i64]) -> Vec<PhotoId> {
        v.iter().map(|&i| PhotoId::new(i)).collect()
    }

    #[test]
    fn events_coalesced_within_16ms() {
        let (bus, clock, rx) = bus();
        for i in 0..100 {
            bus.emit(AppEvent::PhotosChanged {
                ids: ids(&[i % 10, 100 + i]),
            });
            bus.emit(AppEvent::ThumbReady {
                id: PhotoId::new(i % 3),
                size: ThumbSize::S,
                version: u32::try_from(i).unwrap_or(0),
            });
            clock.advance(Duration::from_micros(100));
            bus.tick();
        }
        assert!(
            rx.try_recv().is_err(),
            "nothing delivered inside the window"
        );
        assert!(bus.has_pending());

        clock.advance(COALESCE_WINDOW);
        bus.tick();

        let events: Vec<_> = rx.try_iter().collect();
        assert_eq!(
            events.len(),
            1 + 3,
            "one PhotosChanged and one ThumbReady per photo"
        );
        let AppEvent::PhotosChanged { ids: merged } = &events[0] else {
            panic!("first event must be PhotosChanged, got {:?}", events[0]);
        };
        assert_eq!(merged.len(), 10 + 100, "union of ids, no duplicates");
        let latest: Vec<_> = events[1..]
            .iter()
            .map(|e| match e {
                AppEvent::ThumbReady { id, version, .. } => (id.get(), *version),
                other => panic!("unexpected {other:?}"),
            })
            .collect();
        assert!(
            latest.contains(&(0, 99)) && latest.contains(&(1, 97)) && latest.contains(&(2, 98))
        );
        assert!(!bus.has_pending());
    }

    #[test]
    fn window_opens_at_first_event_and_resets_after_flush() {
        let (bus, clock, rx) = bus();
        bus.emit(AppEvent::PhotosChanged { ids: ids(&[1]) });
        clock.advance(Duration::from_millis(20));
        bus.emit(AppEvent::PhotosChanged { ids: ids(&[2]) });
        let first: Vec<_> = rx.try_iter().collect();
        assert_eq!(first, vec![AppEvent::PhotosChanged { ids: ids(&[1, 2]) }]);
        clock.advance(Duration::from_millis(5));
        bus.emit(AppEvent::PhotosChanged { ids: ids(&[3]) });
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn other_events_are_immediate_and_keep_order() {
        let (bus, _clock, rx) = bus();
        bus.emit(AppEvent::PhotosChanged { ids: ids(&[1]) });
        bus.emit(AppEvent::AlbumsChanged);
        let got: Vec<_> = rx.try_iter().collect();
        assert_eq!(
            got,
            vec![
                AppEvent::PhotosChanged { ids: ids(&[1]) },
                AppEvent::AlbumsChanged
            ]
        );
    }

    #[test]
    fn every_subscriber_gets_every_event_and_dropped_ones_are_pruned() {
        let (bus, _clock, rx1) = bus();
        let rx2 = bus.subscribe();
        drop(bus.subscribe());
        bus.emit(AppEvent::TagsChanged);
        bus.emit(AppEvent::FoldersChanged);
        assert_eq!(rx1.try_iter().count(), 2);
        assert_eq!(rx2.try_iter().count(), 2);
        assert_eq!(bus.inner.state.lock().subscribers.len(), 2);
    }

    #[test]
    fn flush_delivers_immediately() {
        let (bus, _clock, rx) = bus();
        bus.emit(AppEvent::PhotosChanged { ids: ids(&[7]) });
        bus.flush();
        assert_eq!(rx.try_iter().count(), 1);
    }

    #[test]
    fn ticker_delivers_with_the_system_clock() {
        let bus = EventBus::new(Arc::new(lb_core::SystemClock::new()));
        let rx = bus.subscribe();
        bus.spawn_ticker().unwrap();
        bus.emit(AppEvent::PhotosChanged { ids: ids(&[1]) });
        let got = rx.recv_timeout(Duration::from_secs(2));
        assert_eq!(got, Ok(AppEvent::PhotosChanged { ids: ids(&[1]) }));
    }
}
