//! M11.1: performance budgets of SPEC §11 for the catalog, search and scanner.
//! Smoke-sized unless `LIGHTBOX_PERF` is set (see `lb_testkit::perf`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use chrono::{Duration as Days, NaiveDate, NaiveDateTime};
use lb_app::viewer::{
    LoadLevel, MEMORY_BUDGET, MediaBytes, Motion, MotionMode, RectF, ViewerLoader,
};
use lb_app::{Lightbox, LightboxOptions, ViewQuery, ViewSource, capture_viewer_perf};
use lb_catalog::{NewFolder, PhotoUpsert};
use lb_core::{Format, PhotoId, RootMode};
use lb_jobs::JobQueue;
use lb_testkit::perf::{check, check_memory, enforced, scale, time};
use std::path::Path;
use std::time::{Duration, Instant};

const FOLDERS: usize = 100;

fn options() -> LightboxOptions {
    LightboxOptions {
        spawn_event_ticker: false,
        ..LightboxOptions::default()
    }
}

fn start() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2015, 1, 1)
        .and_then(|d| d.and_hms_opt(12, 0, 0))
        .unwrap()
}

/// Inserts `n` photos directly into the catalog: 1 in 10 is named `beach_*`, takes are spread
/// over ten years, every third photo is rated 3 or more.
fn populate(lb: &Lightbox, n: usize) -> Vec<PhotoId> {
    let catalog = lb.catalog_handle();
    let root_path = Path::new("/perf-library");
    let root = catalog
        .add_root(root_path, RootMode::Watch, start())
        .unwrap();
    let folders: Vec<_> = (0..FOLDERS)
        .map(|i| {
            catalog
                .ensure_folder(NewFolder {
                    root_id: root.id,
                    path: root_path.join(format!("f{i:03}")),
                    parent_id: None,
                })
                .unwrap()
        })
        .collect();
    let step = (10 * 365 * 24 * 60 / n.max(1)) as i64;
    let mut ids = Vec::with_capacity(n);
    for chunk in (0..n).collect::<Vec<_>>().chunks(10_000) {
        let rows = chunk
            .iter()
            .map(|&i| {
                let name = if i % 10 == 0 {
                    format!("beach_{i}.jpg")
                } else {
                    format!("img_{i}.jpg")
                };
                let mut row = PhotoUpsert::new(
                    folders[i % FOLDERS].id,
                    name,
                    Format::Jpeg,
                    1000,
                    1_577_836_800,
                    format!("qk{i}"),
                );
                row.taken_at = Some(start() + Days::minutes(i as i64 * step));
                row
            })
            .collect();
        ids.extend(
            catalog
                .upsert_photos(rows, start())
                .unwrap()
                .into_iter()
                .map(|o| o.id),
        );
    }
    let rated: Vec<PhotoId> = ids.iter().copied().step_by(3).collect();
    for chunk in rated.chunks(5000) {
        catalog.set_rating(chunk, 4).unwrap();
    }
    ids
}

#[test]
fn open_first_grid_and_search_100k_under_budget() {
    let n = scale(100_000, 2_000);
    let home = tempfile::tempdir().unwrap();
    let ids = {
        let lb = Lightbox::open_with(home.path(), JobQueue::inline(), options()).unwrap();
        populate(&lb, n)
    };
    assert_eq!(ids.len(), n);

    let (lb, open) = time(|| Lightbox::open_with(home.path(), JobQueue::inline(), options()));
    let lb = lb.unwrap();
    let (grid, first_query) = time(|| {
        lb.search()
            .query(&ViewQuery::new(ViewSource::AllPhotos))
            .unwrap()
    });
    assert_eq!(grid.ids().len(), n);
    check(
        "open_and_first_grid_query",
        open + first_query,
        Duration::from_millis(300),
    );

    let mut view = ViewQuery::new(ViewSource::AllPhotos);
    view.search = "beach rating:>=3 year:2021".into();
    let (hits, search) = time(|| lb.search().query(&view).unwrap());
    let found = hits.ids().len();
    assert!(found > 0 && found < n / 10, "{found} matches");
    for p in lb.photos().get_many(&hits.ids()[..found.min(50)]).unwrap() {
        assert!(p.starred || p.rating >= 3);
        assert!(
            p.path
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with("beach")
        );
    }
    check("search_100k", search, Duration::from_millis(100));

    let slice = &ids[n / 2..n / 2 + 200];
    let (rows, get_many) = time(|| lb.photos().get_many(slice).unwrap());
    assert_eq!(rows.len(), 200);
    check("get_many_200", get_many, Duration::from_millis(10));

    check_memory("idle_memory_100k", 600.0);
}

