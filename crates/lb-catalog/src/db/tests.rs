use super::*;
use crate::LATEST_SCHEMA_VERSION;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

fn open() -> (tempfile::TempDir, Catalog) {
    let dir = tempfile::tempdir().unwrap();
    let catalog = Catalog::open(&dir.path().join("data/catalog.sqlite3")).unwrap();
    (dir, catalog)
}

fn names(catalog: &Catalog, kind: &str) -> Vec<String> {
    catalog
        .read(|c| {
            let mut stmt =
                c.prepare("SELECT name FROM sqlite_master WHERE type = ?1 ORDER BY name")?;
            let rows = stmt.query_map([kind], |r| r.get::<_, String>(0))?;
            Ok(rows.collect::<std::result::Result<Vec<_>, _>>()?)
        })
        .unwrap()
}

#[test]
fn migrations_apply_on_empty_db() {
    let (dir, catalog) = open();
    let version: u32 = catalog
        .read(|c| Ok(c.pragma_query_value(None, "user_version", |r| r.get(0))?))
        .unwrap();
    assert_eq!(version, LATEST_SCHEMA_VERSION);

    let tables = names(&catalog, "table");
    for t in [
        "roots",
        "folders",
        "photos",
        "edits",
        "albums",
        "album_items",
        "tags",
        "photo_tags",
        "people",
        "faces",
        "settings",
        "jobs_log",
        "photos_fts",
    ] {
        assert!(
            tables.iter().any(|x| x == t),
            "missing table {t}: {tables:?}"
        );
    }
    let indexes = names(&catalog, "index");
    for i in [
        "photos_taken_at",
        "photos_quick_key",
        "photos_content_hash",
        "photos_starred",
    ] {
        assert!(
            indexes.iter().any(|x| x == i),
            "missing index {i}: {indexes:?}"
        );
    }

    let mode: String = catalog
        .read(|c| Ok(c.pragma_query_value(None, "journal_mode", |r| r.get(0))?))
        .unwrap();
    assert_eq!(mode, "wal");
    let fk: bool = catalog
        .read(|c| Ok(c.pragma_query_value(None, "foreign_keys", |r| r.get(0))?))
        .unwrap();
    assert!(fk, "readers also enforce foreign keys");

    drop(catalog);
    let again = Catalog::open(&dir.path().join("data/catalog.sqlite3")).unwrap();
    let version: u32 = again
        .read(|c| Ok(c.pragma_query_value(None, "user_version", |r| r.get(0))?))
        .unwrap();
    assert_eq!(
        version, LATEST_SCHEMA_VERSION,
        "reopening does not re-apply or fail"
    );
}

#[test]
fn schema_version_constant_matches_migration_files() {
    let dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("migrations");
    let count = std::fs::read_dir(dir)
        .unwrap()
        .filter(|e| {
            e.as_ref()
                .unwrap()
                .path()
                .extension()
                .is_some_and(|x| x == "sql")
        })
        .count();
    assert_eq!(
        count as u32, LATEST_SCHEMA_VERSION,
        "bump LATEST_SCHEMA_VERSION when adding a migration"
    );
}

