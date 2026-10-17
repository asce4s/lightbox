//! The slideshow state machine (SPEC §6.12), driven by a clock so tests control time.

use crate::error::Result;
use crate::lightbox::Lightbox;
use crate::settings::{SlideshowSettings, Transition};
use lb_core::{Clock, PhotoId, PhotoStatus};
use std::sync::Arc;
use std::time::Duration;

/// Idle time after which the controls hide.
pub const CONTROLS_IDLE: Duration = Duration::from_secs(2);
/// Allowed seconds per photo.
pub const DURATION_RANGE: (u32, u32) = (1, 30);
/// How many upcoming photos the view preloads.
pub const PRELOAD_AHEAD: usize = 2;

/// Slideshow settings of one run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SlideshowOptions {
    /// Seconds per photo, 1 to 30.
    pub duration_secs: u32,
    /// Transition between photos.
    pub transition: Transition,
    /// Show captions.
    pub show_captions: bool,
    /// Random order.
    pub shuffle: bool,
    /// Start over after the last photo.
    pub looping: bool,
    /// Seed of the random order.
    pub seed: u64,
}

impl SlideshowOptions {
    /// The stored defaults with `seed` for the random order.
    pub fn from_settings(s: &SlideshowSettings, seed: u64) -> Self {
        Self {
            duration_secs: s.duration_secs.clamp(DURATION_RANGE.0, DURATION_RANGE.1),
            transition: s.transition,
            show_captions: s.show_captions,
            shuffle: s.shuffle,
            looping: s.loop_forever,
            seed,
        }
    }
}

impl Default for SlideshowOptions {
    fn default() -> Self {
        Self::from_settings(&SlideshowSettings::default(), 0)
    }
}

/// Deterministic generator for the shuffle (SplitMix64).
struct SplitMix(u64);

impl SplitMix {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

fn shuffled(ids: &[PhotoId], seed: u64) -> Vec<PhotoId> {
    let mut out = ids.to_vec();
    let mut rng = SplitMix(seed);
    for i in (1..out.len()).rev() {
        let j = (rng.next() % (i as u64 + 1)) as usize;
        out.swap(i, j);
    }
    out
}

/// What is showing, whether it advances and when the controls hide.
pub struct SlideshowState {
    lb: Lightbox,
    clock: Arc<dyn Clock>,
    base: Vec<PhotoId>,
    order: Vec<PhotoId>,
    index: usize,
    cycle: u64,
    opts: SlideshowOptions,
    playing: bool,
    finished: bool,
    changed_at: Duration,
    touched_at: Duration,
}

impl SlideshowState {
    /// Starts at the first photo of `ids` (grid order), playing. Photos that are unreadable or
    /// missing in the catalog are left out. The lightbox's clock drives the timer.
    pub fn new(lb: &Lightbox, ids: &[PhotoId], opts: SlideshowOptions) -> Result<Self> {
        Self::with_clock(lb, ids, opts, lb.clock().clone())
    }

    /// As [`Self::new`] with an explicit clock.
    pub fn with_clock(
        lb: &Lightbox,
        ids: &[PhotoId],
        mut opts: SlideshowOptions,
        clock: Arc<dyn Clock>,
    ) -> Result<Self> {
        opts.duration_secs = opts.duration_secs.clamp(DURATION_RANGE.0, DURATION_RANGE.1);
        let base: Vec<PhotoId> = lb
            .photos()
            .get_many(ids)?
            .into_iter()
            .filter(|p| p.status == PhotoStatus::Ok)
            .map(|p| p.id)
            .collect();
        let order = if opts.shuffle {
            shuffled(&base, opts.seed)
        } else {
            base.clone()
        };
        let now = clock.elapsed();
        Ok(Self {
            lb: lb.clone(),
            clock,
            finished: order.is_empty(),
            base,
            order,
            index: 0,
            cycle: 0,
            opts,
            playing: true,
            changed_at: now,
            touched_at: now,
        })
    }

    /// Jumps to `id` when it is part of the show.
    pub fn start_at(&mut self, id: PhotoId) -> bool {
        match self.order.iter().position(|i| *i == id) {
            Some(i) => {
                self.index = i;
                self.changed_at = self.clock.elapsed();
                true
            }
            None => false,
        }
    }

    /// The photo being shown.
    pub fn current(&self) -> Option<PhotoId> {
        self.order.get(self.index).copied()
    }

    /// The photos that follow, for preloading.
    pub fn upcoming(&self) -> Vec<PhotoId> {
        let rest = self.order.iter().skip(self.index + 1).take(PRELOAD_AHEAD);
        let mut out: Vec<PhotoId> = rest.copied().collect();
        if out.len() < PRELOAD_AHEAD && self.opts.looping && !self.opts.shuffle {
            out.extend(
                self.order
                    .iter()
                    .take(PRELOAD_AHEAD - out.len())
                    .filter(|id| Some(**id) != self.current()),
            );
        }
        out
    }

