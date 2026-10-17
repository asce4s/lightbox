//! 1024-bin histograms and percentiles for the auto ops (SPEC §6.6.3).

/// Number of histogram bins.
pub const BINS: usize = 1024;

/// A histogram of values in `[0, 1]`.
#[derive(Debug, Clone)]
pub struct Histogram {
    counts: Vec<u32>,
    total: u64,
}

impl Histogram {
    /// An empty histogram.
    pub fn new() -> Self {
        Self {
            counts: vec![0; BINS],
            total: 0,
        }
    }

    /// Adds one value (clamped to `[0, 1]`; NaN counts as 0).
    pub fn add(&mut self, v: f32) {
        let v = if v.is_nan() { 0.0 } else { v.clamp(0.0, 1.0) };
        let bin = ((v * BINS as f32) as usize).min(BINS - 1);
        self.counts[bin] += 1;
        self.total += 1;
    }

    /// Number of values added.
    pub fn len(&self) -> u64 {
        self.total
    }

    /// Whether no value has been added.
    pub fn is_empty(&self) -> bool {
        self.total == 0
    }

    fn bin_reaching(&self, rank: u64) -> usize {
        let mut cum = 0_u64;
        for (i, c) in self.counts.iter().enumerate() {
            cum += u64::from(*c);
            if cum >= rank {
                return i;
            }
        }
        BINS - 1
    }

    /// Lower percentile `p` in `[0, 100]`: the lower edge of the first bin whose cumulative
    /// count reaches `p` percent of the values.
    pub fn low_percentile(&self, p: f64) -> f32 {
        if self.is_empty() {
            return 0.0;
        }
        let rank = ((p / 100.0) * self.total as f64).ceil().max(1.0) as u64;
        self.bin_reaching(rank) as f32 / BINS as f32
    }

    /// Upper percentile `p` in `[0, 100]`: the upper edge of the first bin whose cumulative
    /// count reaches `p` percent of the values.
    pub fn high_percentile(&self, p: f64) -> f32 {
        if self.is_empty() {
            return 1.0;
        }
        let rank = ((p / 100.0) * self.total as f64).ceil().max(1.0) as u64;
        (self.bin_reaching(rank) + 1) as f32 / BINS as f32
    }
}

impl Default for Histogram {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_of_a_uniform_ramp() {
        let mut h = Histogram::new();
        for i in 0..10_000 {
            h.add(i as f32 / 10_000.0);
        }
        assert!((h.low_percentile(0.5) - 0.005).abs() < 0.002);
        assert!((h.high_percentile(99.5) - 0.995).abs() < 0.002);
        assert_eq!(h.len(), 10_000);
    }

    #[test]
    fn constant_values_land_in_one_bin() {
        let mut h = Histogram::new();
        for _ in 0..100 {
            h.add(0.5);
        }
        let lo = h.low_percentile(0.5);
        let hi = h.high_percentile(99.5);
        assert!(lo <= 0.5 && hi >= 0.5 && hi - lo <= 1.5 / BINS as f32);
        let mut top = Histogram::new();
        top.add(1.0);
        top.add(7.0);
        top.add(f32::NAN);
        assert_eq!(
            top.high_percentile(100.0),
            1.0,
            "values clamp into the last bin"
        );
        assert_eq!(top.low_percentile(0.5), 0.0, "NaN counts as zero");
        assert!(Histogram::new().is_empty());
    }
}
