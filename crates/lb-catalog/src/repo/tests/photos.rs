use crate::repo::test_util::*;
use crate::{PhotoUpsert, UpsertOutcome};
use chrono::NaiveDate;
use lb_core::{Format, GeoPoint, MediaType, PhotoId, PhotoStatus, TakenAtSource};
use std::path::Path;
use std::time::{Duration, Instant};

fn full_row(folder: &lb_core::Folder) -> PhotoUpsert {
    let mut row = upsert(folder, "beach_01.jpg");
    row.width = Some(640);
    row.height = Some(480);
    row.orientation = 6;
    row.taken_at = NaiveDate::from_ymd_opt(2021, 7, 1).and_then(|d| d.and_hms_opt(12, 0, 0));
    row.taken_at_src = Some(TakenAtSource::Exif);
    row.camera_make = Some("Canon".into());
    row.camera_model = Some("Canon EOS R6".into());
    row.lens = Some("RF24-105mm".into());
    row.iso = Some(100);
    row.f_number = Some(4.0);
    row.exposure_s = Some(0.004);
    row.focal_mm = Some(50.0);
    row.gps = Some(GeoPoint {
        lat: 38.7223,
        lon: -9.1393,
    });
    row.gps_alt = Some(12.5);
    row.import_caption = Some("At the beach".into());
    row
}

#[test]
fn photo_upsert_batch_500() {
    let (_dir, catalog) = open();
    let (_, folder) = root_and_folder(&catalog);
    let rows: Vec<_> = (0..500)
        .map(|i| upsert(&folder, &format!("img_{i:04}.jpg")))
        .collect();

    let start = Instant::now();
    let first = catalog.upsert_photos(rows.clone(), now()).unwrap();
    assert!(
        start.elapsed() < Duration::from_secs(5),
        "one transaction: {:?}",
        start.elapsed()
    );
    assert_eq!(first.len(), 500);
    assert!(first.iter().all(|o| o.inserted));
    let ids: std::collections::HashSet<_> = first.iter().map(|o| o.id).collect();
    assert_eq!(ids.len(), 500, "distinct ids");

    let second = catalog.upsert_photos(rows, now()).unwrap();
    assert_eq!(
        second,
        first
            .iter()
            .map(|o| UpsertOutcome {
                id: o.id,
                inserted: false
            })
            .collect::<Vec<_>>()
    );
    let count: i64 = catalog
        .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM photos", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(count, 500, "idempotent");
}

#[test]
fn photo_upsert_is_atomic() {
    let (_dir, catalog) = open();
    let (_, folder) = root_and_folder(&catalog);
    let mut rows: Vec<_> = (0..10)
        .map(|i| upsert(&folder, &format!("{i}.jpg")))
        .collect();
    rows[7].folder_id = lb_core::FolderId::new(9999);
    assert!(catalog.upsert_photos(rows, now()).is_err());
    let count: i64 = catalog
        .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM photos", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(count, 0, "a failing row rolls back the whole batch");
}

