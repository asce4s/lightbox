//! Opening databases written by older schema versions (SPEC §5.2: migrations are append-only).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use lb_catalog::{Catalog, CatalogError, LATEST_SCHEMA_VERSION};
use rusqlite::Connection;
use std::path::Path;

const V1_SCHEMA: &str = include_str!("data/v1_schema.sql");
const V1_DATA: &str = include_str!("data/v1_data.sql");

fn write_v1_database(path: &Path) {
    let conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "journal_mode", "WAL").unwrap();
    conn.execute_batch(V1_SCHEMA).unwrap();
    conn.execute_batch(V1_DATA).unwrap();
    conn.pragma_update(None, "user_version", 1).unwrap();
}

#[test]
fn migrate_from_v1_fixture() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("catalog.sqlite3");
    write_v1_database(&path);

    let catalog = Catalog::open(&path).unwrap();
    let version: u32 = catalog
        .read(|c| Ok(c.pragma_query_value(None, "user_version", |r| r.get(0))?))
        .unwrap();
    assert_eq!(version, LATEST_SCHEMA_VERSION);

    let (photos, edits, items, tags, caption): (i64, i64, i64, i64, String) = catalog
        .read(|c| {
            Ok(c.query_row(
                "SELECT (SELECT COUNT(*) FROM photos), (SELECT COUNT(*) FROM edits), \
                        (SELECT COUNT(*) FROM album_items), (SELECT COUNT(*) FROM photo_tags), \
                        (SELECT caption FROM photos WHERE id = 1)",
                [],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?)),
            )?)
        })
        .unwrap();
    assert_eq!((photos, edits, items, tags), (2, 1, 2, 1));
    assert_eq!(caption, "At the beach", "user data survives migration");

    let integrity: String = catalog
        .read(|c| Ok(c.query_row("PRAGMA integrity_check", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(integrity, "ok");
    let violations: i64 = catalog
        .read(|c| {
            Ok(c.prepare("PRAGMA foreign_key_check")?
                .query_map([], |_| Ok(()))?
                .count() as i64)
        })
        .unwrap();
    assert_eq!(violations, 0);
}

#[test]
fn catalog_rejects_database_from_the_future() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("catalog.sqlite3");
    write_v1_database(&path);
    Connection::open(&path)
        .unwrap()
        .pragma_update(None, "user_version", LATEST_SCHEMA_VERSION + 5)
        .unwrap();
    let err = Catalog::open(&path).unwrap_err();
    assert!(matches!(err, CatalogError::SchemaTooNew), "{err:?}");
}
