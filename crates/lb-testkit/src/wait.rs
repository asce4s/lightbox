//! Bounded polling helper for asynchronous conditions.

use std::time::{Duration, Instant};

/// Polls `cond` every few milliseconds until it returns `true` or `timeout` elapses.
///
/// Returns whether the condition became true. Never sleeps longer than 5 ms at a time,
/// so tests stay deterministic and fast (AGENTS.md §3.7).
#[must_use = "assert on the result"]
pub fn wait_until(timeout: Duration, mut cond: impl FnMut() -> bool) -> bool {
    let deadline = Instant::now() + timeout;
    loop {
        if cond() {
            return true;
        }
        if Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(
            Duration::from_millis(5).min(deadline.saturating_duration_since(Instant::now())),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wait_until_times_out() {
        let start = Instant::now();
        assert!(!wait_until(Duration::from_millis(50), || false));
        let elapsed = start.elapsed();
        assert!(elapsed >= Duration::from_millis(50), "{elapsed:?}");
        assert!(elapsed < Duration::from_secs(2), "{elapsed:?}");
    }

    #[test]
    fn wait_until_returns_as_soon_as_condition_holds() {
        let mut calls = 0;
        let ok = wait_until(Duration::from_secs(5), || {
            calls += 1;
            calls >= 3
        });
        assert!(ok);
        assert_eq!(calls, 3);
    }

    #[test]
    fn wait_until_checks_condition_once_even_with_zero_timeout() {
        assert!(wait_until(Duration::ZERO, || true));
    }
}