fn write_small_jpegs(dir: &Path, n: usize) {
    let base = image::RgbImage::from_fn(48, 32, |x, y| image::Rgb([x as u8 * 5, y as u8 * 7, 90]));
    for i in 0..n {
        let sub = dir.join(format!("d{:02}", i % 20));
        std::fs::create_dir_all(&sub).unwrap();
        let mut img = base.clone();
        img.put_pixel(0, 0, image::Rgb([(i % 251) as u8, (i / 251) as u8, 7]));
        img.save(sub.join(format!("img_{i:05}.jpg"))).unwrap();
    }
}

#[test]
fn scan_2000_fixtures_under_budget() {
    let n = scale(2_000, 60);
    let home = tempfile::tempdir().unwrap();
    let photos = tempfile::tempdir().unwrap();
    write_small_jpegs(photos.path(), n);
    let lb = Lightbox::open_with(home.path(), JobQueue::inline(), options()).unwrap();

    let (change, scan) = time(|| {
        lb.library()
            .add_root(photos.path(), RootMode::Watch)
            .unwrap()
    });
    let summary = change.scan.unwrap();
    assert_eq!(summary.added, n);
    check("scan_2000_cold", scan, Duration::from_secs(15));

    let root = change.root.unwrap();
    let (summary, rescan) = time(|| lb.library().rescan(root.id).unwrap());
    assert_eq!(
        (summary.added, summary.updated, summary.unchanged),
        (0, 0, n)
    );
    check("rescan_2000_unchanged", rescan, Duration::from_secs(2));
}

struct Counting {
    reads: Vec<(usize, LoadLevel)>,
}

impl MediaBytes for Counting {
    fn read(&mut self, index: usize, level: LoadLevel) -> lb_app::Result<u64> {
        self.reads.push((index, level));
        Ok(1_000)
    }
}

fn bench_jpeg(dir: &Path) -> std::path::PathBuf {
    let (w, h) = if enforced() { (6000, 4000) } else { (64, 48) };
    let path = dir.join("bench.jpg");
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([(x % 251) as u8, (y % 251) as u8, 40])
    });
    img.save(&path).unwrap();
    path
}

/// §V12: no file decode between open and the first frame, and that work stays within 4 ms.
#[test]
fn viewer_open_frame_under_budget() {
    let home = tempfile::tempdir().unwrap();
    let lb = Lightbox::open_with(home.path(), JobQueue::inline(), options()).unwrap();
    let ids = populate(&lb, 1);
    let mut media = Counting { reads: Vec::new() };
    let loader = ViewerLoader::open(1, 0, &[true], &[false]);
    assert!(media.reads.is_empty(), "opening the loader decodes nothing");
    let _ = &mut media;
    let _ = loader.shown(0);
    let (_, elapsed) = time(|| {
        let mut vm = lb_app::vm::ViewerVm::open(&lb, ids.clone(), ids[0]).unwrap();
        vm.set_stage((1280.0, 800.0));
        vm.begin_open(
            Duration::ZERO,
            None,
            RectF {
                x: 24.0,
                y: 48.0,
                w: 800.0,
                h: 600.0,
            },
        );
        vm.presented(Duration::ZERO)
    });
    check("viewer_open_frame", elapsed, Duration::from_millis(4));
}

