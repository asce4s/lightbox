//! M9.5: slideshow state machine.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use lb_app::vm::{SlideshowOptions, SlideshowState};
use lb_app::{Lightbox, LightboxOptions};
use lb_core::{PhotoId, RootMode};
use lb_jobs::JobQueue;
use lb_testkit::FakeClock;
use std::sync::Arc;
use std::time::Duration;

struct Rig {
    _invariants: lb_testkit::Invariants,
    _home: tempfile::TempDir,
    _dir: tempfile::TempDir,
    lb: Lightbox,
    clock: Arc<FakeClock>,
    ids: Vec<PhotoId>,
}

fn rig(n: usize, corrupt: &[usize]) -> Rig {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    for i in 0..n {
        let path = dir.path().join(format!("p{i:02}.jpg"));
        if corrupt.contains(&i) {
            std::fs::write(&path, b"not a jpeg at all").unwrap();
        } else {
            common::write_jpeg(&path, i as u8 + 1);
        }
    }
    let clock = Arc::new(FakeClock::new());
    let options = LightboxOptions {
        clock: clock.clone(),
        spawn_event_ticker: false,
        ..LightboxOptions::default()
    };
    let lb = Lightbox::open_with(home.path(), JobQueue::inline(), options).unwrap();
    lb.library().add_root(dir.path(), RootMode::Watch).unwrap();
    let mut all = lb.photos().all().unwrap();
    all.sort_by(|a, b| a.path.cmp(&b.path));
    let ids = all.iter().map(|p| p.id).collect();
    Rig {
        _invariants: common::invariants(home.path(), &[dir.path()]),
        _home: home,
        _dir: dir,
        lb,
        clock,
        ids,
    }
}

fn show(r: &Rig, opts: SlideshowOptions) -> SlideshowState {
    SlideshowState::new(&r.lb, &r.ids, opts).unwrap()
}

fn secs(n: f32) -> Duration {
    Duration::from_secs_f32(n)
}

#[test]
fn slideshow_advances_on_tick() {
    let r = rig(3, &[]);
    let mut s = show(&r, SlideshowOptions::default());
    assert_eq!(s.current(), Some(r.ids[0]));
    assert_eq!(s.upcoming(), vec![r.ids[1], r.ids[2]], "preloads two ahead");
    assert!(!s.tick());
    r.clock.advance(secs(2.9));
    assert!(!s.tick(), "interval of 3 s not over");
    r.clock.advance(secs(0.2));
    assert!(s.tick());
    assert_eq!(s.current(), Some(r.ids[1]));
    assert!(!s.tick(), "the timer restarted");
    r.clock.advance(secs(3.0));
    assert!(s.tick());
    assert_eq!(s.current(), Some(r.ids[2]));

    s.set_duration(99);
    assert_eq!(s.options().duration_secs, 30);
    s.set_duration(0);
    assert_eq!(s.options().duration_secs, 1);

    let mut once = show(
        &r,
        SlideshowOptions {
            looping: false,
            ..SlideshowOptions::default()
        },
    );
    assert!(once.go_next() && once.go_next());
    assert!(!once.go_next(), "end of a non-looping show");
    assert!(once.is_finished() && !once.is_playing());
    assert_eq!(once.current(), Some(r.ids[2]));
    assert!(once.go_prev());
    assert!(!once.is_finished());
}

#[test]
fn slideshow_pause_stops_advance() {
    let r = rig(3, &[]);
    let mut s = show(&r, SlideshowOptions::default());
    s.pause();
    r.clock.advance(secs(60.0));
    assert!(!s.tick());
    assert_eq!(s.current(), Some(r.ids[0]));
    s.toggle_play();
    assert!(s.is_playing());
    r.clock.advance(secs(2.0));
    assert!(!s.tick(), "resuming gives the photo a full interval");
    r.clock.advance(secs(1.1));
    assert!(s.tick());

    s.touch();
    assert!(s.controls_visible());
    r.clock.advance(secs(1.9));
    assert!(s.controls_visible());
    r.clock.advance(secs(0.2));
    assert!(!s.controls_visible(), "controls hide after 2 s idle");
    s.touch();
    assert!(s.controls_visible());
}

#[test]
fn slideshow_loop_and_shuffle_deterministic_with_seed() {
    let r = rig(8, &[]);
    let run = |seed: u64| -> Vec<PhotoId> {
        let mut s = show(
            &r,
            SlideshowOptions {
                shuffle: true,
                looping: true,
                seed,
                ..SlideshowOptions::default()
            },
        );
        let mut seen = vec![s.current().unwrap()];
        for _ in 0..19 {
            assert!(s.go_next());
            seen.push(s.current().unwrap());
        }
        seen
    };
    let a = run(42);
    assert_eq!(a, run(42), "same seed, same show");
    assert_ne!(a, run(43));
    let mut first_cycle = a[..8].to_vec();
    first_cycle.sort_by_key(|i| i.get());
    let mut expected = r.ids.clone();
    expected.sort_by_key(|i| i.get());
    assert_eq!(first_cycle, expected, "every photo once per cycle");
    assert_ne!(a[..8], r.ids[..], "not in grid order");

    let mut plain = show(&r, SlideshowOptions::default());
    for _ in 0..8 {
        plain.go_next();
    }
    assert_eq!(plain.current(), Some(r.ids[0]), "loop wraps around");

    let mut toggled = show(&r, SlideshowOptions::default());
    toggled.go_next();
    toggled.go_next();
    let here = toggled.current();
    toggled.set_shuffle(true);
    assert_eq!(
        toggled.current(),
        here,
        "turning shuffle on keeps the photo"
    );
    toggled.set_shuffle(false);
    assert_eq!(toggled.current(), here);
    assert_eq!(toggled.index(), 2);
}

#[test]
fn slideshow_skips_unreadable() {
    let r = rig(5, &[1, 3]);
    let mut s = show(&r, SlideshowOptions::default());
    assert_eq!(s.len(), 3, "unreadable files are left out");
    let mut seen = vec![s.current().unwrap()];
    while s.go_next() && seen.len() < 3 {
        seen.push(s.current().unwrap());
    }
    assert_eq!(seen, vec![r.ids[0], r.ids[2], r.ids[4]]);

    let mut s = show(&r, SlideshowOptions::default());
    s.go_next();
    s.skip_unreadable();
    assert_eq!(
        s.current(),
        Some(r.ids[4]),
        "a photo that fails to show is dropped"
    );
    s.skip_unreadable();
    s.skip_unreadable();
    assert!(s.is_finished() && s.current().is_none());
}
