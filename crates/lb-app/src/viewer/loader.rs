//! Progressive load queue (SPEC-VIEWER §V7).
//!
//! Opening never reads a file. The caller polls when it is ready to decode, and each poll runs
//! only the jobs whose time has come. L0 is an in-memory thumbnail flag. L2 waits 120 ms while
//! navigation is rapid. Neighbours are preloaded after the current L2, direction of travel first.

use std::collections::HashMap;
use std::time::Duration;

use crate::error::Result;

/// Decoded-image budget for L2/L3 (SPEC-VIEWER §V7.4).
pub const MEMORY_BUDGET: u64 = 512 * 1024 * 1024;
const RAPID: Duration = Duration::from_millis(150);
const SETTLE: Duration = Duration::from_millis(120);

/// How detailed a shown or requested image is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum LoadLevel {
    /// Cached thumbnail already in memory.
    L0,
    /// Embedded EXIF thumbnail.
    L1,
    /// Screen proxy.
    L2,
    /// Full resolution, only when zoom needs it.
    L3,
}

/// Reads one level. `open` does not call this.
pub trait MediaBytes {
    /// Byte size `width * height * 4` of the decoded level, or why it failed.
    fn read(&mut self, index: usize, level: LoadLevel) -> Result<u64>;
}

#[derive(Debug, Clone, Copy)]
struct Pending {
    index: usize,
    level: LoadLevel,
    not_before: Duration,
}

#[derive(Debug, Clone, Copy)]
struct Resident {
    level: LoadLevel,
    bytes: u64,
}

/// Schedules L0–L3 for a fixed list of entries.
pub struct ViewerLoader {
    count: usize,
    current: usize,
    travel: i32,
    budget: u64,
    used: u64,
    thumbs: Vec<bool>,
    exif: Vec<bool>,
    shown: HashMap<usize, LoadLevel>,
    resident: HashMap<usize, Resident>,
    errors: HashMap<usize, String>,
    queue: Vec<Pending>,
    last_nav: Option<Duration>,
    opened_at: Duration,
}

impl ViewerLoader {
    /// Loader for `count` entries. `thumbs[i]` / `exif[i]` are memory-only facts.
    ///
    /// Does not call [`MediaBytes`]. The current entry is shown at L0 when `thumbs` says so.
    #[must_use]
    pub fn open(count: usize, start: usize, thumbs: &[bool], exif: &[bool]) -> Self {
        Self::with_budget(count, start, thumbs, exif, MEMORY_BUDGET)
    }

    /// [`Self::open`] with an explicit decoded-byte budget. Production uses [`MEMORY_BUDGET`].
    #[must_use]
    pub fn with_budget(
        count: usize,
        start: usize,
        thumbs: &[bool],
        exif: &[bool],
        budget: u64,
    ) -> Self {
        let count = count.max(1);
        let current = start.min(count - 1);
        let mut loader = Self {
            count,
            current,
            travel: 1,
            budget,
            used: 0,
            thumbs: pad(thumbs, count),
            exif: pad(exif, count),
            shown: HashMap::new(),
            resident: HashMap::new(),
            errors: HashMap::new(),
            queue: Vec::new(),
            last_nav: None,
            opened_at: Duration::ZERO,
        };
        loader.note_cached(current);
        loader.enqueue_initial(current, Duration::ZERO, false);
        loader
    }

    /// Level on screen for `index`, if any.
    #[must_use]
    pub fn shown(&self, index: usize) -> Option<LoadLevel> {
        self.shown.get(&index).copied()
    }

    /// Decode or open error for `index`.
    #[must_use]
    pub fn error(&self, index: usize) -> Option<&str> {
        self.errors.get(&index).map(String::as_str)
    }

    /// Indexes that still hold decoded bytes, nearest first.
    #[must_use]
    pub fn resident_indexes(&self) -> Vec<usize> {
        let mut ids: Vec<_> = self.resident.keys().copied().collect();
        ids.sort_by_key(|i| i.abs_diff(self.current));
        ids
    }

    /// True when nothing is on screen yet and L2 has had 120 ms.
    #[must_use]
    pub fn spinner(&self, now: Duration) -> bool {
        self.shown(self.current).is_none()
            && now.saturating_sub(self.opened_at) >= SETTLE
            && self.error(self.current).is_none()
    }

