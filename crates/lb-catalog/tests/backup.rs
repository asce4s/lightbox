//! Backups, rotation, integrity checks and restore (SPEC §10).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use chrono::NaiveDate;
use lb_catalog::{
    BACKUPS_KEPT, Catalog, CatalogError, backup_file_name, check_file, list_backups,
    restore_from_backup,
};
use std::fs;
use std::io::{Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

fn day(d: u32) -> NaiveDate {
    NaiveDate::from_ymd_opt(2026, 10, d).unwrap()
}

fn setting_count(catalog: &Catalog) -> i64 {
    catalog
        .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM settings", [], |r| r.get(0))?))
        .unwrap()
}

fn names(dir: &Path) -> Vec<String> {
    let mut v: Vec<_> = fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().into_string().unwrap())
        .collect();
    v.sort();
    v
}

fn open(dir: &Path) -> (PathBuf, Catalog) {
    let path = dir.join("data").join("catalog.sqlite3");
    let catalog = Catalog::open(&path).unwrap();
    (path, catalog)
}

fn fill(catalog: &Catalog, rows: usize) {
    catalog
        .write_tx(move |tx| {
            for i in 0..rows {
                tx.execute(
                    "INSERT INTO settings VALUES (?1, ?2)",
                    (format!("key-{i}"), "x".repeat(200)),
                )?;
            }
            Ok(())
        })
        .unwrap();
}

#[test]
fn catalog_backup_rotation() {
    let dir = tempfile::tempdir().unwrap();
    let (_, catalog) = open(dir.path());
    let backups = dir.path().join("backups");

    fs::create_dir_all(&backups).unwrap();
    for stray in [
        "notes.txt",
        "catalog-latest.sqlite3",
        "catalog-2026-13-45.sqlite3",
        "catalog-2026-1-5.sqlite3",
        "catalog-2026-10-02.sqlite3.bak",
    ] {
        fs::write(backups.join(stray), b"keep me").unwrap();
    }
    fs::write(
        backups.join("catalog-2026-09-30.sqlite3.tmp"),
        b"crashed half-written backup",
    )
    .unwrap();

    let mut taken = Vec::new();
    for d in 1..=10 {
        catalog.set_setting(&format!("day-{d}"), "1").unwrap();
        let path = catalog
            .daily_backup(&backups, day(d), BACKUPS_KEPT)
            .unwrap()
            .expect("first backup of the day");
        assert_eq!(path, backups.join(backup_file_name(day(d))));
        taken.push(path);
    }

    let kept: Vec<_> = list_backups(&backups)
        .unwrap()
        .into_iter()
        .map(|b| b.date)
        .collect();
    assert_eq!(
        kept,
        (4..=10).rev().map(day).collect::<Vec<_>>(),
        "newest {BACKUPS_KEPT} kept, newest first"
    );
    for old in &taken[..3] {
        assert!(!old.exists(), "{} rotated out", old.display());
    }

    let dir_names = names(&backups);
    for stray in [
        "notes.txt",
        "catalog-latest.sqlite3",
        "catalog-2026-13-45.sqlite3",
        "catalog-2026-1-5.sqlite3",
        "catalog-2026-10-02.sqlite3.bak",
    ] {
        assert!(
            dir_names.contains(&stray.to_owned()),
            "{stray} untouched: {dir_names:?}"
        );
    }
    assert!(
        !dir_names.iter().any(|n| n.ends_with(".tmp")),
        "no temp files left: {dir_names:?}"
    );
    assert!(
        !dir_names
            .iter()
            .any(|n| n.ends_with("-wal") || n.ends_with("-shm")),
        "backups are single files"
    );

    for (i, path) in taken.iter().enumerate().skip(3) {
        assert!(
            check_file(path).unwrap().is_empty(),
            "{} passes quick_check",
            path.display()
        );
        let snap = Catalog::open(path).unwrap();
        assert_eq!(
            setting_count(&snap),
            i as i64 + 1,
            "backup of day {} is a snapshot of that day",
            i + 1
        );
    }

    let before = fs::read(&taken[9]).unwrap();
    catalog.set_setting("later-the-same-day", "1").unwrap();
    assert_eq!(
        catalog
            .daily_backup(&backups, day(10), BACKUPS_KEPT)
            .unwrap(),
        None,
        "one backup per day"
    );
    assert_eq!(
        fs::read(&taken[9]).unwrap(),
        before,
        "existing backup is not overwritten"
    );
}