#[test]
fn photo_roundtrips_all_scanner_fields() {
    let (_dir, catalog) = open();
    let (_, folder) = root_and_folder(&catalog);
    let id = catalog
        .upsert_photos(vec![full_row(&folder)], now())
        .unwrap()[0]
        .id;
    let photo = catalog.get_photo(id).unwrap().unwrap();
    assert_eq!(photo.id, id);
    assert_eq!(photo.folder_id, folder.id);
    assert_eq!(photo.path, Path::new("/photos/beach_01.jpg"));
    assert_eq!(
        (photo.media_type, photo.format),
        (MediaType::Image, Format::Jpeg)
    );
    assert_eq!(
        (photo.size, photo.file_mtime, photo.quick_key.as_str()),
        (1000, 1_577_836_800, "qk-beach_01.jpg")
    );
    assert_eq!(
        (photo.width, photo.height, photo.orientation),
        (Some(640), Some(480), 6)
    );
    assert_eq!(
        photo.taken_at,
        NaiveDate::from_ymd_opt(2021, 7, 1).and_then(|d| d.and_hms_opt(12, 0, 0))
    );
    assert_eq!(photo.taken_at_src, Some(TakenAtSource::Exif));
    assert_eq!(
        photo.camera.as_deref(),
        Some("Canon EOS R6"),
        "make is not repeated when the model has it"
    );
    assert_eq!(
        photo.gps,
        Some(GeoPoint {
            lat: 38.7223,
            lon: -9.1393
        })
    );
    assert_eq!(photo.caption.as_deref(), Some("At the beach"));
    assert_eq!(
        (
            photo.starred,
            photo.rating,
            photo.hidden,
            photo.edit_version
        ),
        (false, 0, false, 0)
    );
    assert_eq!(
        (photo.status, photo.error, photo.content_hash),
        (PhotoStatus::Ok, None, None)
    );

    let ts: String = catalog
        .read(|c| {
            Ok(c.query_row(
                "SELECT taken_at FROM photos WHERE id = ?1",
                [id.get()],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(
        ts, "2021-07-01T12:00:00",
        "ISO-8601 with T, not SQLite's space form"
    );
}

#[test]
fn camera_name_joining() {
    let (_dir, catalog) = open();
    let (_, folder) = root_and_folder(&catalog);
    let cases = [
        (Some("Google"), Some("Pixel 7"), Some("Google Pixel 7")),
        (Some("Canon"), Some("canon eos"), Some("canon eos")),
        (Some("Nikon"), None, Some("Nikon")),
        (None, Some("X100"), Some("X100")),
        (Some("  "), Some(" "), None),
        (None, None, None),
    ];
    for (i, (make, model, expected)) in cases.iter().enumerate() {
        let mut row = upsert(&folder, &format!("c{i}.jpg"));
        row.camera_make = make.map(str::to_owned);
        row.camera_model = model.map(str::to_owned);
        let id = catalog.upsert_photos(vec![row], now()).unwrap()[0].id;
        assert_eq!(
            catalog.get_photo(id).unwrap().unwrap().camera.as_deref(),
            *expected,
            "{make:?} {model:?}"
        );
    }
}

#[test]
fn upsert_preserves_user_fields_and_id() {
    let (_dir, catalog) = open();
    let (_, folder) = root_and_folder(&catalog);
    let id = catalog
        .upsert_photos(vec![upsert(&folder, "a.jpg")], now())
        .unwrap()[0]
        .id;
    catalog
        .write(move |c| {
            Ok(c.execute(
                "UPDATE photos SET starred = 1, rating = 4, hidden = 1, caption = 'mine', edit_version = 7 WHERE id = ?1",
                [id.get()],
            )?)
        })
        .unwrap();

    let mut rescanned = upsert(&folder, "a.jpg");
    rescanned.file_size = 2222;
    rescanned.width = Some(10);
    rescanned.import_caption = Some("from exif".into());
    let out = catalog.upsert_photos(vec![rescanned], now()).unwrap();
    assert_eq!(
        out,
        [UpsertOutcome {
            id,
            inserted: false
        }]
    );

    let p = catalog.get_photo(id).unwrap().unwrap();
    assert_eq!(
        (p.starred, p.rating, p.hidden, p.edit_version),
        (true, 4, true, 7)
    );
    assert_eq!(
        p.caption.as_deref(),
        Some("mine"),
        "an existing caption is not replaced by metadata"
    );
    assert_eq!(
        (p.size, p.width),
        (2222, Some(10)),
        "scanner fields refresh"
    );
}

#[test]
fn upsert_import_caption_fills_empty_caption_only() {
    let (_dir, catalog) = open();
    let (_, folder) = root_and_folder(&catalog);
    let mut row = upsert(&folder, "a.jpg");
    let id = catalog.upsert_photos(vec![row.clone()], now()).unwrap()[0].id;
    assert_eq!(catalog.get_photo(id).unwrap().unwrap().caption, None);
    row.import_caption = Some("from exif".into());
    catalog.upsert_photos(vec![row.clone()], now()).unwrap();
    assert_eq!(
        catalog.get_photo(id).unwrap().unwrap().caption.as_deref(),
        Some("from exif")
    );
    row.import_caption = Some("changed in file".into());
    catalog.upsert_photos(vec![row], now()).unwrap();
    assert_eq!(
        catalog.get_photo(id).unwrap().unwrap().caption.as_deref(),
        Some("from exif")
    );
}

#[test]
fn upsert_clears_hashes_when_quick_key_changes() {
    let (_dir, catalog) = open();
    let (_, folder) = root_and_folder(&catalog);
    let row = upsert(&folder, "a.jpg");
    let id = catalog.upsert_photos(vec![row.clone()], now()).unwrap()[0].id;
    catalog
        .write(move |c| {
            Ok(c.execute(
                "UPDATE photos SET content_hash = 'full', phash = 42 WHERE id = ?1",
                [id.get()],
            )?)
        })
        .unwrap();
    let hash = |catalog: &crate::Catalog| {
        catalog
            .read(|c| {
                Ok(c.query_row(
                    "SELECT content_hash, phash FROM photos WHERE id = ?1",
                    [id.get()],
                    |r| Ok((r.get::<_, Option<String>>(0)?, r.get::<_, Option<i64>>(1)?)),
                )?)
            })
            .unwrap()
    };

    catalog.upsert_photos(vec![row.clone()], now()).unwrap();
    assert_eq!(
        hash(&catalog),
        (Some("full".into()), Some(42)),
        "same quick_key keeps hashes"
    );

    let mut changed = row;
    changed.quick_key = "different".into();
    catalog.upsert_photos(vec![changed], now()).unwrap();
    assert_eq!(
        hash(&catalog),
        (None, None),
        "changed content invalidates hashes"
    );
}

#[test]
fn get_many_preserves_order() {
    let (_dir, catalog) = open();
    let (_, folder) = root_and_folder(&catalog);
    let ids = insert_photos(&catalog, &folder, 1200);

    let mut wanted: Vec<PhotoId> = ids.iter().rev().step_by(3).copied().collect();
    wanted.push(PhotoId::new(999_999));
    wanted.push(ids[5]);
    wanted.push(ids[5]);
    wanted.swap(0, 10);
    let expected: Vec<_> = wanted
        .iter()
        .copied()
        .filter(|id| *id != PhotoId::new(999_999))
        .collect();

    let got: Vec<_> = catalog
        .get_many(&wanted)
        .unwrap()
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(
        got, expected,
        "request order kept, unknown skipped, duplicates repeated, across 500-id chunks"
    );
    assert!(catalog.get_many(&[]).unwrap().is_empty());
    assert!(catalog.get_photo(PhotoId::new(999_999)).unwrap().is_none());
}

#[test]
fn photo_by_path_and_quick_key() {
    let (_dir, catalog) = open();
    let (root, folder) = root_and_folder(&catalog);
    let sub = catalog
        .ensure_folder(crate::NewFolder {
            root_id: root.id,
            path: "/photos/2021".into(),
            parent_id: Some(folder.id),
        })
        .unwrap();
    let mut a = upsert(&sub, "café ☕.jpg");
    a.quick_key = "same".into();
    let mut b = upsert(&folder, "copy.jpg");
    b.quick_key = "same".into();
    let ids = catalog
        .upsert_photos(vec![a, b, upsert(&folder, "other.jpg")], now())
        .unwrap();

    let found = catalog
        .photo_by_path(Path::new("/photos/2021/café ☕.jpg"))
        .unwrap()
        .unwrap();
    assert_eq!(found.id, ids[0].id);
    assert!(
        catalog
            .photo_by_path(Path::new("/photos/2021/copy.jpg"))
            .unwrap()
            .is_none(),
        "file name alone is not enough"
    );
    assert!(
        catalog
            .photo_by_path(Path::new("/photos/missing.jpg"))
            .unwrap()
            .is_none()
    );
    assert!(catalog.photo_by_path(Path::new("/")).unwrap().is_none());

    let same: Vec<_> = catalog
        .photos_by_quick_key("same")
        .unwrap()
        .into_iter()
        .map(|p| p.id)
        .collect();
    assert_eq!(same, [ids[0].id, ids[1].id]);
    assert!(catalog.photos_by_quick_key("nope").unwrap().is_empty());
}

#[test]
fn unreadable_status_and_error_are_stored() {
    let (_dir, catalog) = open();
    let (_, folder) = root_and_folder(&catalog);
    let mut row = upsert(&folder, "broken.jpg");
    row.status = PhotoStatus::Unreadable;
    row.error = Some("truncated JPEG".into());
    let id = catalog.upsert_photos(vec![row], now()).unwrap()[0].id;
    let p = catalog.get_photo(id).unwrap().unwrap();
    assert_eq!(
        (p.status, p.error.as_deref()),
        (PhotoStatus::Unreadable, Some("truncated JPEG"))
    );
}

#[test]
fn flags_are_set_in_batches_and_validated() {
    let (_dir, catalog) = open();
    let (_root, folder) = root_and_folder(&catalog);
    let ids = insert_photos(&catalog, &folder, 3);
    assert_eq!(catalog.set_starred(&ids[..2], true).unwrap(), 2);
    assert_eq!(catalog.set_rating(&ids, 4).unwrap(), 3);
    assert_eq!(catalog.set_hidden(&ids[2..], true).unwrap(), 1);
    let photos = catalog.get_many(&ids).unwrap();
    assert_eq!(
        photos.iter().map(|p| p.starred).collect::<Vec<_>>(),
        [true, true, false]
    );
    assert!(photos.iter().all(|p| p.rating == 4));
    assert_eq!(
        photos.iter().map(|p| p.hidden).collect::<Vec<_>>(),
        [false, false, true]
    );
    assert!(
        catalog.set_rating(&ids, 6).is_err(),
        "schema CHECK rejects ratings above 5"
    );
    assert!(
        catalog
            .get_many(&ids)
            .unwrap()
            .iter()
            .all(|p| p.rating == 4),
        "failed batch changes nothing"
    );
    catalog.set_starred(&ids, false).unwrap();
    assert!(catalog.get_many(&ids).unwrap().iter().all(|p| !p.starred));
}

#[test]
fn all_photo_ids_ascending() {
    let (_dir, catalog) = open();
    let (_, folder) = root_and_folder(&catalog);
    assert!(catalog.all_photo_ids().unwrap().is_empty());
    let ids = insert_photos(&catalog, &folder, 5);
    assert_eq!(catalog.all_photo_ids().unwrap(), ids);
}
