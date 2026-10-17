//! Job trait and the types around it.

use lb_core::JobId;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Scheduling class of a job (SPEC §4.3). `Interactive` runs before `Foreground`, which runs
/// before `Background`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum Priority {
    /// Visible thumbnails, viewer decode: the user is waiting right now.
    Interactive,
    /// User-requested work such as export and import.
    Foreground,
    /// Scanning, hashing, pre-generation.
    Background,
}

/// Why a job did not complete successfully.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum JobError {
    /// The job was cancelled before or while running.
    #[error("job cancelled")]
    Cancelled,
    /// The job reported a failure.
    #[error("job failed: {0}")]
    Failed(String),
    /// The job panicked; the panic was contained.
    #[error("job panicked: {0}")]
    Panicked(String),
}

/// Outcome of a job.
pub type JobResult = Result<(), JobError>;

/// Notifications about running jobs, delivered to the observer set with
/// [`crate::JobQueue::set_observer`].
#[derive(Debug, Clone, PartialEq)]
pub enum JobEvent {
    /// A job reported progress in `0.0..=1.0`.
    Progress {
        /// Job id.
        id: JobId,
        /// Job kind.
        kind: &'static str,
        /// Fraction done.
        progress: f32,
    },
    /// A job ended (successfully, failed, panicked or cancelled).
    Finished {
        /// Job id.
        id: JobId,
        /// Job kind.
        kind: &'static str,
        /// Outcome.
        result: JobResult,
    },
}

/// A unit of background work.
pub trait Job: Send + 'static {
    /// Short stable name used in logs and events (`"thumbnail"`, `"scan"`, ...).
    fn kind(&self) -> &'static str;

    /// Jobs submitted with an equal key while one is queued or running are coalesced into the
    /// existing job (SPEC §6.5). `None` disables coalescing.
    fn coalesce_key(&self) -> Option<String> {
        None
    }

    /// Does the work. Long jobs should poll [`JobCtx::is_cancelled`] and return
    /// [`JobError::Cancelled`].
    fn run(self: Box<Self>, ctx: &JobCtx) -> JobResult;
}

/// Context handed to a running job.
pub struct JobCtx {
    pub(crate) id: JobId,
    pub(crate) kind: &'static str,
    pub(crate) cancel: Arc<AtomicBool>,
    pub(crate) sink: Arc<dyn Fn(JobEvent) + Send + Sync>,
}

impl JobCtx {
    /// Id of the running job.
    pub fn id(&self) -> JobId {
        self.id
    }

    /// Whether cancellation was requested.
    pub fn is_cancelled(&self) -> bool {
        self.cancel.load(Ordering::Relaxed)
    }

    /// Reports progress (clamped to `0.0..=1.0`).
    pub fn progress(&self, fraction: f32) {
        (self.sink)(JobEvent::Progress {
            id: self.id,
            kind: self.kind,
            progress: fraction.clamp(0.0, 1.0),
        });
    }
}