    /// Position in the play order, from 0.
    pub fn index(&self) -> usize {
        self.index
    }

    /// Photos in the play order.
    pub fn len(&self) -> usize {
        self.order.len()
    }

    /// Whether there is nothing to show.
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }

    /// Whether the show advances by itself.
    pub fn is_playing(&self) -> bool {
        self.playing
    }

    /// The last photo was shown and the show does not loop.
    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// The settings of this run.
    pub fn options(&self) -> &SlideshowOptions {
        &self.opts
    }

    /// Caption of the current photo when captions are on and it has one.
    pub fn caption(&self) -> Option<String> {
        if !self.opts.show_captions {
            return None;
        }
        let photo = self.lb.photos().get(self.current()?).ok()??;
        photo.caption.filter(|c| !c.trim().is_empty())
    }

    /// Resumes advancing; the current photo gets a full interval.
    pub fn play(&mut self) {
        if !self.finished {
            self.playing = true;
            self.changed_at = self.clock.elapsed();
        }
    }

    /// Stops advancing.
    pub fn pause(&mut self) {
        self.playing = false;
    }

    /// Space.
    pub fn toggle_play(&mut self) {
        if self.playing {
            self.pause();
        } else {
            self.play();
        }
    }

    /// Advances when the interval has passed; returns whether the photo changed.
    pub fn tick(&mut self) -> bool {
        if !self.playing || self.finished {
            return false;
        }
        let interval = Duration::from_secs(u64::from(self.opts.duration_secs));
        if self.clock.elapsed().saturating_sub(self.changed_at) < interval {
            return false;
        }
        self.go_next()
    }

    /// Right arrow: the next photo. At the end it loops or, without looping, finishes the show
    /// and returns `false`.
    pub fn go_next(&mut self) -> bool {
        if self.order.is_empty() {
            return false;
        }
        self.changed_at = self.clock.elapsed();
        if self.index + 1 < self.order.len() {
            self.index += 1;
            return true;
        }
        if self.opts.looping && self.order.len() > 1 {
            self.cycle += 1;
            self.order = if self.opts.shuffle {
                shuffled(&self.base, self.opts.seed.wrapping_add(self.cycle))
            } else {
                self.base.clone()
            };
            self.index = 0;
            return true;
        }
        if self.opts.looping {
            return false;
        }
        self.playing = false;
        self.finished = true;
        false
    }

    /// Left arrow: the previous photo, if any.
    pub fn go_prev(&mut self) -> bool {
        self.changed_at = self.clock.elapsed();
        if self.index == 0 {
            return false;
        }
        self.index -= 1;
        self.finished = false;
        true
    }

    /// The view could not show the current photo: drop it from the show and move on.
    pub fn skip_unreadable(&mut self) {
        let Some(bad) = self.current() else { return };
        self.base.retain(|id| *id != bad);
        self.order.remove(self.index);
        self.changed_at = self.clock.elapsed();
        if self.order.is_empty() {
            self.playing = false;
            self.finished = true;
        } else if self.index >= self.order.len() {
            if self.opts.looping {
                self.index = 0;
            } else {
                self.index = self.order.len() - 1;
                self.playing = false;
                self.finished = true;
            }
        }
    }

    /// Sets seconds per photo (clamped to 1 to 30).
    pub fn set_duration(&mut self, secs: u32) {
        self.opts.duration_secs = secs.clamp(DURATION_RANGE.0, DURATION_RANGE.1);
    }

    /// Turns the random order on or off, keeping the current photo.
    pub fn set_shuffle(&mut self, on: bool) {
        if self.opts.shuffle == on {
            return;
        }
        let current = self.current();
        self.opts.shuffle = on;
        self.order = if on {
            shuffled(&self.base, self.opts.seed.wrapping_add(self.cycle))
        } else {
            self.base.clone()
        };
        let at = current.and_then(|c| self.order.iter().position(|id| *id == c));
        match at {
            Some(i) if on => {
                self.order.swap(0, i);
                self.index = 0;
            }
            Some(i) => self.index = i,
            None => self.index = 0,
        }
    }

    /// Turns looping on or off.
    pub fn set_loop(&mut self, on: bool) {
        self.opts.looping = on;
    }

    /// Turns captions on or off.
    pub fn set_show_captions(&mut self, on: bool) {
        self.opts.show_captions = on;
    }

    /// Sets the transition.
    pub fn set_transition(&mut self, t: Transition) {
        self.opts.transition = t;
    }

    /// Pointer or key activity: shows the controls again.
    pub fn touch(&mut self) {
        self.touched_at = self.clock.elapsed();
    }

    /// Controls show until [`CONTROLS_IDLE`] after the last activity.
    pub fn controls_visible(&self) -> bool {
        self.clock.elapsed().saturating_sub(self.touched_at) < CONTROLS_IDLE
    }
}
