//! Full-text search over a 20k-photo catalog (SPEC §11).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use criterion::{Criterion, criterion_group, criterion_main};
use lb_catalog::{Catalog, NewFolder, PhotoQuery, PhotoUpsert, QuerySource};
use lb_core::{Format, RootMode};
use std::hint::black_box;
use std::path::Path;

fn bench(c: &mut Criterion) {
    let dir = tempfile::tempdir().unwrap();
    let catalog = Catalog::open(&dir.path().join("catalog.sqlite3")).unwrap();
    let now = chrono::NaiveDate::from_ymd_opt(2026, 1, 1)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .unwrap();
    let root = catalog
        .add_root(Path::new("/bench"), RootMode::Watch, now)
        .unwrap();
    let folder = catalog
        .ensure_folder(NewFolder {
            root_id: root.id,
            path: "/bench".into(),
            parent_id: None,
        })
        .unwrap();
    let rows = (0..20_000)
        .map(|i| {
            let name = if i % 10 == 0 {
                format!("beach_{i}.jpg")
            } else {
                format!("img_{i}.jpg")
            };
            PhotoUpsert::new(folder.id, name, Format::Jpeg, 1000, 0, format!("qk{i}"))
        })
        .collect();
    catalog.upsert_photos(rows, now).unwrap();

    let mut query = PhotoQuery::new(QuerySource::All);
    query.fts = Some("\"beach\"*".into());
    c.bench_function("fts/beach_prefix", |b| {
        b.iter(|| catalog.query_photos(black_box(&query)).unwrap());
    });
}

criterion_group!(benches, bench);
criterion_main!(benches);