#[test]
fn backup_rotation_orders_by_date_not_creation_order() {
    let dir = tempfile::tempdir().unwrap();
    let (_, catalog) = open(dir.path());
    let backups = dir.path().join("backups");
    for d in [20, 3, 15, 9, 27, 1] {
        catalog.daily_backup(&backups, day(d), 3).unwrap();
    }
    let kept: Vec<_> = list_backups(&backups)
        .unwrap()
        .into_iter()
        .map(|b| b.date)
        .collect();
    assert_eq!(kept, [day(27), day(20), day(15)]);
    assert_eq!(
        catalog
            .daily_backup(&backups, day(1), 0)
            .unwrap()
            .map(|_| ()),
        Some(()),
        "keep is clamped to at least 1"
    );
    assert_eq!(list_backups(&backups).unwrap().len(), 1);
    assert!(
        list_backups(&dir.path().join("missing"))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn backup_snapshot_is_consistent_while_writers_are_active() {
    let dir = tempfile::tempdir().unwrap();
    let (_, catalog) = open(dir.path());
    fill(&catalog, 200);
    let writer = {
        let catalog = catalog.clone();
        std::thread::spawn(move || {
            for i in 0..100 {
                catalog
                    .write_tx(move |tx| {
                        tx.execute("INSERT INTO settings VALUES (?1, 'a')", [format!("a-{i}")])?;
                        tx.execute("INSERT INTO settings VALUES (?1, 'b')", [format!("b-{i}")])?;
                        Ok(())
                    })
                    .unwrap();
            }
        })
    };
    let backups = dir.path().join("backups");
    let path = catalog
        .daily_backup(&backups, day(1), BACKUPS_KEPT)
        .unwrap()
        .unwrap();
    writer.join().unwrap();
    let snap = Catalog::open(&path).unwrap();
    let (a, b): (i64, i64) = snap
        .read(|c| {
            Ok(c.query_row(
                "SELECT (SELECT COUNT(*) FROM settings WHERE value_json = 'a'), (SELECT COUNT(*) FROM settings WHERE value_json = 'b')",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?)
        })
        .unwrap();
    assert_eq!(a, b, "no transaction is half in the snapshot");
    assert!(snap.quick_check().unwrap().is_empty());
}

#[test]
fn catalog_restore_from_backup() {
    let dir = tempfile::tempdir().unwrap();
    let (path, catalog) = open(dir.path());
    fill(&catalog, 50);
    let backups = dir.path().join("backups");
    let backup = catalog
        .daily_backup(&backups, day(1), BACKUPS_KEPT)
        .unwrap()
        .unwrap();
    catalog.set_setting("added-after-backup", "1").unwrap();
    assert!(catalog.quick_check().unwrap().is_empty());
    drop(catalog);

    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .unwrap()
        .write_all(&[0xA5; 200])
        .unwrap();
    let err = Catalog::open(&path).unwrap_err();
    assert!(
        err.is_corruption(),
        "a trashed header is reported as corruption: {err:?}"
    );
    assert!(!matches!(err, CatalogError::SchemaTooNew));
    fs::write(
        append(&path, "-wal"),
        b"stale wal frames that must never be replayed",
    )
    .unwrap();
    fs::write(append(&path, "-shm"), b"stale shm").unwrap();

    restore_from_backup(&path, &backup).unwrap();

    let restored = Catalog::open(&path).unwrap();
    assert!(restored.quick_check().unwrap().is_empty());
    assert_eq!(
        setting_count(&restored),
        50,
        "contents are those of the backup"
    );
    assert!(
        restored
            .get_setting("added-after-backup")
            .unwrap()
            .is_none()
    );
    assert!(
        restored
            .backup_to(&dir.path().join("again.sqlite3"))
            .is_ok(),
        "restored catalog is fully writable"
    );
    restored.set_setting("new", "1").unwrap();
    drop(restored);

    let data = path.parent().unwrap();
    let listing = names(data);
    assert!(
        listing.contains(&"catalog.sqlite3.corrupt".to_owned()),
        "damaged file kept for diagnosis: {listing:?}"
    );
    assert!(listing.contains(&"catalog.sqlite3.corrupt-wal".to_owned()));
    assert!(
        !listing.iter().any(|n| n.ends_with(".restoring")),
        "{listing:?}"
    );
    assert!(
        check_file(&backup).unwrap().is_empty(),
        "the backup itself is untouched"
    );
}

fn append(path: &Path, suffix: &str) -> PathBuf {
    let mut s = path.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

#[test]
fn quick_check_detects_page_level_corruption() {
    let dir = tempfile::tempdir().unwrap();
    let (path, catalog) = open(dir.path());
    fill(&catalog, 2000);
    assert!(catalog.quick_check().unwrap().is_empty());
    drop(catalog);

    let mut file = fs::OpenOptions::new().write(true).open(&path).unwrap();
    let len = file.metadata().unwrap().len();
    assert!(len > 40 * 4096, "enough pages to damage: {len}");
    file.seek(SeekFrom::Start(len / 2)).unwrap();
    file.write_all(&[0xFF; 8192]).unwrap();
    drop(file);

    let problems = check_file(&path).map(|p| !p.is_empty()).unwrap_or(true);
    assert!(problems, "damaged pages are detected by quick_check");
}

#[test]
fn restore_rejects_bad_backups_and_leaves_catalog_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let (path, catalog) = open(dir.path());
    catalog.set_setting("mine", "1").unwrap();
    drop(catalog);
    let before = fs::read(&path).unwrap();

    let garbage = dir.path().join("catalog-2026-10-01.sqlite3");
    fs::write(&garbage, vec![7u8; 10_000]).unwrap();
    let err = restore_from_backup(&path, &garbage).unwrap_err();
    assert!(err.is_corruption(), "{err:?}");
    assert!(restore_from_backup(&path, &dir.path().join("missing.sqlite3")).is_err());

    assert_eq!(fs::read(&path).unwrap(), before, "catalog file unchanged");
    let listing = names(path.parent().unwrap());
    assert_eq!(
        listing,
        ["catalog.sqlite3"],
        "no .corrupt/.restoring leftovers: {listing:?}"
    );
    assert_eq!(
        Catalog::open(&path)
            .unwrap()
            .get_setting("mine")
            .unwrap()
            .as_deref(),
        Some("1")
    );
}

#[test]
fn restore_rejects_backup_from_a_newer_schema() {
    let dir = tempfile::tempdir().unwrap();
    let (path, catalog) = open(dir.path());
    let backup = dir.path().join("backups").join(backup_file_name(day(2)));
    fs::create_dir_all(backup.parent().unwrap()).unwrap();
    catalog.backup_to(&backup).unwrap();
    drop(catalog);
    rusqlite::Connection::open(&backup)
        .unwrap()
        .pragma_update(None, "user_version", lb_catalog::LATEST_SCHEMA_VERSION + 1)
        .unwrap();
    let err = restore_from_backup(&path, &backup).unwrap_err();
    assert!(matches!(err, CatalogError::SchemaTooNew), "{err:?}");
}
