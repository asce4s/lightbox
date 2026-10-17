use super::*;
use parking_lot::Mutex;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::time::Duration;

const T: Duration = Duration::from_secs(10);

struct Record {
    name: &'static str,
    log: Arc<Mutex<Vec<&'static str>>>,
}

impl Job for Record {
    fn kind(&self) -> &'static str {
        "record"
    }
    fn run(self: Box<Self>, _: &JobCtx) -> JobResult {
        self.log.lock().push(self.name);
        Ok(())
    }
}

struct Gate {
    started: mpsc::Sender<()>,
    release: mpsc::Receiver<()>,
}

impl Job for Gate {
    fn kind(&self) -> &'static str {
        "gate"
    }
    fn run(self: Box<Self>, _: &JobCtx) -> JobResult {
        let _ = self.started.send(());
        let _ = self.release.recv_timeout(T);
        Ok(())
    }
}

/// Occupies the single worker until released, so later submissions queue up.
fn block_worker(q: &JobQueue) -> (mpsc::Sender<()>, JobHandle) {
    let (started_tx, started_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let handle = q.submit(
        Gate {
            started: started_tx,
            release: release_rx,
        },
        Priority::Interactive,
    );
    started_rx.recv_timeout(T).unwrap();
    (release_tx, handle)
}

#[test]
fn jobs_priority_order() {
    let q = JobQueue::threaded(1);
    let (release, gate) = block_worker(&q);
    let log = Arc::new(Mutex::new(Vec::new()));
    let rec = |name| Record {
        name,
        log: log.clone(),
    };
    q.submit(rec("bg1"), Priority::Background);
    q.submit(rec("fg1"), Priority::Foreground);
    q.submit(rec("bg2"), Priority::Background);
    q.submit(rec("int1"), Priority::Interactive);
    q.submit(rec("fg2"), Priority::Foreground);
    q.submit(rec("int2"), Priority::Interactive);
    assert_eq!(q.queued(), 6);
    release.send(()).unwrap();
    assert!(q.wait_idle(T));
    assert_eq!(*log.lock(), ["int1", "int2", "fg1", "fg2", "bg1", "bg2"]);
    assert_eq!(gate.result(), Some(Ok(())));
}

#[test]
fn jobs_reprioritize_moves_a_queued_job() {
    let q = JobQueue::threaded(1);
    let (release, _gate) = block_worker(&q);
    let log = Arc::new(Mutex::new(Vec::new()));
    let rec = |name| Record {
        name,
        log: log.clone(),
    };
    q.submit(rec("a"), Priority::Background);
    let b = q.submit(rec("b"), Priority::Background);
    q.reprioritize(b.id, Priority::Interactive);
    release.send(()).unwrap();
    assert!(q.wait_idle(T));
    assert_eq!(*log.lock(), ["b", "a"]);
}

#[test]
fn jobs_cancel_before_start() {
    let q = JobQueue::threaded(1);
    let (release, _gate) = block_worker(&q);
    let log = Arc::new(Mutex::new(Vec::new()));
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    q.set_observer(move |e| sink.lock().push(e));
    let doomed = q.submit(
        Record {
            name: "doomed",
            log: log.clone(),
        },
        Priority::Background,
    );
    q.submit(
        Record {
            name: "kept",
            log: log.clone(),
        },
        Priority::Background,
    );
    q.cancel(doomed.id);
    assert_eq!(doomed.result(), Some(Err(JobError::Cancelled)));
    release.send(()).unwrap();
    assert!(q.wait_idle(T));
    assert_eq!(*log.lock(), ["kept"]);
    assert!(events.lock().iter().any(|e| matches!(
        e,
        JobEvent::Finished { id, result: Err(JobError::Cancelled), .. } if *id == doomed.id
    )));
}

struct Spin {
    started: mpsc::Sender<()>,
    iterations: Arc<AtomicUsize>,
}

impl Job for Spin {
    fn kind(&self) -> &'static str {
        "spin"
    }
    fn run(self: Box<Self>, ctx: &JobCtx) -> JobResult {
        let _ = self.started.send(());
        loop {
            if ctx.is_cancelled() {
                return Err(JobError::Cancelled);
            }
            self.iterations.fetch_add(1, Ordering::Relaxed);
            std::thread::sleep(Duration::from_millis(1));
        }
    }
}

