//! The queue: a priority heap served by worker threads, or run inline.

use crate::{Job, JobCtx, JobError, JobEvent, JobResult, Priority};
use lb_core::JobId;
use parking_lot::{Condvar, Mutex};
use std::collections::{BTreeMap, HashMap};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

type Observer = Arc<dyn Fn(JobEvent) + Send + Sync>;

struct Shared {
    done: Mutex<Option<JobResult>>,
    cv: Condvar,
}

/// A handle to a submitted job.
#[derive(Clone)]
pub struct JobHandle {
    /// Id of the job.
    pub id: JobId,
    cancel: Arc<AtomicBool>,
    shared: Arc<Shared>,
}

impl JobHandle {
    /// Requests cancellation. A queued job never starts; a running job stops at its next
    /// [`JobCtx::is_cancelled`] check.
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// The job's result once it ended.
    pub fn result(&self) -> Option<JobResult> {
        self.shared.done.lock().clone()
    }

    /// Whether the job ended.
    pub fn is_finished(&self) -> bool {
        self.shared.done.lock().is_some()
    }

    /// Waits up to `timeout` for the job to end and returns its result.
    pub fn wait(&self, timeout: Duration) -> Option<JobResult> {
        let deadline = Instant::now() + timeout;
        let mut done = self.shared.done.lock();
        while done.is_none() {
            if self.shared.cv.wait_until(&mut done, deadline).timed_out() {
                break;
            }
        }
        done.clone()
    }
}

struct Entry {
    id: JobId,
    kind: &'static str,
    key: Option<String>,
    job: Box<dyn Job>,
    cancel: Arc<AtomicBool>,
    shared: Arc<Shared>,
}

struct Tracked {
    cancel: Arc<AtomicBool>,
    shared: Arc<Shared>,
    key: Option<String>,
    priority: Priority,
    seq: Option<u64>,
}

#[derive(Default)]
struct State {
    queue: BTreeMap<(Priority, u64), Entry>,
    tracked: HashMap<JobId, Tracked>,
    keys: HashMap<String, JobId>,
    running: usize,
    next_seq: u64,
    shutdown: bool,
}

struct Inner {
    state: Mutex<State>,
    work: Condvar,
    idle: Condvar,
    next_id: AtomicI64,
    observer: Mutex<Observer>,
    inline: bool,
}

/// Runs jobs by priority on worker threads, or synchronously in tests.
#[derive(Clone)]
pub struct JobQueue {
    inner: Arc<Inner>,
    #[allow(dead_code)]
    workers: Arc<WorkerGuard>,
}

struct WorkerGuard {
    inner: Arc<Inner>,
    threads: Mutex<Vec<JoinHandle<()>>>,
}

impl Drop for WorkerGuard {
    fn drop(&mut self) {
        {
            let mut st = self.inner.state.lock();
            st.shutdown = true;
            for t in st.tracked.values() {
                t.cancel.store(true, Ordering::Relaxed);
            }
            let queued = std::mem::take(&mut st.queue);
            for (_, entry) in queued {
                finish_entry_cancelled(&entry);
            }
        }
        self.inner.work.notify_all();
        for t in self.threads.lock().drain(..) {
            let _ = t.join();
        }
    }
}

fn finish_entry_cancelled(entry: &Entry) {
    *entry.shared.done.lock() = Some(Err(JobError::Cancelled));
    entry.shared.cv.notify_all();
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "unknown panic".to_owned())
}

impl JobQueue {
    fn build(workers: usize, inline: bool) -> Self {
        let inner = Arc::new(Inner {
            state: Mutex::new(State::default()),
            work: Condvar::new(),
            idle: Condvar::new(),
            next_id: AtomicI64::new(1),
            observer: Mutex::new(Arc::new(|_| {})),
            inline,
        });
        let mut threads = Vec::new();
        if !inline {
            for n in 0..workers.max(1) {
                let inner = inner.clone();
                let spawned = std::thread::Builder::new()
                    .name(format!("lb-job-{n}"))
                    .spawn(move || worker(&inner));
                match spawned {
                    Ok(t) => threads.push(t),
                    Err(e) => tracing::error!(error = %e, "cannot spawn job worker"),
                }
            }
        }
        Self {
            workers: Arc::new(WorkerGuard {
                inner: inner.clone(),
                threads: Mutex::new(threads),
            }),
            inner,
        }
    }

    /// A queue served by `workers` threads (at least one).
    pub fn threaded(workers: usize) -> Self {
        Self::build(workers, false)
    }

    /// A queue that runs each job on the calling thread inside [`JobQueue::submit`]; used by
    /// tests and the CLI for determinism.
    pub fn inline() -> Self {
        Self::build(0, true)
    }

    /// Worker count for this machine: cores minus one, at least one (SPEC §4.3).
    pub fn default_workers() -> usize {
        std::thread::available_parallelism()
            .map_or(1, std::num::NonZero::get)
            .saturating_sub(1)
            .max(1)
    }

    /// Receives progress and completion events of every job.
    pub fn set_observer(&self, observer: impl Fn(JobEvent) + Send + Sync + 'static) {
        *self.inner.observer.lock() = Arc::new(observer);
    }

