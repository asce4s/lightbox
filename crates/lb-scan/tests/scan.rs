//! Scanner acceptance tests (SPEC §6.2.1, §6.2.4).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use lb_core::{PhotoStatus, RootMode, TakenAtSource};
use lb_scan::{ScanEvent, ScanOptions};
use lb_testkit::fixtures::{
    self,
    manifest::{Manifest, Status},
};
use std::sync::atomic::AtomicBool;
use std::time::Duration;

#[test]
fn scan_indexes_basic_fixture_set() {
    let env = env();
    let basic = generated_basic();
    let _invariants = lb_testkit::Invariants::new().originals_exact(&basic);
    let root = env.add_root(&basic.canonicalize().unwrap(), RootMode::Once);
    let (summary, _) = env.scan(&root);
    let manifest = Manifest::load_default().unwrap();
    let expect_ok = manifest
        .files
        .iter()
        .filter(|f| f.expect.status == Status::Ok)
        .count();
    let expect_bad = manifest
        .files
        .iter()
        .filter(|f| f.expect.status == Status::Unreadable)
        .count();
    assert_eq!(summary.added, expect_ok + expect_bad);
    assert_eq!(
        (summary.updated, summary.unchanged, summary.missing),
        (0, 0, 0)
    );
    assert_eq!(summary.errors, expect_bad);

    let photos = env.photos_under(&root.path);
    assert_eq!(photos.len(), expect_ok + expect_bad);
    for spec in manifest
        .files
        .iter()
        .filter(|f| f.expect.status != Status::Ignored)
    {
        let abs = root.path.join(spec.path.strip_prefix("basic/").unwrap());
        let photo = photos
            .iter()
            .find(|p| p.path == abs)
            .unwrap_or_else(|| panic!("{} not indexed", spec.path));
        match spec.expect.status {
            Status::Ok => {
                assert_eq!(photo.status, PhotoStatus::Ok, "{}", spec.path);
                assert_eq!(
                    (photo.width, photo.height),
                    (spec.expect.width, spec.expect.height),
                    "{}",
                    spec.path
                );
                assert_eq!(
                    photo.taken_at_src.map(TakenAtSource::as_str),
                    spec.expect.taken_at_src.as_deref(),
                    "{}",
                    spec.path
                );
                if let Some(text) = &spec.expect.taken_at {
                    assert_eq!(
                        photo
                            .taken_at
                            .unwrap()
                            .format("%Y-%m-%dT%H:%M:%S")
                            .to_string(),
                        *text,
                        "{}",
                        spec.path
                    );
                }
                if let Some([lat, lon]) = spec.expect.gps {
                    let g = photo.gps.unwrap();
                    assert!(
                        (g.lat - lat).abs() < 1e-6 && (g.lon - lon).abs() < 1e-6,
                        "{}",
                        spec.path
                    );
                }
                assert_eq!(photo.quick_key.len(), 64, "{}", spec.path);
            }
            Status::Unreadable => {
                assert_eq!(photo.status, PhotoStatus::Unreadable, "{}", spec.path);
                assert!(photo.error.is_some(), "{}", spec.path);
                assert!(
                    photo.taken_at.is_some(),
                    "unreadable files still get a date: {}",
                    spec.path
                );
            }
            Status::Ignored => unreachable!(),
        }
    }
    for hidden in [".hidden", "@eaDir", ".picasa.ini"] {
        assert!(
            photos
                .iter()
                .all(|p| !p.path.to_string_lossy().contains(hidden)),
            "{hidden}"
        );
    }
    let camera = photos
        .iter()
        .find(|p| p.path.ends_with("beach_01.jpg"))
        .unwrap();
    assert_eq!(camera.camera.as_deref(), Some("Canon EOS R6"));
    let folders = env.catalog.folders_in_root(root.id).unwrap();
    assert!(
        folders
            .iter()
            .any(|f| f.path == root.path.join("2021/beach"))
    );
    let beach = folders
        .iter()
        .find(|f| f.path == root.path.join("2021/beach"))
        .unwrap();
    assert_eq!(
        beach.parent_id,
        folders
            .iter()
            .find(|f| f.path == root.path.join("2021"))
            .map(|f| f.id)
    );
    assert_eq!(
        beach.sort_date.unwrap().format("%Y-%m-%d").to_string(),
        "2021-07-01",
        "sort_date is the earliest taken_at"
    );
}

