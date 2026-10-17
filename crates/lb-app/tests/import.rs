//! M9.3: importing from a folder or card.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use lb_app::{
    FileCopier, FolderNaming, IMPORT_TEMP_SUFFIX, ImportCandidate, ImportOptions, Lightbox,
    LightboxOptions, StdCopier,
};
use lb_core::RootMode;
use lb_jobs::JobQueue;
use std::cell::Cell;
use std::io;
use std::path::{Path, PathBuf};
use walkdir::WalkDir;

struct Rig {
    _home: tempfile::TempDir,
    source: tempfile::TempDir,
    dest: tempfile::TempDir,
    lb: Lightbox,
}

fn rig(files: &[(&str, u8)]) -> Rig {
    let home = tempfile::tempdir().unwrap();
    let source = tempfile::tempdir().unwrap();
    let dest = tempfile::tempdir().unwrap();
    for (name, seed) in files {
        common::write_jpeg(&source.path().join(name), *seed);
    }
    let lb =
        Lightbox::open_with(home.path(), JobQueue::inline(), LightboxOptions::default()).unwrap();
    Rig {
        _home: home,
        source,
        dest,
        lb,
    }
}

fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = WalkDir::new(dir)
        .into_iter()
        .filter_map(Result::ok)
        .filter(|e| e.file_type().is_file())
        .map(|e| e.path().strip_prefix(dir).unwrap().to_owned())
        .collect();
    v.sort();
    v
}

fn candidates(r: &Rig) -> Vec<ImportCandidate> {
    r.lb.import().discover(r.source.path()).unwrap()
}

struct CorruptingCopier;

impl FileCopier for CorruptingCopier {
    fn copy(&self, from: &Path, to: &Path, c: &dyn Fn() -> bool) -> io::Result<bool> {
        let done = StdCopier.copy(from, to, c)?;
        let mut bytes = std::fs::read(to)?;
        let last = bytes.len() - 1;
        bytes[last] ^= 0xff;
        std::fs::write(to, bytes)?;
        Ok(done)
    }
}

struct HalfThenCancel;

impl FileCopier for HalfThenCancel {
    fn copy(&self, from: &Path, to: &Path, _: &dyn Fn() -> bool) -> io::Result<bool> {
        let bytes = std::fs::read(from)?;
        std::fs::write(to, &bytes[..bytes.len() / 2])?;
        Ok(false)
    }
}

#[test]
fn import_copies_into_date_folders() {
    let r = rig(&[
        ("IMG_20210615_100000.jpg", 1),
        ("IMG_20210615_110000.jpg", 2),
        ("IMG_20220102_120000.jpg", 3),
    ]);
    let found = candidates(&r);
    assert_eq!(found.len(), 3);
    let report =
        r.lb.import()
            .import_now(&found, &ImportOptions::new(r.dest.path()))
            .unwrap();
    assert_eq!(report.imported.len(), 3, "{report:?}");
    assert_eq!(
        files_under(r.dest.path()),
        vec![
            PathBuf::from("2021/2021-06-15/IMG_20210615_100000.jpg"),
            PathBuf::from("2021/2021-06-15/IMG_20210615_110000.jpg"),
            PathBuf::from("2022/2022-01-02/IMG_20220102_120000.jpg"),
        ]
    );
    assert!(report.imported.iter().all(|f| f.id.is_some()), "indexed");
    assert_eq!(r.lb.photos().all().unwrap().len(), 3);
    assert_eq!(files_under(r.source.path()).len(), 3, "sources stay");

    let titled = ImportOptions {
        naming: FolderNaming::DateTitle("Beach: day/1".into()),
        skip_duplicates: false,
        ..ImportOptions::new(r.dest.path())
    };
    r.lb.import().import_now(&found[2..], &titled).unwrap();
    assert!(
        r.dest
            .path()
            .join("2022-01-02 Beach day1/IMG_20220102_120000.jpg")
            .is_file()
    );
}