    /// Submits `job`. If a queued or running job has the same [`Job::coalesce_key`], its handle
    /// is returned instead (raising its priority when `priority` is higher).
    pub fn submit(&self, job: impl Job, priority: Priority) -> JobHandle {
        let key = job.coalesce_key();
        let kind = job.kind();
        let mut st = self.inner.state.lock();
        if let Some(existing) = key.as_ref().and_then(|k| st.keys.get(k)).copied() {
            let handle = st.tracked.get(&existing).map(|t| JobHandle {
                id: existing,
                cancel: t.cancel.clone(),
                shared: t.shared.clone(),
            });
            if let Some(handle) = handle {
                Self::raise(&mut st, existing, priority);
                return handle;
            }
        }
        let id = JobId::new(self.inner.next_id.fetch_add(1, Ordering::Relaxed));
        let cancel = Arc::new(AtomicBool::new(false));
        let shared = Arc::new(Shared {
            done: Mutex::new(None),
            cv: Condvar::new(),
        });
        let handle = JobHandle {
            id,
            cancel: cancel.clone(),
            shared: shared.clone(),
        };
        let seq = st.next_seq;
        st.next_seq += 1;
        st.tracked.insert(
            id,
            Tracked {
                cancel: cancel.clone(),
                shared: shared.clone(),
                key: key.clone(),
                priority,
                seq: Some(seq),
            },
        );
        if let Some(k) = &key {
            st.keys.insert(k.clone(), id);
        }
        let entry = Entry {
            id,
            kind,
            key,
            job: Box::new(job),
            cancel,
            shared,
        };
        if self.inner.inline {
            st.tracked.entry(id).and_modify(|t| t.seq = None);
            st.running += 1;
            drop(st);
            run_entry(&self.inner, entry);
        } else {
            st.queue.insert((priority, seq), entry);
            drop(st);
            self.inner.work.notify_one();
        }
        handle
    }

    fn raise(st: &mut State, id: JobId, priority: Priority) {
        let Some(t) = st.tracked.get_mut(&id) else {
            return;
        };
        let (Some(seq), true) = (t.seq, priority < t.priority) else {
            return;
        };
        if let Some(entry) = st.queue.remove(&(t.priority, seq)) {
            t.priority = priority;
            st.queue.insert((priority, seq), entry);
        }
    }

    /// Changes the priority of a queued job (no effect once it started).
    pub fn reprioritize(&self, id: JobId, priority: Priority) {
        let mut st = self.inner.state.lock();
        let Some(t) = st.tracked.get(&id) else {
            return;
        };
        let (Some(seq), old) = (t.seq, t.priority) else {
            return;
        };
        if let Some(entry) = st.queue.remove(&(old, seq)) {
            if let Some(t) = st.tracked.get_mut(&id) {
                t.priority = priority;
            }
            st.queue.insert((priority, seq), entry);
        }
    }

    /// Cancels a job by id: a queued job is dropped without running, a running one is asked to
    /// stop.
    pub fn cancel(&self, id: JobId) {
        let mut st = self.inner.state.lock();
        let Some(t) = st.tracked.get(&id) else {
            return;
        };
        t.cancel.store(true, Ordering::Relaxed);
        let (priority, seq, key) = (t.priority, t.seq, t.key.clone());
        let Some(seq) = seq else { return };
        if let Some(entry) = st.queue.remove(&(priority, seq)) {
            st.tracked.remove(&id);
            if let Some(k) = key {
                st.keys.remove(&k);
            }
            finish_entry_cancelled(&entry);
            let observer = self.inner.observer.lock().clone();
            observer(JobEvent::Finished {
                id,
                kind: entry.kind,
                result: Err(JobError::Cancelled),
            });
            if st.queue.is_empty() && st.running == 0 {
                self.inner.idle.notify_all();
            }
        }
    }

    /// Number of jobs waiting to start.
    pub fn queued(&self) -> usize {
        self.inner.state.lock().queue.len()
    }

    /// Waits until no job is queued or running. Returns `false` on timeout.
    pub fn wait_idle(&self, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let mut st = self.inner.state.lock();
        while !(st.queue.is_empty() && st.running == 0) {
            if self.inner.idle.wait_until(&mut st, deadline).timed_out() {
                return st.queue.is_empty() && st.running == 0;
            }
        }
        true
    }
}

fn worker(inner: &Arc<Inner>) {
    loop {
        let entry = {
            let mut st = inner.state.lock();
            loop {
                if st.shutdown {
                    return;
                }
                if let Some((&key, _)) = st.queue.iter().next() {
                    let Some(entry) = st.queue.remove(&key) else {
                        continue;
                    };
                    if let Some(t) = st.tracked.get_mut(&entry.id) {
                        t.seq = None;
                    }
                    st.running += 1;
                    break entry;
                }
                inner.work.wait(&mut st);
            }
        };
        run_entry(inner, entry);
    }
}

fn run_entry(inner: &Arc<Inner>, entry: Entry) {
    let observer = inner.observer.lock().clone();
    let ctx = JobCtx {
        id: entry.id,
        kind: entry.kind,
        cancel: entry.cancel.clone(),
        sink: observer.clone(),
    };
    let result = if entry.cancel.load(Ordering::Relaxed) {
        Err(JobError::Cancelled)
    } else {
        match catch_unwind(AssertUnwindSafe(|| entry.job.run(&ctx))) {
            Ok(r) => r,
            Err(payload) => {
                let message = panic_message(payload.as_ref());
                tracing::error!(kind = entry.kind, %message, "job panicked");
                Err(JobError::Panicked(message))
            }
        }
    };
    *entry.shared.done.lock() = Some(result.clone());
    entry.shared.cv.notify_all();
    observer(JobEvent::Finished {
        id: entry.id,
        kind: entry.kind,
        result,
    });
    let mut st = inner.state.lock();
    st.tracked.remove(&entry.id);
    if let Some(k) = &entry.key
        && st.keys.get(k) == Some(&entry.id)
    {
        st.keys.remove(k);
    }
    st.running -= 1;
    if st.queue.is_empty() && st.running == 0 {
        inner.idle.notify_all();
    }
}