    /// Moves to `index`. Inputs closer than 150 ms skip intermediate L2 decodes.
    pub fn navigate(&mut self, index: usize, now: Duration) {
        if index >= self.count || index == self.current {
            return;
        }
        let rapid = self.last_nav.is_some_and(|t| now.saturating_sub(t) < RAPID);
        self.travel = if index > self.current { 1 } else { -1 };
        self.current = index;
        self.last_nav = Some(now);
        self.note_cached(index);
        if rapid {
            self.queue
                .retain(|job| job.level < LoadLevel::L2 || job.index == index);
        } else {
            self.queue
                .retain(|job| job.index.abs_diff(self.current) <= 2);
        }
        self.enqueue_initial(index, now, rapid);
    }

    /// Asks for L3 of the current entry (zoom past the proxy).
    pub fn request_full(&mut self, now: Duration) {
        if self
            .shown(self.current)
            .is_some_and(|level| level >= LoadLevel::L3)
        {
            return;
        }
        self.enqueue(self.current, LoadLevel::L3, now);
    }

    /// Runs every job that is due at `now`.
    pub fn poll(&mut self, now: Duration, media: &mut dyn MediaBytes) {
        let ready: Vec<Pending> = self
            .queue
            .iter()
            .copied()
            .filter(|job| job.not_before <= now)
            .collect();
        self.queue.retain(|job| job.not_before > now);
        for job in ready {
            if self.satisfied(job.index, job.level) {
                continue;
            }
            match media.read(job.index, job.level) {
                Ok(bytes) => self.admit(job.index, job.level, bytes, now),
                Err(error) => {
                    self.errors.insert(job.index, error.to_string());
                }
            }
        }
    }

    fn note_cached(&mut self, index: usize) {
        if self.thumbs.get(index).copied().unwrap_or(false) {
            self.shown.entry(index).or_insert(LoadLevel::L0);
        }
    }

    fn enqueue_initial(&mut self, index: usize, now: Duration, rapid: bool) {
        let have_thumb = self.thumbs.get(index).copied().unwrap_or(false);
        let have_exif = self.exif.get(index).copied().unwrap_or(false);
        if !have_thumb && have_exif {
            self.enqueue(index, LoadLevel::L1, now);
        }
        let l2_at = if rapid {
            now.saturating_add(SETTLE)
        } else {
            now
        };
        self.enqueue(index, LoadLevel::L2, l2_at);
    }

    fn enqueue(&mut self, index: usize, level: LoadLevel, not_before: Duration) {
        if self.satisfied(index, level) {
            return;
        }
        if self
            .queue
            .iter()
            .any(|job| job.index == index && job.level == level)
        {
            return;
        }
        self.queue.push(Pending {
            index,
            level,
            not_before,
        });
    }

    fn satisfied(&self, index: usize, level: LoadLevel) -> bool {
        if level <= LoadLevel::L1 {
            return self.shown(index).is_some_and(|shown| shown >= level);
        }
        self.resident
            .get(&index)
            .is_some_and(|resident| resident.level >= level)
    }

    fn admit(&mut self, index: usize, level: LoadLevel, bytes: u64, now: Duration) {
        self.shown
            .entry(index)
            .and_modify(|shown| {
                if level > *shown {
                    *shown = level;
                }
            })
            .or_insert(level);
        if level < LoadLevel::L2 {
            return;
        }
        if let Some(prev) = self.resident.remove(&index) {
            self.used = self.used.saturating_sub(prev.bytes);
        }
        self.resident.insert(index, Resident { level, bytes });
        self.used = self.used.saturating_add(bytes);
        self.evict_farthest();
        if index == self.current && level == LoadLevel::L2 && self.resident.contains_key(&index) {
            self.enqueue_preloads(now);
        }
    }

    fn evict_farthest(&mut self) {
        while self.used > self.budget {
            let Some(victim) = self
                .resident
                .keys()
                .copied()
                .filter(|index| *index != self.current)
                .max_by_key(|index| index.abs_diff(self.current))
            else {
                break;
            };
            if let Some(prev) = self.resident.remove(&victim) {
                self.used = self.used.saturating_sub(prev.bytes);
            }
            if self
                .shown
                .get(&victim)
                .is_some_and(|level| *level >= LoadLevel::L2)
            {
                self.shown.remove(&victim);
            }
        }
    }

    fn enqueue_preloads(&mut self, now: Duration) {
        let dir = self.travel;
        for step in [dir, -dir, dir.saturating_mul(2), -dir.saturating_mul(2)] {
            let next = self.current as i32 + step;
            if next < 0 || next >= self.count as i32 {
                continue;
            }
            self.enqueue(next as usize, LoadLevel::L2, now);
        }
    }
}

fn pad(flags: &[bool], count: usize) -> Vec<bool> {
    let mut out = flags.iter().copied().take(count).collect::<Vec<_>>();
    out.resize(count, false);
    out
}