/// §V12: L2 of a 24 MP JPEG (smoke-sized unless `LIGHTBOX_PERF` is set).
#[test]
fn viewer_l2_24mp_under_budget() {
    let dir = tempfile::tempdir().unwrap();
    let path = bench_jpeg(dir.path());
    let (_, elapsed) = time(|| {
        let image = lb_media::decode(&path, Some(2560)).unwrap();
        assert!(image.width() > 0);
    });
    check("viewer_l2_24mp", elapsed, Duration::from_millis(150));
}

/// §V12: a preloaded neighbour is already L2, so navigation decodes nothing.
#[test]
fn viewer_preloaded_neighbour_under_budget() {
    let mut media = Counting { reads: Vec::new() };
    let mut loader = ViewerLoader::open(3, 0, &[true; 3], &[false; 3]);
    loader.poll(Duration::ZERO, &mut media);
    loader.poll(Duration::ZERO, &mut media);
    assert!(media.reads.contains(&(1, LoadLevel::L2)));
    let before = media.reads.len();
    let (_, elapsed) = time(|| {
        loader.navigate(1, Duration::from_millis(1_000));
        assert_eq!(loader.shown(1), Some(LoadLevel::L2));
        loader.poll(Duration::from_millis(1_000), &mut media);
    });
    assert!(
        !media.reads[before..].contains(&(1, LoadLevel::L2)),
        "a preloaded neighbour is not decoded again"
    );
    check(
        "viewer_preloaded_neighbour",
        elapsed,
        Duration::from_millis(4),
    );
}

/// §V12: library already open, so the image does not pay for process start.
#[test]
fn viewer_warm_launch_image_under_budget() {
    let home = tempfile::tempdir().unwrap();
    let photos = tempfile::tempdir().unwrap();
    let path = bench_jpeg(photos.path());
    let lb = Lightbox::open_with(home.path(), JobQueue::inline(), options()).unwrap();
    let origin = Instant::now();
    let (_, elapsed) = time(|| capture_viewer_perf(&lb, &path, origin).unwrap());
    check(
        "viewer_warm_launch_image",
        elapsed,
        Duration::from_millis(150),
    );
}

/// §V12: one transition sample (p95 of 100) stays within 4 ms.
#[test]
fn viewer_transition_frame_under_budget() {
    let mut motion = Motion::new(MotionMode::Full, (1920.0, 1080.0));
    motion.open_plain(
        Duration::ZERO,
        RectF {
            x: 40.0,
            y: 80.0,
            w: 1200.0,
            h: 800.0,
        },
    );
    let mut samples = Vec::with_capacity(100);
    for step in 0..100 {
        let now = Duration::from_millis(step * 3);
        let (_, elapsed) = time(|| motion.presented(now));
        samples.push(elapsed);
    }
    samples.sort();
    let p95 = samples[94];
    check("viewer_transition_frame", p95, Duration::from_millis(4));
}

/// §V12: decoded viewer images stay inside the 512 MB cap.
#[test]
fn viewer_decoded_memory_under_budget() {
    assert_eq!(MEMORY_BUDGET, 512 * 1024 * 1024);
    check_memory("viewer_decoded_image", 512.0);
}

/// `lightbox view --perf-json` fields, produced without a window.
#[test]
fn viewer_perf_json_reports_milestones() {
    let home = tempfile::tempdir().unwrap();
    let photos = tempfile::tempdir().unwrap();
    let path = photos.path().join("one.jpg");
    image::RgbImage::from_pixel(32, 24, image::Rgb([10, 20, 30]))
        .save(&path)
        .unwrap();
    let lb = Lightbox::open_with(home.path(), JobQueue::inline(), options()).unwrap();
    let report = capture_viewer_perf(&lb, &path, Instant::now()).unwrap();
    assert!(report.process_start_ms <= report.first_frame_ms);
    assert!(report.first_frame_ms <= report.first_image_ms);
    assert_eq!(report.first_image_ms, report.l2_ms);
    let json = report.to_json();
    for key in [
        "process_start_ms",
        "first_frame_ms",
        "first_image_ms",
        "l2_ms",
    ] {
        assert!(json.contains(&format!("\"{key}\"")), "{json}");
    }
}