#[test]
fn fts5_is_available_in_bundled_sqlite() {
    let (_dir, catalog) = open();
    catalog
        .write(|c| {
            c.execute(
                "INSERT INTO photos_fts (rowid, file_name, caption) VALUES (1, 'beach.jpg', 'sunny café')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    let hit: i64 = catalog
        .read(|c| {
            Ok(c.query_row(
                "SELECT rowid FROM photos_fts WHERE photos_fts MATCH 'cafe'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(hit, 1, "remove_diacritics tokenizer folds é to e");
}

fn insert_root_and_folder(c: &Connection) -> rusqlite::Result<()> {
    c.execute(
        "INSERT INTO roots (id, path, mode, added_at) VALUES (1, '/r', 'watch', 'now')",
        [],
    )?;
    c.execute(
        "INSERT INTO folders (id, root_id, path, name) VALUES (1, 1, '/r', 'r')",
        [],
    )?;
    Ok(())
}

const INSERT_PHOTO: &str = "INSERT INTO photos (id, folder_id, file_name, media_type, format, file_size, \
     file_mtime, quick_key, indexed_at) VALUES (?1, ?2, ?3, 'image', 'jpeg', 1, 1, 'k', 'now')";

#[test]
fn foreign_keys_enforced() {
    let (_dir, catalog) = open();
    let err = catalog
        .write(|c| Ok(c.execute(INSERT_PHOTO, (1, 99, "orphan.jpg"))?))
        .unwrap_err();
    assert!(
        matches!(&err, CatalogError::Sqlite(rusqlite::Error::SqliteFailure(e, _))
            if e.code == rusqlite::ErrorCode::ConstraintViolation),
        "{err:?}"
    );

    catalog
        .write(|c| {
            insert_root_and_folder(c)?;
            c.execute(INSERT_PHOTO, (1, 1, "a.jpg"))?;
            c.execute(
                "INSERT INTO edits (photo_id, ops_json, updated_at) VALUES (1, '[]', 'now')",
                [],
            )?;
            Ok(())
        })
        .unwrap();
    catalog
        .write(|c| Ok(c.execute("DELETE FROM roots WHERE id = 1", [])?))
        .unwrap();
    let left: i64 = catalog
        .read(|c| {
            Ok(c.query_row(
                "SELECT (SELECT COUNT(*) FROM folders) + (SELECT COUNT(*) FROM photos) + (SELECT COUNT(*) FROM edits)",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(
        left, 0,
        "deleting a root cascades to folders, photos and edits"
    );
}

#[test]
fn check_constraints_reject_bad_values() {
    let (_dir, catalog) = open();
    catalog.write(|c| Ok(insert_root_and_folder(c)?)).unwrap();
    let bad_rating = catalog
        .write(|c| {
            Ok(c.execute(
                "INSERT INTO photos (folder_id, file_name, media_type, format, file_size, file_mtime, \
                 quick_key, rating, indexed_at) VALUES (1, 'x.jpg', 'image', 'jpeg', 1, 1, 'k', 6, 'now')",
                [],
            )?)
        })
        .unwrap_err();
    assert!(matches!(bad_rating, CatalogError::Sqlite(_)));
    let bad_mode = catalog
        .write(|c| {
            Ok(c.execute(
                "INSERT INTO roots (path, mode, added_at) VALUES ('/x', 'sometimes', 'now')",
                [],
            )?)
        })
        .unwrap_err();
    assert!(matches!(bad_mode, CatalogError::Sqlite(_)));
}

#[test]
fn writer_serializes_concurrent_writes() {
    const THREADS: usize = 8;
    const ITERATIONS: usize = 40;
    let (_dir, catalog) = open();
    catalog
        .write(|c| {
            Ok(c.execute(
                "INSERT INTO settings (key, value_json) VALUES ('counter', '0')",
                [],
            )?)
        })
        .unwrap();

    let inside = Arc::new(AtomicBool::new(false));
    let overlaps = Arc::new(AtomicUsize::new(0));
    let handles: Vec<_> = (0..THREADS)
        .map(|_| {
            let (catalog, inside, overlaps) = (catalog.clone(), inside.clone(), overlaps.clone());
            std::thread::spawn(move || {
                for _ in 0..ITERATIONS {
                    let (inside, overlaps) = (inside.clone(), overlaps.clone());
                    catalog
                        .write_tx(move |tx| {
                            if inside.swap(true, Ordering::SeqCst) {
                                overlaps.fetch_add(1, Ordering::SeqCst);
                            }
                            let n: i64 = tx.query_row(
                                "SELECT CAST(value_json AS INTEGER) FROM settings WHERE key = 'counter'",
                                [],
                                |r| r.get(0),
                            )?;
                            std::thread::yield_now();
                            tx.execute("UPDATE settings SET value_json = ?1 WHERE key = 'counter'", [(n + 1).to_string()])?;
                            inside.store(false, Ordering::SeqCst);
                            Ok(())
                        })
                        .unwrap();
                }
            })
        })
        .collect();

    let mut last_seen = 0;
    while handles.iter().any(|h| !h.is_finished()) {
        let n: i64 = catalog
            .read(|c| {
                Ok(c.query_row(
                    "SELECT CAST(value_json AS INTEGER) FROM settings WHERE key = 'counter'",
                    [],
                    |r| r.get(0),
                )?)
            })
            .unwrap();
        assert!(n >= last_seen, "readers never see time go backwards");
        last_seen = n;
    }
    for h in handles {
        h.join().unwrap();
    }
    assert_eq!(
        overlaps.load(Ordering::SeqCst),
        0,
        "write closures never overlap"
    );
    let total: i64 = catalog
        .read(|c| {
            Ok(c.query_row(
                "SELECT CAST(value_json AS INTEGER) FROM settings WHERE key = 'counter'",
                [],
                |r| r.get(0),
            )?)
        })
        .unwrap();
    assert_eq!(total, (THREADS * ITERATIONS) as i64, "no lost updates");
}

#[test]
fn write_tx_rolls_back_on_error_and_panic() {
    let (_dir, catalog) = open();
    let count = |c: &Catalog| -> i64 {
        c.read(|c| Ok(c.query_row("SELECT COUNT(*) FROM settings", [], |r| r.get(0))?))
            .unwrap()
    };
    let err = catalog
        .write_tx(|tx| {
            tx.execute("INSERT INTO settings VALUES ('a', '1')", [])?;
            tx.execute("INSERT INTO settings VALUES ('a', '2')", [])?;
            Ok(())
        })
        .unwrap_err();
    assert!(matches!(err, CatalogError::Sqlite(_)));
    assert_eq!(count(&catalog), 0, "failed transaction left nothing behind");

    let err = catalog
        .write_tx(|tx| -> Result<()> {
            tx.execute("INSERT INTO settings VALUES ('b', '1')", [])?;
            panic!("boom");
        })
        .unwrap_err();
    assert!(
        matches!(&err, CatalogError::WriterPanicked(m) if m.contains("boom")),
        "{err:?}"
    );
    assert_eq!(count(&catalog), 0, "panicking transaction rolled back");

    catalog
        .write_tx(|tx| Ok(tx.execute("INSERT INTO settings VALUES ('c', '1')", [])?))
        .unwrap();
    assert_eq!(count(&catalog), 1, "writer thread survives a panic");
}

#[test]
fn readers_are_read_only_and_see_committed_writes() {
    let (_dir, catalog) = open();
    let err = catalog
        .read(|c| Ok(c.execute("INSERT INTO settings VALUES ('x', '1')", [])?))
        .unwrap_err();
    assert!(matches!(err, CatalogError::Sqlite(_)), "{err:?}");

    for i in 0..20 {
        catalog
            .write(move |c| {
                Ok(c.execute("INSERT INTO settings VALUES (?1, '1')", [format!("k{i}")])?)
            })
            .unwrap();
        let n: i64 = catalog
            .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM settings", [], |r| r.get(0))?))
            .unwrap();
        assert_eq!(
            n,
            i64::from(i) + 1,
            "read-after-write sees the committed row"
        );
    }
}

#[test]
fn reader_pool_is_bounded_and_reusable() {
    let (_dir, catalog) = open();
    assert!((1..=4).contains(&catalog.reader_pool_size()));
    for _ in 0..50 {
        catalog
            .read(|c| Ok(c.query_row("SELECT 1", [], |r| r.get::<_, i64>(0))?))
            .unwrap();
    }
    let panicked = std::panic::catch_unwind(AssertUnwindSafe(|| {
        let _ = catalog.read(|_| -> Result<()> { panic!("reader panic") });
    }));
    assert!(panicked.is_err());
    for _ in 0..catalog.reader_pool_size() + 2 {
        catalog
            .read(|c| Ok(c.query_row("SELECT 1", [], |r| r.get::<_, i64>(0))?))
            .unwrap();
    }
}

#[test]
fn reentrant_write_is_an_error_not_a_deadlock() {
    let (_dir, catalog) = open();
    let inner = catalog.clone();
    let err = catalog.write(move |_| inner.write(|_| Ok(()))).unwrap_err();
    assert!(matches!(err, CatalogError::ReentrantWrite), "{err:?}");
}

#[test]
fn data_survives_reopen_and_last_handle_drop_stops_writer() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("catalog.sqlite3");
    let catalog = Catalog::open(&path).unwrap();
    catalog
        .write(|c| Ok(c.execute("INSERT INTO settings VALUES ('keep', '\"me\"')", [])?))
        .unwrap();
    let clone = catalog.clone();
    drop(catalog);
    clone
        .write(|c| Ok(c.execute("INSERT INTO settings VALUES ('also', '1')", [])?))
        .unwrap();
    drop(clone);

    let reopened = Catalog::open(&path).unwrap();
    let n: i64 = reopened
        .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM settings", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(n, 2);
}

#[test]
fn dropping_the_catalog_checkpoints_the_wal() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("catalog.sqlite3");
    let catalog = Catalog::open(&path).unwrap();
    catalog
        .write(|c| Ok(c.execute("INSERT INTO settings VALUES ('k', '1')", [])?))
        .unwrap();
    catalog
        .read(|c| Ok(c.query_row("SELECT 1", [], |r| r.get::<_, i64>(0))?))
        .unwrap();
    drop(catalog);

    let mut files: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    files.sort();
    assert_eq!(files, ["catalog.sqlite3"], "no -wal/-shm left behind");
    let raw =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let n: i64 = raw
        .query_row("SELECT COUNT(*) FROM settings", [], |r| r.get(0))
        .unwrap();
    assert_eq!(n, 1, "the main file alone holds the data");
}
