//! Prioritised job queue with cancellation, coalescing and an inline test mode (SPEC §6.5).
#![warn(missing_docs)]

mod job;
mod queue;

pub use job::{Job, JobCtx, JobError, JobEvent, JobResult, Priority};
pub use queue::{JobHandle, JobQueue};

#[cfg(test)]
mod tests;
