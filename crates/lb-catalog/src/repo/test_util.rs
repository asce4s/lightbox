//! Shared helpers for repository tests.

use super::{NewFolder, PhotoUpsert};
use crate::Catalog;
use chrono::{NaiveDate, NaiveDateTime};
use lb_core::{Folder, Format, PhotoId, Root, RootMode};
use std::path::Path;

pub fn now() -> NaiveDateTime {
    NaiveDate::from_ymd_opt(2026, 10, 17)
        .and_then(|d| d.and_hms_opt(8, 30, 0))
        .unwrap()
}

pub fn open() -> (tempfile::TempDir, Catalog) {
    let dir = tempfile::tempdir().unwrap();
    let catalog = Catalog::open(&dir.path().join("catalog.sqlite3")).unwrap();
    (dir, catalog)
}

pub fn root_and_folder(catalog: &Catalog) -> (Root, Folder) {
    let root = catalog
        .add_root(Path::new("/photos"), RootMode::Watch, now())
        .unwrap();
    let folder = catalog
        .ensure_folder(NewFolder {
            root_id: root.id,
            path: "/photos".into(),
            parent_id: None,
        })
        .unwrap();
    (root, folder)
}

pub fn upsert(folder: &Folder, name: &str) -> PhotoUpsert {
    PhotoUpsert::new(
        folder.id,
        name,
        Format::Jpeg,
        1000,
        1_577_836_800,
        format!("qk-{name}"),
    )
}

pub fn insert_photos(catalog: &Catalog, folder: &Folder, n: usize) -> Vec<PhotoId> {
    let rows = (0..n)
        .map(|i| upsert(folder, &format!("p{i:04}.jpg")))
        .collect();
    catalog
        .upsert_photos(rows, now())
        .unwrap()
        .into_iter()
        .map(|o| o.id)
        .collect()
}