#[test]
fn scan_is_idempotent() {
    let env = env();
    let copy = env.subdir("lib");
    fixtures::copy_tree(&generated_basic(), &copy).unwrap();
    let root = env.add_root(&copy, RootMode::Once);
    let (first, _) = env.scan(&root);
    let before = env.photos_under(&copy);
    let (second, events) = env.scan(&root);
    let after = env.photos_under(&copy);
    assert_eq!(
        second.added + second.updated + second.missing,
        0,
        "{second:?}"
    );
    assert_eq!(second.unchanged, first.added);
    assert_eq!(
        second.errors, 0,
        "unchanged unreadable files are skipped, not re-reported"
    );
    assert_eq!(before, after);
    assert!(matches!(events.last(), Some(ScanEvent::Finished { .. })));
}

#[test]
fn scanner_skips_hidden_and_system_dirs() {
    let basic = generated_basic();
    let jpg = basic.join("2021/beach/beach_01.jpg");
    let own = tempfile::tempdir().unwrap();
    let env = env_with(ScanOptions {
        progress_interval: Duration::ZERO,
        ignored_dirs: vec![own.path().canonicalize().unwrap().join("data")],
        ..ScanOptions::default()
    });
    let root_dir = env.subdir("lib");
    let put = |rel: &str| {
        let p = root_dir.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::copy(&jpg, p).unwrap();
    };
    for rel in [
        "a.jpg",
        "nested/h.JPG",
        ".hidden/b.jpg",
        "@eaDir/c.jpg",
        "$RECYCLE.BIN/d.jpg",
        "System Volume Information/e.jpg",
        ".Trash-1000/f.jpg",
        ".hiddenfile.jpg",
        "nested/.picasaoriginals/orig.jpg",
        "notes.txt",
        "nested/readme",
    ] {
        put(rel);
    }
    let root = env.add_root(&root_dir, RootMode::Watch);
    let (summary, _) = env.scan(&root);
    let mut names: Vec<_> = env
        .photos_under(&root_dir)
        .into_iter()
        .map(|p| {
            p.path
                .strip_prefix(&root_dir)
                .unwrap()
                .display()
                .to_string()
        })
        .collect();
    names.sort();
    assert_eq!(names, ["a.jpg", "nested/h.JPG"]);
    assert_eq!(summary.added, 2);

    let data_root = own.path().canonicalize().unwrap();
    std::fs::create_dir_all(data_root.join("data/cache")).unwrap();
    std::fs::copy(&jpg, data_root.join("data/cache/t.jpg")).unwrap();
    std::fs::copy(&jpg, data_root.join("keep.jpg")).unwrap();
    let own_root = env.add_root(&data_root, RootMode::Once);
    env.scan(&own_root);
    let own_names: Vec<_> = env
        .photos_under(&data_root)
        .into_iter()
        .map(|p| p.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(
        own_names,
        ["keep.jpg"],
        "own data/cache dirs are never indexed"
    );
}

#[test]
fn scan_emits_progress_and_finished() {
    let env = env();
    let root = env.add_root(&generated_basic().canonicalize().unwrap(), RootMode::Once);
    let (summary, events) = env.scan(&root);
    let Some(ScanEvent::Finished {
        root: finished_root,
        summary: finished,
    }) = events.last()
    else {
        panic!("last event must be Finished: {events:?}");
    };
    assert_eq!((*finished_root, *finished), (root.id, summary));
    assert_eq!(
        events
            .iter()
            .filter(|e| matches!(e, ScanEvent::Finished { .. }))
            .count(),
        1
    );
    let progress: Vec<_> = events
        .iter()
        .filter_map(|e| match e {
            ScanEvent::Progress {
                root: r,
                done,
                total_estimate,
            } => {
                assert_eq!(*r, root.id);
                Some((*done, *total_estimate))
            }
            ScanEvent::Finished { .. } => None,
        })
        .collect();
    assert!(progress.len() >= 2, "{progress:?}");
    assert!(
        progress.windows(2).all(|w| w[0].0 <= w[1].0),
        "monotonic: {progress:?}"
    );
    assert!(progress.iter().all(|(d, t)| d <= t));
    let total = summary.added + summary.unchanged;
    assert_eq!(*progress.last().unwrap(), (total, total));
}

#[test]
fn progress_is_throttled_by_interval() {
    let env = env_with(ScanOptions {
        progress_interval: Duration::from_secs(3600),
        batch_size: 1,
        ..ScanOptions::default()
    });
    let root = env.add_root(&generated_basic().canonicalize().unwrap(), RootMode::Once);
    let (_, events) = env.scan(&root);
    let progress = events
        .iter()
        .filter(|e| matches!(e, ScanEvent::Progress { .. }))
        .count();
    assert_eq!(
        progress, 1,
        "only the final progress event is forced: {events:?}"
    );
}

#[test]
fn changes_deletions_and_returns_are_tracked() {
    let env = env();
    let dir = env.subdir("lib");
    let src = generated_basic().join("2021/beach");
    for n in ["beach_01.jpg", "beach_02.jpg", "beach_03.jpg"] {
        std::fs::copy(src.join(n), dir.join(n)).unwrap();
    }
    let root = env.add_root(&dir, RootMode::Watch);
    assert_eq!(env.scan(&root).0.added, 3);

    std::fs::copy(src.join("beach_04.jpg"), dir.join("beach_01.jpg")).unwrap();
    std::fs::remove_file(dir.join("beach_02.jpg")).unwrap();
    std::fs::copy(src.join("beach_05.jpg"), dir.join("new.jpg")).unwrap();
    let (s, _) = env.scan(&root);
    assert_eq!(
        (s.added, s.updated, s.missing, s.unchanged),
        (1, 1, 1, 1),
        "{s:?}"
    );
    let by_name = |n: &str| {
        env.photos_under(&dir)
            .into_iter()
            .find(|p| p.path.ends_with(n))
            .unwrap()
    };
    assert_eq!(by_name("beach_02.jpg").status, PhotoStatus::Missing);
    assert_eq!(
        by_name("beach_01.jpg")
            .taken_at
            .unwrap()
            .format("%d")
            .to_string(),
        "04"
    );

    std::fs::copy(src.join("beach_02.jpg"), dir.join("beach_02.jpg")).unwrap();
    let (s, _) = env.scan(&root);
    assert_eq!((s.added, s.updated, s.missing), (0, 1, 0), "{s:?}");
    assert_eq!(by_name("beach_02.jpg").status, PhotoStatus::Ok);
    let again = env.scan(&root).0;
    assert_eq!(again.missing, 0, "already-missing rows are not re-counted");
}

#[test]
fn user_fields_survive_rescans_of_changed_files() {
    let env = env();
    let dir = env.subdir("lib");
    let src = generated_basic().join("2021/beach");
    std::fs::copy(src.join("beach_01.jpg"), dir.join("a.jpg")).unwrap();
    let root = env.add_root(&dir, RootMode::Watch);
    env.scan(&root);
    let id = env.photos_under(&dir)[0].id;
    env.catalog.set_caption(&[id], Some("mine".into())).unwrap();
    std::fs::copy(src.join("beach_02.jpg"), dir.join("a.jpg")).unwrap();
    assert_eq!(env.scan(&root).0.updated, 1);
    let photo = env.catalog.get_photo(id).unwrap().unwrap();
    assert_eq!(photo.caption.as_deref(), Some("mine"));
    assert!(photo.gps.is_some(), "metadata was refreshed");
}

#[test]
fn cancelled_scan_stops_and_keeps_rows_present() {
    let env = env_with(ScanOptions {
        batch_size: 2,
        progress_interval: Duration::ZERO,
        ..ScanOptions::default()
    });
    let dir = env.subdir("lib");
    fixtures::copy_tree(&generated_basic(), &dir).unwrap();
    let root = env.add_root(&dir, RootMode::Once);
    env.scan(&root);
    std::fs::remove_file(dir.join("2021/beach/beach_01.jpg")).unwrap();
    let cancel = AtomicBool::new(true);
    let summary = env.scanner.scan_root(&root, &cancel, &mut |_| {}).unwrap();
    assert!(summary.cancelled);
    assert_eq!(
        summary.missing, 0,
        "a cancelled scan must not mark anything missing"
    );
}

#[test]
fn exclude_roots_index_nothing() {
    let env = env();
    let dir = env.subdir("lib");
    std::fs::copy(
        generated_basic().join("2021/beach/beach_01.jpg"),
        dir.join("a.jpg"),
    )
    .unwrap();
    let root = env.add_root(&dir, RootMode::Exclude);
    let (summary, events) = env.scan(&root);
    assert_eq!(summary, lb_scan::ScanSummary::default());
    assert!(matches!(events.as_slice(), [ScanEvent::Finished { .. }]));
    assert!(env.photos_under(&dir).is_empty());
}

#[derive(Debug)]
struct FakeProber;

impl lb_media::VideoProber for FakeProber {
    fn probe(&self, path: &std::path::Path) -> lb_media::Result<lb_media::VideoMetadata> {
        if path.file_name().is_some_and(|n| n == "broken.mp4") {
            return Err(lb_media::MediaError::Corrupt("no moov atom".into()));
        }
        Ok(lb_media::VideoMetadata {
            width: 1080,
            height: 1920,
            duration_ms: 4_200,
            creation_time: chrono::NaiveDate::from_ymd_opt(2022, 3, 4)
                .and_then(|d| d.and_hms_opt(5, 6, 7)),
            location: Some(lb_core::GeoPoint { lat: 1.5, lon: 2.5 }),
        })
    }
}

fn video_env(prober: Option<std::sync::Arc<dyn lb_media::VideoProber>>) -> Env {
    env_with(ScanOptions {
        progress_interval: Duration::ZERO,
        caps: lb_media::Capabilities {
            video: true,
            ..lb_media::Capabilities::default()
        },
        video_prober: prober,
        ..ScanOptions::default()
    })
}

fn video_root(env: &Env) -> std::path::PathBuf {
    let dir = env.subdir("videos");
    for name in ["clip.mp4", "broken.mp4"] {
        std::fs::write(dir.join(name), b"\0\0\0\x18ftypisom0000").unwrap();
    }
    dir
}

#[test]
fn videos_are_indexed_with_probed_metadata() {
    let env = video_env(Some(std::sync::Arc::new(FakeProber)));
    let root = env.add_root(&video_root(&env), RootMode::Once);
    let (summary, _) = env.scan(&root);
    assert_eq!((summary.added, summary.errors), (2, 0), "{summary:?}");
    let photos = env.photos_under(&env.subdir("videos"));
    let clip = photos
        .iter()
        .find(|p| p.path.ends_with("clip.mp4"))
        .unwrap();
    assert_eq!(clip.media_type, lb_core::MediaType::Video);
    assert_eq!((clip.width, clip.height), (Some(1080), Some(1920)));
    assert_eq!(clip.duration_ms, Some(4_200));
    assert_eq!(clip.taken_at_src, Some(TakenAtSource::Video));
    assert_eq!(clip.gps, Some(lb_core::GeoPoint { lat: 1.5, lon: 2.5 }));
    let broken = photos
        .iter()
        .find(|p| p.path.ends_with("broken.mp4"))
        .unwrap();
    assert_eq!(
        broken.status,
        PhotoStatus::Ok,
        "a failed probe does not hide the file"
    );
    assert_eq!(broken.duration_ms, None);
}

#[test]
fn videos_are_skipped_unless_enabled_and_listed_without_a_prober() {
    let env = env();
    let root = env.add_root(&video_root(&env), RootMode::Once);
    assert_eq!(env.scan(&root).0.added, 0, "videos are off by default");

    let env = video_env(None);
    let root = env.add_root(&video_root(&env), RootMode::Once);
    assert_eq!(env.scan(&root).0.added, 2);
    assert!(
        env.photos_under(&env.subdir("videos"))
            .iter()
            .all(|p| p.duration_ms.is_none())
    );
}
