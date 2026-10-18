//! M13.5: progressive loading and error entries.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::write_jpeg;
use lb_app::Lightbox;
use lb_app::viewer::{LoadLevel, MEMORY_BUDGET, MediaBytes, ViewerLoader};
use lb_app::vm::ViewerVm;
use lb_jobs::JobQueue;
use std::fs;
use std::time::Duration;

struct Fake {
    reads: Vec<(usize, LoadLevel)>,
    bytes: u64,
}

impl Fake {
    fn new(bytes: u64) -> Self {
        Self {
            reads: Vec::new(),
            bytes,
        }
    }
}

impl MediaBytes for Fake {
    fn read(&mut self, index: usize, level: LoadLevel) -> lb_app::Result<u64> {
        self.reads.push((index, level));
        Ok(self.bytes)
    }
}

fn ms(ms: u64) -> Duration {
    Duration::from_millis(ms)
}

#[test]
fn viewer_progressive_load_order() {
    let mut media = Fake::new(100);
    let mut with_thumb = ViewerLoader::open(1, 0, &[true], &[true]);
    assert_eq!(with_thumb.shown(0), Some(LoadLevel::L0));
    assert!(media.reads.is_empty(), "open does not decode");
    with_thumb.poll(ms(0), &mut media);
    assert_eq!(media.reads, [(0, LoadLevel::L2)]);
    assert_eq!(with_thumb.shown(0), Some(LoadLevel::L2));
    with_thumb.request_full(ms(1));
    with_thumb.poll(ms(1), &mut media);
    assert_eq!(media.reads.last().copied(), Some((0, LoadLevel::L3)));
    assert_eq!(with_thumb.shown(0), Some(LoadLevel::L3));

    let mut media = Fake::new(100);
    let mut standalone = ViewerLoader::open(1, 0, &[false], &[true]);
    assert!(standalone.shown(0).is_none());
    assert!(!standalone.spinner(ms(119)));
    assert!(standalone.spinner(ms(120)));
    standalone.poll(ms(0), &mut media);
    assert_eq!(
        media.reads,
        [(0, LoadLevel::L1), (0, LoadLevel::L2)],
        "embedded thumb is decoded before the screen proxy"
    );
    assert_eq!(standalone.shown(0), Some(LoadLevel::L2));
    assert!(!standalone.spinner(ms(120)));
}

#[test]
fn viewer_open_has_no_blocking_io() {
    let mut media = Fake::new(100);
    let loader = ViewerLoader::open(3, 1, &[false, true, false], &[true, true, true]);
    assert_eq!(media.reads, []);
    assert_eq!(loader.shown(1), Some(LoadLevel::L0));
    let _ = &mut media;
}

#[test]
fn viewer_preloads_neighbors_then_instant_navigation() {
    let mut media = Fake::new(100);
    let mut loader = ViewerLoader::open(5, 2, &[true; 5], &[false; 5]);
    loader.poll(ms(0), &mut media);
    assert_eq!(media.reads, [(2, LoadLevel::L2)]);
    loader.poll(ms(0), &mut media);
    assert_eq!(
        media.reads,
        [
            (2, LoadLevel::L2),
            (3, LoadLevel::L2),
            (1, LoadLevel::L2),
            (4, LoadLevel::L2),
            (0, LoadLevel::L2),
        ],
        "direction of travel first, then the other side, then ±2"
    );
    let before = media.reads.len();
    loader.navigate(3, ms(1_000));
    assert_eq!(loader.shown(3), Some(LoadLevel::L2));
    loader.poll(ms(1_000), &mut media);
    assert!(
        !media.reads[before..].contains(&(3, LoadLevel::L2)),
        "a preloaded neighbour is not decoded again"
    );
}

#[test]
fn viewer_rapid_navigation_skips_intermediate_decodes() {
    let mut media = Fake::new(100);
    let mut loader = ViewerLoader::open(6, 0, &[true; 6], &[true; 6]);
    loader.poll(ms(0), &mut media);
    loader.navigate(1, ms(10));
    loader.navigate(2, ms(80));
    loader.navigate(3, ms(140));
    loader.poll(ms(140), &mut media);
    let passed: Vec<_> = media
        .reads
        .iter()
        .filter(|(index, _level)| *index == 1 || *index == 2)
        .map(|(_, level)| *level)
        .collect();
    assert!(
        passed.iter().all(|level| *level < LoadLevel::L2),
        "photos passed through in under 150 ms stay at L0/L1, got {passed:?}"
    );
    assert!(!media.reads.contains(&(3, LoadLevel::L2)));
    loader.poll(ms(260), &mut media);
    assert!(
        media.reads.contains(&(3, LoadLevel::L2)),
        "L2 starts once the photo has stayed current for 120 ms"
    );
}

#[test]
fn viewer_memory_budget_evicts_farthest() {
    assert_eq!(MEMORY_BUDGET, 512 * 1024 * 1024);
    let mut media = Fake::new(1_000);
    let mut loader = ViewerLoader::with_budget(4, 0, &[true; 4], &[false; 4], 2_500);
    loader.poll(ms(0), &mut media);
    loader.poll(ms(0), &mut media);
    assert_eq!(
        loader.resident_indexes(),
        vec![0, 1],
        "the current frame stays; the farthest decoded frames are dropped"
    );
    assert!(loader.shown(0).is_some());
}

#[test]
fn viewer_missing_file_shows_error_and_navigation_continues() {
    let home = tempfile::tempdir().unwrap();
    let lb = Lightbox::open(home.path(), JobQueue::inline()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let missing = dir.path().join("gone.jpg");
    let ok = dir.path().join("ok.jpg");
    write_jpeg(&ok, 1);
    let mut vm = ViewerVm::open_paths(&lb, &[missing, ok.clone()]).unwrap();
    assert_eq!(vm.current_error(), Some("File not found"));
    assert!(vm.go_next());
    assert_eq!(vm.current_entry().path, ok);
    assert!(vm.current_error().is_none());
}

#[test]
fn viewer_non_image_argument_shows_error() {
    let home = tempfile::tempdir().unwrap();
    let lb = Lightbox::open(home.path(), JobQueue::inline()).unwrap();
    let dir = tempfile::tempdir().unwrap();
    let notes = dir.path().join("notes.txt");
    fs::write(&notes, b"hello").unwrap();
    let vm = ViewerVm::open_paths(&lb, std::slice::from_ref(&notes)).unwrap();
    assert_eq!(
        vm.current_error(),
        Some("Lightbox can't open this kind of file")
    );
}
