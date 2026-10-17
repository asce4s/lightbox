//! Folder Manager acceptance tests (SPEC §6.1).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use lb_core::RootMode;
use lb_scan::{FolderManager, RootChange, normalize_path, strip_verbatim_prefix};
use lb_testkit::fixtures;
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

fn now() -> chrono::NaiveDateTime {
    chrono::NaiveDateTime::default()
}

fn add(m: &FolderManager, path: &Path, mode: RootMode) -> RootChange {
    m.add_root(path, mode, now(), &AtomicBool::new(false), &mut |_| {})
        .unwrap()
}

fn hashes(dir: &Path) -> BTreeMap<PathBuf, String> {
    walk(dir)
        .into_iter()
        .map(|p| {
            let h = lb_media::content_hash(&p).unwrap();
            (p, h)
        })
        .collect()
}

fn walk(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).unwrap() {
        let p = e.unwrap().path();
        if p.is_dir() {
            out.extend(walk(&p));
        } else {
            out.push(p);
        }
    }
    out
}

#[test]
fn folder_manager_once_does_not_watch() {
    let env = env();
    let once = env.subdir("once");
    let watch = env.subdir("watch");
    let beach = generated_basic().join("2021/beach");
    for d in [&once, &watch] {
        std::fs::copy(beach.join("beach_01.jpg"), d.join("a.jpg")).unwrap();
    }
    let manager = FolderManager::new(env.scanner.clone());
    let c1 = add(&manager, &once, RootMode::Once);
    assert_eq!(c1.scan.unwrap().added, 1, "once scans immediately");
    assert!(
        manager.watched_roots().unwrap().is_empty(),
        "once roots are never watched"
    );
    add(&manager, &watch, RootMode::Watch);
    let watched: Vec<_> = manager
        .watched_roots()
        .unwrap()
        .into_iter()
        .map(|r| r.path)
        .collect();
    assert_eq!(watched, std::slice::from_ref(&watch));

    std::fs::copy(beach.join("beach_02.jpg"), once.join("later.jpg")).unwrap();
    assert_eq!(
        env.photos_under(&once).len(),
        1,
        "nothing rescans a once root by itself"
    );

    manager
        .set_mode(
            c1.root.unwrap().id,
            RootMode::Watch,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    assert_eq!(manager.watched_roots().unwrap().len(), 2);
    assert_eq!(
        env.photos_under(&once).len(),
        2,
        "changing the mode applies it with a scan"
    );
}

#[test]
fn folder_manager_exclude_removes_catalog_rows_not_files() {
    let env = env();
    let lib = env.subdir("lib");
    fixtures::copy_tree(&generated_basic(), &lib).unwrap();
    let before = hashes(&lib);
    let manager = FolderManager::new(env.scanner.clone());
    let change = add(&manager, &lib, RootMode::Watch);
    let id = change.root.unwrap().id;
    let indexed = env.photos_under(&lib).len();
    assert!(indexed > 20);

    let excluded = manager
        .set_mode(id, RootMode::Exclude, &AtomicBool::new(false), &mut |_| {})
        .unwrap();
    assert_eq!(excluded.removed.len(), indexed);
    assert!(env.photos_under(&lib).is_empty(), "catalog rows are gone");
    assert!(manager.watched_roots().unwrap().is_empty());
    assert_eq!(hashes(&lib), before, "files are untouched");

    let rescan = manager.scanner().scan_root(
        &manager.roots().unwrap()[0],
        &AtomicBool::new(false),
        &mut |_| {},
    );
    assert_eq!(rescan.unwrap().added, 0, "excluded roots stay empty");
}

#[test]
fn folder_manager_nested_override() {
    let env = env();
    let beach = generated_basic().join("2021/beach");
    let make_lib = |name: &str| {
        let lib = env.subdir(name);
        for (rel, src) in [
            ("top.jpg", "beach_01.jpg"),
            ("b/inner.jpg", "beach_02.jpg"),
            ("b/deeper/x.jpg", "beach_03.jpg"),
        ] {
            let dest = lib.join(rel);
            std::fs::create_dir_all(dest.parent().unwrap()).unwrap();
            std::fs::copy(beach.join(src), dest).unwrap();
        }
        lib
    };
    let names = |dir: &Path| -> Vec<String> {
        let mut v: Vec<_> = env
            .photos_under(dir)
            .into_iter()
            .map(|p| p.path.strip_prefix(dir).unwrap().display().to_string())
            .collect();
        v.sort();
        v
    };

    let lib = make_lib("first");
    let manager = FolderManager::new(env.scanner.clone());
    add(&manager, &lib, RootMode::Watch);
    assert_eq!(names(&lib).len(), 3);
    let removed = add(&manager, &lib.join("b"), RootMode::Exclude);
    assert_eq!(removed.removed.len(), 2);
    assert_eq!(
        names(&lib),
        ["top.jpg"],
        "existing rows below the excluded root are removed"
    );
    let again = manager
        .scanner()
        .scan_root(
            &manager.effective_root(&lib).unwrap().unwrap(),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    assert_eq!(
        again.added, 0,
        "rescanning the ancestor skips the excluded subtree"
    );

    let lib2 = make_lib("second");
    add(&manager, &lib2.join("b"), RootMode::Exclude);
    add(&manager, &lib2, RootMode::Watch);
    assert_eq!(
        names(&lib2),
        ["top.jpg"],
        "exclusion added first also holds"
    );

    let lib3 = make_lib("third");
    add(&manager, &lib3, RootMode::Exclude);
    add(&manager, &lib3.join("b"), RootMode::Watch);
    assert_eq!(
        names(&lib3),
        ["b/deeper/x.jpg", "b/inner.jpg"],
        "a nested watch root overrides an excluded ancestor"
    );
    let roots = manager.roots().unwrap();
    assert_eq!(
        manager
            .effective_root(&lib3.join("b/deeper"))
            .unwrap()
            .unwrap()
            .path,
        lib3.join("b")
    );
    assert_eq!(
        manager
            .effective_root(&lib3.join("top.jpg"))
            .unwrap()
            .unwrap()
            .path,
        lib3
    );
    assert!(
        manager
            .effective_root(Path::new("/definitely/elsewhere"))
            .unwrap()
            .is_none()
    );
    assert_eq!(roots.len(), 6);

    let rm = manager
        .remove_root(
            manager.effective_root(&lib3.join("b")).unwrap().unwrap().id,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    assert_eq!(rm.removed.len(), 2);
    assert!(
        names(&lib3).is_empty(),
        "the excluded ancestor takes over again"
    );
}

#[test]
fn removing_a_nested_watch_root_hands_files_back_to_the_ancestor() {
    let env = env();
    let lib = env.subdir("lib");
    let beach = generated_basic().join("2021/beach");
    std::fs::create_dir_all(lib.join("b")).unwrap();
    std::fs::copy(beach.join("beach_01.jpg"), lib.join("b/inner.jpg")).unwrap();
    let manager = FolderManager::new(env.scanner.clone());
    add(&manager, &lib, RootMode::Once);
    let nested = add(&manager, &lib.join("b"), RootMode::Watch);
    let before = env.photos_under(&lib);
    assert_eq!(before.len(), 1);
    let change = manager
        .remove_root(
            nested.root.unwrap().id,
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    assert_eq!(change.scan.unwrap().added, 1);
    assert_eq!(env.photos_under(&lib).len(), 1);
}

#[test]
fn path_normalization_table() {
    let env = env();
    let base = env.subdir("base");
    std::fs::create_dir_all(base.join("a/b")).unwrap();
    let canonical = base.join("a/b");
    let cases: Vec<(PathBuf, &str)> = vec![
        (canonical.clone(), "already canonical"),
        (
            PathBuf::from(format!("{}/", canonical.display())),
            "trailing separator",
        ),
        (base.join("a/./b"), "dot component"),
        (base.join("a/b/../b"), "dotdot component"),
        (base.join("a/../a/b/"), "dotdot and trailing separator"),
    ];
    for (input, label) in cases {
        let got = normalize_path(&input).unwrap();
        assert_eq!(got, canonical, "{label}");
        assert!(
            !got.to_str().unwrap().ends_with('/') || got == Path::new("/"),
            "{label}"
        );
    }
    assert_eq!(normalize_path(Path::new("/")).unwrap(), Path::new("/"));
    assert!(normalize_path(&base.join("missing")).is_err(), "must exist");

    #[cfg(unix)]
    {
        let link = base.join("link");
        std::os::unix::fs::symlink(&canonical, &link).unwrap();
        assert_eq!(
            normalize_path(&link).unwrap(),
            canonical,
            "symlinks are resolved"
        );
    }

    let verbatim = [
        (r"\\?\C:\Photos\2021", r"C:\Photos\2021"),
        (r"\\?\UNC\server\share\p", r"\\server\share\p"),
        (r"C:\Photos", r"C:\Photos"),
        (r"\\?\Volume{1234}\x", r"\\?\Volume{1234}\x"),
        ("/home/me/photos", "/home/me/photos"),
        ("", ""),
    ];
    for (input, expected) in verbatim {
        assert_eq!(strip_verbatim_prefix(input), expected, "{input}");
    }
}

#[cfg(unix)]
#[test]
fn symlink_loops_are_skipped_without_hanging() {
    let env = env();
    let lib = env.subdir("lib");
    std::fs::copy(
        generated_basic().join("2021/beach/beach_01.jpg"),
        lib.join("a.jpg"),
    )
    .unwrap();
    std::fs::create_dir_all(lib.join("sub")).unwrap();
    std::os::unix::fs::symlink(&lib, lib.join("sub/loop")).unwrap();
    let manager = FolderManager::new(env.scanner.clone());
    let change = add(&manager, &lib, RootMode::Once);
    let scan = change.scan.unwrap();
    assert_eq!(scan.added, 1, "{scan:?}");
    assert!(
        scan.errors >= 1,
        "the loop is reported, not followed: {scan:?}"
    );
}

#[test]
fn removing_a_missing_root_is_an_error() {
    let env = env();
    let manager = FolderManager::new(env.scanner.clone());
    let err = manager
        .remove_root(
            lb_core::RootId::new(42),
            &AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap_err();
    assert!(matches!(err, lb_scan::ScanError::RootNotFound));
}
