//! A manually advanced clock.

use chrono::{Duration as ChronoDuration, NaiveDate, NaiveDateTime};
use lb_core::Clock;
use parking_lot::Mutex;
use std::time::Duration;

/// A [`Clock`] that only moves when told to, so coalescing windows and timestamps are exact.
#[derive(Debug)]
pub struct FakeClock {
    state: Mutex<(NaiveDateTime, Duration)>,
}

impl FakeClock {
    /// Starts at 2024-01-01T00:00:00.
    pub fn new() -> Self {
        let start = NaiveDate::from_ymd_opt(2024, 1, 1)
            .and_then(|d| d.and_hms_opt(0, 0, 0))
            .unwrap_or_default();
        Self::starting_at(start)
    }

    /// Starts at `start` with the monotonic timer at zero.
    pub fn starting_at(start: NaiveDateTime) -> Self {
        Self {
            state: Mutex::new((start, Duration::ZERO)),
        }
    }

    /// Moves both the wall clock and the monotonic timer forward.
    pub fn advance(&self, by: Duration) {
        let mut s = self.state.lock();
        s.0 += ChronoDuration::from_std(by).unwrap_or_default();
        s.1 += by;
    }
}

impl Default for FakeClock {
    fn default() -> Self {
        Self::new()
    }
}

impl Clock for FakeClock {
    fn now(&self) -> NaiveDateTime {
        self.state.lock().0
    }

    fn elapsed(&self) -> Duration {
        self.state.lock().1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advance_moves_both_clocks() {
        let c = FakeClock::new();
        let t0 = c.now();
        c.advance(Duration::from_millis(1500));
        assert_eq!(c.elapsed(), Duration::from_millis(1500));
        assert_eq!((c.now() - t0).num_milliseconds(), 1500);
    }
}
