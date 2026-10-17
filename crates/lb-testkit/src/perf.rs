//! Performance budget checks (SPEC §11).
//!
//! Budget tests always run, at a reduced scale, so they stay correct under `cargo test`.
//! Timing is only asserted when [`PERF_ENV`] is set, which `cargo xtask perf` does in release.

use std::time::{Duration, Instant};

/// Environment variable that turns budget enforcement and full-size workloads on.
pub const PERF_ENV: &str = "LIGHTBOX_PERF";

/// A budget may be exceeded by this factor before a check fails, to absorb runner noise.
pub const TOLERANCE: f64 = 1.5;

/// Whether timing budgets are enforced.
pub fn enforced() -> bool {
    std::env::var_os(PERF_ENV).is_some_and(|v| v != "0")
}

/// `full` when budgets are enforced, otherwise the smoke-test size `small`.
pub fn scale(full: usize, small: usize) -> usize {
    if enforced() { full } else { small }
}

/// Runs `f` and returns its result with the time it took.
pub fn time<T>(f: impl FnOnce() -> T) -> (T, Duration) {
    let start = Instant::now();
    let value = f();
    (value, start.elapsed())
}

/// The median duration of `runs` calls to `f`.
pub fn median_of(runs: usize, mut f: impl FnMut()) -> Duration {
    let mut times: Vec<Duration> = (0..runs.max(1)).map(|_| time(&mut f).1).collect();
    times.sort();
    times[times.len() / 2]
}

/// Whether `measured` is within [`TOLERANCE`] times `budget`.
pub fn within_budget(measured: Duration, budget: Duration) -> bool {
    measured.as_secs_f64() <= budget.as_secs_f64() * TOLERANCE
}

/// Reports `measured` against `budget` and, when enforced, panics if it is over the limit.
pub fn check(name: &str, measured: Duration, budget: Duration) {
    let status = if within_budget(measured, budget) {
        "ok"
    } else {
        "OVER"
    };
    println!(
        "perf {name}: {:.1} ms (budget {:.1} ms) {status}",
        measured.as_secs_f64() * 1000.0,
        budget.as_secs_f64() * 1000.0
    );
    assert!(
        !enforced() || within_budget(measured, budget),
        "perf budget exceeded for {name}: {measured:?} > {budget:?} x {TOLERANCE}"
    );
}

/// Resident set size of this process in MiB, where the platform exposes it.
pub fn resident_mib() -> Option<f64> {
    let status = std::fs::read_to_string("/proc/self/status").ok()?;
    let kb: f64 = status
        .lines()
        .find_map(|l| l.strip_prefix("VmRSS:"))?
        .split_whitespace()
        .next()?
        .parse()
        .ok()?;
    Some(kb / 1024.0)
}

/// Reports resident memory against `budget_mib` and, when enforced, panics if it is over.
pub fn check_memory(name: &str, budget_mib: f64) {
    let Some(used) = resident_mib() else {
        println!("perf {name}: resident memory unavailable on this platform");
        return;
    };
    println!("perf {name}: {used:.0} MiB (budget {budget_mib:.0} MiB)");
    assert!(
        !enforced() || used <= budget_mib * TOLERANCE,
        "memory budget exceeded for {name}: {used:.0} MiB > {budget_mib:.0} MiB x {TOLERANCE}"
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_tolerance_is_fifty_percent() {
        let budget = Duration::from_millis(100);
        assert!(within_budget(Duration::from_millis(150), budget));
        assert!(!within_budget(Duration::from_millis(151), budget));
    }

    #[test]
    fn median_picks_the_middle_run() {
        let mut n = 0;
        let d = median_of(3, || {
            n += 1;
        });
        assert_eq!(n, 3);
        assert!(d < Duration::from_secs(1));
    }
}