#[test]
fn jobs_cancel_cooperative() {
    let q = JobQueue::threaded(2);
    let (tx, rx) = mpsc::channel();
    let iterations = Arc::new(AtomicUsize::new(0));
    let h = q.submit(
        Spin {
            started: tx,
            iterations: iterations.clone(),
        },
        Priority::Foreground,
    );
    rx.recv_timeout(T).unwrap();
    assert!(!h.is_finished());
    h.cancel();
    assert_eq!(h.wait(T), Some(Err(JobError::Cancelled)));
    let after = iterations.load(Ordering::Relaxed);
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(iterations.load(Ordering::Relaxed), after, "job stopped");
    assert!(q.wait_idle(T));
}

struct Keyed {
    key: &'static str,
    runs: Arc<AtomicUsize>,
}

impl Job for Keyed {
    fn kind(&self) -> &'static str {
        "keyed"
    }
    fn coalesce_key(&self) -> Option<String> {
        Some(self.key.to_owned())
    }
    fn run(self: Box<Self>, _: &JobCtx) -> JobResult {
        self.runs.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn jobs_coalesce_duplicates() {
    let q = JobQueue::threaded(1);
    let (release, _gate) = block_worker(&q);
    let runs = Arc::new(AtomicUsize::new(0));
    let log = Arc::new(Mutex::new(Vec::new()));
    let keyed = |key| Keyed {
        key,
        runs: runs.clone(),
    };
    let first = q.submit(keyed("thumb:1"), Priority::Background);
    q.submit(
        Record {
            name: "other-bg",
            log: log.clone(),
        },
        Priority::Background,
    );
    let second = q.submit(keyed("thumb:1"), Priority::Interactive);
    let distinct = q.submit(keyed("thumb:2"), Priority::Background);
    assert_eq!(first.id, second.id, "duplicate returns the existing handle");
    assert_ne!(first.id, distinct.id);
    assert_eq!(q.queued(), 3);
    release.send(()).unwrap();
    assert!(q.wait_idle(T));
    assert_eq!(runs.load(Ordering::SeqCst), 2, "one run per distinct key");
    assert_eq!(first.result(), Some(Ok(())));

    let again = q.submit(keyed("thumb:1"), Priority::Background);
    assert!(again.wait(T).is_some());
    assert_ne!(again.id, first.id, "a finished key can be submitted again");
    assert_eq!(runs.load(Ordering::SeqCst), 3);
}

#[test]
fn jobs_coalesce_with_a_running_job() {
    let q = JobQueue::threaded(2);
    let (tx, rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    struct KeyedGate(mpsc::Sender<()>, mpsc::Receiver<()>);
    impl Job for KeyedGate {
        fn kind(&self) -> &'static str {
            "kg"
        }
        fn coalesce_key(&self) -> Option<String> {
            Some("k".into())
        }
        fn run(self: Box<Self>, _: &JobCtx) -> JobResult {
            let _ = self.0.send(());
            let _ = self.1.recv_timeout(T);
            Ok(())
        }
    }
    let a = q.submit(KeyedGate(tx, release_rx), Priority::Background);
    rx.recv_timeout(T).unwrap();
    let (tx2, _rx2) = mpsc::channel();
    let (_r2, release_rx2) = mpsc::channel();
    let b = q.submit(KeyedGate(tx2, release_rx2), Priority::Interactive);
    assert_eq!(a.id, b.id);
    release_tx.send(()).unwrap();
    assert!(q.wait_idle(T));
}

struct Boom;

impl Job for Boom {
    fn kind(&self) -> &'static str {
        "boom"
    }
    fn run(self: Box<Self>, _: &JobCtx) -> JobResult {
        panic!("job exploded");
    }
}

