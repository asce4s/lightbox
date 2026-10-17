//! Injectable time source so services and tests agree on "now".

use chrono::NaiveDateTime;
use std::time::{Duration, Instant};

/// A wall clock plus a monotonic timer.
pub trait Clock: Send + Sync {
    /// Local wall-clock time, stored in the catalog as ISO-8601 (SPEC §5.2).
    fn now(&self) -> NaiveDateTime;

    /// Time since an arbitrary fixed origin; never decreases. Used for coalescing windows.
    fn elapsed(&self) -> Duration;
}

/// The real system clock.
#[derive(Debug)]
pub struct SystemClock {
    origin: Instant,
}

impl SystemClock {
    /// A clock whose monotonic origin is now.
    pub fn new() -> Self {
        Self {
            origin: Instant::now(),
        }
    }
}

impl Default for SystemClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for SystemClock {
    fn now(&self) -> NaiveDateTime {
        chrono::Local::now().naive_local()
    }

    fn elapsed(&self) -> Duration {
        self.origin.elapsed()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn system_clock_is_monotonic() {
        let c = SystemClock::new();
        let a = c.elapsed();
        let b = c.elapsed();
        assert!(b >= a);
        assert!(c.now().and_utc().timestamp() > 1_600_000_000);
    }
}