#[test]
fn import_skips_duplicates() {
    let r = rig(&[
        ("IMG_20210615_100000.jpg", 1),
        ("IMG_20210615_100001.jpg", 2),
    ]);
    std::fs::copy(
        r.source.path().join("IMG_20210615_100000.jpg"),
        r.source.path().join("IMG_20210615_100002.jpg"),
    )
    .unwrap();
    let found = candidates(&r);
    assert_eq!(found.len(), 3);

    let library = tempfile::tempdir().unwrap();
    std::fs::copy(
        r.source.path().join("IMG_20210615_100001.jpg"),
        library.path().join("existing.jpg"),
    )
    .unwrap();
    r.lb.library()
        .add_root(library.path(), RootMode::Watch)
        .unwrap();

    let report =
        r.lb.import()
            .import_now(&found, &ImportOptions::new(r.dest.path()))
            .unwrap();
    assert_eq!(report.imported.len(), 1, "{report:?}");
    assert_eq!(report.duplicates.len(), 2);
    assert!(
        report
            .duplicates
            .iter()
            .any(|p| p.ends_with("IMG_20210615_100002.jpg")),
        "a copy inside the same import"
    );
    assert!(
        report
            .duplicates
            .iter()
            .any(|p| p.ends_with("IMG_20210615_100001.jpg")),
        "already in the catalog"
    );

    let again = ImportOptions {
        skip_duplicates: false,
        ..ImportOptions::new(r.dest.path())
    };
    let report = r.lb.import().import_now(&found, &again).unwrap();
    assert_eq!(report.imported.len(), 3);
    assert!(report.duplicates.is_empty());
}

#[test]
fn import_verifies_hash() {
    let r = rig(&[
        ("IMG_20210615_100000.jpg", 1),
        ("IMG_20210616_100000.jpg", 2),
    ]);
    let found = candidates(&r);
    let report =
        r.lb.import()
            .run(
                &found,
                &ImportOptions::new(r.dest.path()),
                &CorruptingCopier,
                &|| false,
                &|_, _| {},
            )
            .unwrap();
    assert!(report.imported.is_empty());
    assert_eq!(report.failed.len(), 2, "{report:?}");
    assert!(
        files_under(r.dest.path()).is_empty(),
        "{:?}",
        files_under(r.dest.path())
    );
}

#[test]
fn import_delete_source_only_after_verify() {
    let r = rig(&[("IMG_20210615_100000.jpg", 1)]);
    let found = candidates(&r);
    let opts = ImportOptions {
        delete_source: true,
        ..ImportOptions::new(r.dest.path())
    };
    let bad =
        r.lb.import()
            .run(&found, &opts, &CorruptingCopier, &|| false, &|_, _| {})
            .unwrap();
    assert!(bad.deleted_sources.is_empty());
    assert!(found[0].path.exists(), "unverified copy keeps the original");

    let good = r.lb.import().import_now(&found, &opts).unwrap();
    assert_eq!(good.deleted_sources, vec![found[0].path.clone()]);
    assert!(!found[0].path.exists());
    assert_eq!(files_under(r.dest.path()).len(), 1);

    let r = rig(&[("IMG_20210615_100000.jpg", 1)]);
    let found = candidates(&r);
    r.lb.import()
        .import_now(&found, &ImportOptions::new(r.dest.path()))
        .unwrap();
    assert!(found[0].path.exists(), "off by default");
}

#[test]
fn import_cancel_leaves_no_partial_files() {
    let r = rig(&[
        ("IMG_20210615_100000.jpg", 1),
        ("IMG_20210615_100001.jpg", 2),
        ("IMG_20210615_100002.jpg", 3),
    ]);
    let found = candidates(&r);
    let opts = ImportOptions::new(r.dest.path());

    let calls = Cell::new(0);
    let report =
        r.lb.import()
            .run(
                &found,
                &opts,
                &StdCopier,
                &|| {
                    calls.set(calls.get() + 1);
                    calls.get() > 1
                },
                &|_, _| {},
            )
            .unwrap();
    assert!(report.cancelled);
    assert!(report.imported.len() < 3, "{report:?}");

    let report =
        r.lb.import()
            .run(&found, &opts, &HalfThenCancel, &|| false, &|_, _| {})
            .unwrap();
    assert!(report.cancelled);
    for f in files_under(r.dest.path()) {
        let name = f.to_string_lossy();
        assert!(!name.ends_with(IMPORT_TEMP_SUFFIX), "{name}");
        let full = r.dest.path().join(&f);
        assert!(
            image::open(&full).is_ok(),
            "{name} is a complete image, not a partial copy"
        );
    }
}
