//! SPEC §10 acceptance tests: user-facing error messages and atomic writes / crash logs.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use lb_app::{
    AppError, dismiss_crash_logs, pending_crash_logs, write_atomic, write_bytes_atomic,
    write_crash_log,
};
use lb_catalog::CatalogError;
use lb_core::EditError;
use lb_media::MediaError;
use lb_scan::ScanError;
use lb_thumbs::ThumbError;
use std::io::{self, Write};
use std::path::PathBuf;

fn io_err(kind: io::ErrorKind) -> io::Error {
    io::Error::new(kind, "secret internal detail /var/lib/x.sqlite3")
}

/// Exhaustive on purpose: adding an `AppError` variant breaks compilation here until a sample
/// (and therefore a user message) exists for it.
fn variant_name(e: &AppError) -> &'static str {
    match e {
        AppError::Catalog(_) => "catalog",
        AppError::Scan(_) => "scan",
        AppError::Thumb(_) => "thumb",
        AppError::Edit(_) => "edit",
        AppError::Media(_) => "media",
        AppError::Cancelled => "cancelled",
        AppError::Io(_) => "io",
        AppError::Invalid(_) => "invalid",
        AppError::AlreadyExists(_) => "exists",
        AppError::NotFound(_) => "not_found",
    }
}

fn samples() -> Vec<AppError> {
    vec![
        AppError::Catalog(sqlite_error()),
        AppError::Catalog(CatalogError::Io(io_err(io::ErrorKind::Other))),
        AppError::Catalog(CatalogError::WriterStopped),
        AppError::Catalog(CatalogError::WriterPanicked("boom".into())),
        AppError::Catalog(CatalogError::ReentrantWrite),
        AppError::Catalog(CatalogError::NonUtf8Path(PathBuf::from("/x"))),
        AppError::Catalog(CatalogError::NotFound("photo")),
        AppError::Catalog(CatalogError::BackupInvalid("bad".into())),
        AppError::Catalog(CatalogError::SchemaTooNew),
        AppError::Scan(ScanError::Io {
            path: PathBuf::from("/photos"),
            source: io_err(io::ErrorKind::PermissionDenied),
        }),
        AppError::Scan(ScanError::NonUtf8Path(PathBuf::from("/x"))),
        AppError::Scan(ScanError::Watch("inotify".into())),
        AppError::Scan(ScanError::RootNotFound),
        AppError::Thumb(ThumbError::Encode("jpeg".into())),
        AppError::Thumb(ThumbError::Io(io_err(io::ErrorKind::Other))),
        AppError::Thumb(ThumbError::Media(MediaError::Corrupt("x".into()))),
        AppError::Edit(EditError::BadTurns(3)),
        AppError::Media(MediaError::Io(io_err(io::ErrorKind::Other))),
        AppError::Media(MediaError::Corrupt("x".into())),
        AppError::Media(MediaError::Unsupported("heic".into())),
        AppError::Cancelled,
        AppError::Io(io_err(io::ErrorKind::PermissionDenied)),
        AppError::Io(io_err(io::ErrorKind::NotFound)),
        AppError::Io(io_err(io::ErrorKind::StorageFull)),
        AppError::Io(io_err(io::ErrorKind::Other)),
        AppError::Invalid("name must not be empty".into()),
        AppError::AlreadyExists(PathBuf::from("/photos/a.jpg")),
        AppError::NotFound("album"),
    ]
}

fn sqlite_error() -> CatalogError {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("garbage.sqlite3");
    std::fs::write(&path, vec![0x42u8; 4096]).unwrap();
    lb_catalog::Catalog::open(&path).expect_err("opening a non-database file fails")
}

#[test]
fn user_messages_for_all_error_variants() {
    let all = samples();
    let covered: std::collections::BTreeSet<_> = all.iter().map(variant_name).collect();
    assert_eq!(
        covered.len(),
        10,
        "every AppError variant has a sample: {covered:?}"
    );
    for e in &all {
        let msg = e.user_message();
        assert!(!msg.trim().is_empty(), "{e:?}: empty message");
        assert!(msg.len() <= 200, "{e:?}: message too long: {msg}");
        for forbidden in [
            "sqlite", "SQL", "rusqlite", "panicked", "Error(", "inotify", "secret", "/var/",
        ] {
            assert!(
                !msg.contains(forbidden),
                "{e:?}: leaks internals ({forbidden}): {msg}"
            );
        }
        assert!(
            msg.ends_with(['.', '!']),
            "{e:?}: message should be a sentence: {msg}"
        );
    }
}

fn dir_entries(dir: &std::path::Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn atomic_write_leaves_no_partial_on_error() {
    let dir = tempfile::tempdir().unwrap();
    let dest = dir.path().join("out.txt");

    write_bytes_atomic(&dest, b"original").unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"original");
    assert_eq!(dir_entries(dir.path()), ["out.txt"]);

    let err = write_atomic(&dest, |f| {
        f.write_all(b"partial gar")?;
        Err(io::Error::other("disk exploded"))
    })
    .unwrap_err();
    assert_eq!(err.to_string(), "disk exploded");
    assert_eq!(
        std::fs::read(&dest).unwrap(),
        b"original",
        "existing file untouched"
    );
    assert_eq!(
        dir_entries(dir.path()),
        ["out.txt"],
        "no temp file left behind"
    );

    let fresh = dir.path().join("fresh.txt");
    assert!(write_atomic(&fresh, |_| Err(io::Error::other("nope"))).is_err());
    assert!(!fresh.exists(), "no partial destination is created");
    assert_eq!(dir_entries(dir.path()), ["out.txt"]);

    write_bytes_atomic(&dest, b"replaced").unwrap();
    assert_eq!(std::fs::read(&dest).unwrap(), b"replaced");
}

#[test]
fn crash_log_is_written_and_reported_until_dismissed() {
    let dir = tempfile::tempdir().unwrap();
    assert!(pending_crash_logs(dir.path()).is_empty());
    let a = write_crash_log(dir.path(), "boom", Some("src/x.rs:1:1"), "frame 0").unwrap();
    let b = write_crash_log(dir.path(), "again", None, "frame 1").unwrap();
    assert_ne!(a, b);
    let text = std::fs::read_to_string(&a).unwrap();
    assert!(
        text.contains("message: boom") && text.contains("src/x.rs:1:1") && text.contains("frame 0")
    );
    assert_eq!(pending_crash_logs(dir.path()).len(), 2);
    std::fs::write(dir.path().join("catalog.sqlite3"), b"x").unwrap();
    dismiss_crash_logs(dir.path()).unwrap();
    assert!(pending_crash_logs(dir.path()).is_empty());
    assert!(dir.path().join("catalog.sqlite3").exists());
}
