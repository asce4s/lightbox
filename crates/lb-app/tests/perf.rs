//! M11.1: performance budgets of SPEC §11 for the catalog, search and scanner.
//! Smoke-sized unless `LIGHTBOX_PERF` is set (see `lb_testkit::perf`).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use chrono::{Duration as Days, NaiveDate, NaiveDateTime};
use lb_app::{Lightbox, LightboxOptions, ViewQuery, ViewSource};
use lb_catalog::{NewFolder, PhotoUpsert};
use lb_core::{Format, PhotoId, RootMode};
use lb_jobs::JobQueue;
use lb_testkit::perf::{check, check_memory, scale, time};
use std::path::Path;
use std::time::Duration;

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