#[test]
fn jobs_panic_is_contained() {
    for q in [JobQueue::threaded(1), JobQueue::inline()] {
        let h = q.submit(Boom, Priority::Foreground);
        let result = h.wait(T).unwrap();
        assert!(
            matches!(&result, Err(JobError::Panicked(m)) if m.contains("job exploded")),
            "{result:?}"
        );
        let log = Arc::new(Mutex::new(Vec::new()));
        let h = q.submit(
            Record {
                name: "after",
                log: log.clone(),
            },
            Priority::Foreground,
        );
        assert_eq!(h.wait(T), Some(Ok(())), "the queue keeps working");
        assert_eq!(*log.lock(), ["after"]);
        assert!(q.wait_idle(T));
    }
}

struct Failing;

impl Job for Failing {
    fn kind(&self) -> &'static str {
        "failing"
    }
    fn run(self: Box<Self>, ctx: &JobCtx) -> JobResult {
        ctx.progress(0.5);
        ctx.progress(7.0);
        Err(JobError::Failed("nope".into()))
    }
}

#[test]
fn jobs_report_progress_and_failure_to_the_observer() {
    let q = JobQueue::inline();
    let events = Arc::new(Mutex::new(Vec::new()));
    let sink = events.clone();
    q.set_observer(move |e| sink.lock().push(e));
    let h = q.submit(Failing, Priority::Foreground);
    assert_eq!(h.result(), Some(Err(JobError::Failed("nope".into()))));
    let events = events.lock();
    assert_eq!(events.len(), 3);
    assert!(
        matches!(events[0], JobEvent::Progress { progress, .. } if (progress - 0.5).abs() < f32::EPSILON)
    );
    assert!(
        matches!(events[1], JobEvent::Progress { progress, .. } if (progress - 1.0).abs() < f32::EPSILON),
        "clamped"
    );
    assert!(matches!(
        &events[2],
        JobEvent::Finished {
            kind: "failing",
            result: Err(JobError::Failed(_)),
            ..
        }
    ));
}

#[test]
fn jobs_wait_idle() {
    let q = JobQueue::threaded(1);
    assert!(q.wait_idle(Duration::from_millis(1)), "empty queue is idle");
    let (release, _gate) = block_worker(&q);
    assert!(
        !q.wait_idle(Duration::from_millis(30)),
        "busy queue times out"
    );
    release.send(()).unwrap();
    assert!(q.wait_idle(T));
    let done = Arc::new(AtomicBool::new(false));
    struct Flag(Arc<AtomicBool>);
    impl Job for Flag {
        fn kind(&self) -> &'static str {
            "flag"
        }
        fn run(self: Box<Self>, _: &JobCtx) -> JobResult {
            std::thread::sleep(Duration::from_millis(30));
            self.0.store(true, Ordering::SeqCst);
            Ok(())
        }
    }
    q.submit(Flag(done.clone()), Priority::Background);
    assert!(q.wait_idle(T));
    assert!(
        done.load(Ordering::SeqCst),
        "wait_idle returns only after jobs finished"
    );
}

#[test]
fn inline_queue_runs_on_the_calling_thread() {
    let q = JobQueue::inline();
    let caller = std::thread::current().id();
    struct Where(Arc<Mutex<Option<std::thread::ThreadId>>>);
    impl Job for Where {
        fn kind(&self) -> &'static str {
            "where"
        }
        fn run(self: Box<Self>, _: &JobCtx) -> JobResult {
            *self.0.lock() = Some(std::thread::current().id());
            Ok(())
        }
    }
    let slot = Arc::new(Mutex::new(None));
    let h = q.submit(Where(slot.clone()), Priority::Background);
    assert!(h.is_finished(), "finished before submit returns");
    assert_eq!(*slot.lock(), Some(caller));
    assert!(q.wait_idle(Duration::from_millis(1)));
}

#[test]
fn dropping_the_queue_cancels_pending_jobs_and_joins_workers() {
    let q = JobQueue::threaded(1);
    let (release, _gate) = block_worker(&q);
    let log = Arc::new(Mutex::new(Vec::new()));
    let pending = q.submit(
        Record {
            name: "never",
            log: log.clone(),
        },
        Priority::Background,
    );
    release.send(()).unwrap();
    drop(q);
    let result = pending.wait(T);
    assert!(
        result == Some(Err(JobError::Cancelled)) || result == Some(Ok(())),
        "either ran before shutdown or was cancelled: {result:?}"
    );
}
