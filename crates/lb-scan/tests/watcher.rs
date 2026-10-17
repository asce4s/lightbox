//! Watcher acceptance tests (SPEC §6.3). Real temp directories, polling with a 10 s timeout.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use lb_core::PhotoStatus;
use lb_scan::WatchConfig;
use lb_testkit::wait_until;
use std::time::Duration;

#[test]
fn watcher_detects_new_file() {
    let env = env();
    let dir = env.subdir("lib");
    let beach = generated_basic().join("2021/beach");
    put(&beach.join("beach_01.jpg"), &dir.join("a.jpg"), 0);
    let w = watch(&env, fast_watch());
    add_watch_root(&w, &dir);
    assert_eq!(env.photos_under(&dir).len(), 1);

    put(&beach.join("beach_02.jpg"), &dir.join("sub/new.jpg"), 1);
    assert!(
        wait_until(WAIT, || env.photos_under(&dir).len() == 2),
        "new file in a new sub-directory was not indexed"
    );
    put(&beach.join("beach_03.jpg"), &dir.join("sub/another.jpg"), 2);
    assert!(wait_until(WAIT, || env.photos_under(&dir).len() == 3));
    let events = w.events.lock().unwrap();
    assert!(!events.is_empty(), "watcher reports what it changed");
}

#[test]
fn watcher_detects_modified_file_updates_metadata() {
    let env = env();
    let dir = env.subdir("lib");
    let beach = generated_basic().join("2021/beach");
    put(&beach.join("beach_01.jpg"), &dir.join("a.jpg"), 0);
    let w = watch(&env, fast_watch());
    add_watch_root(&w, &dir);
    let before = env.photos_under(&dir).remove(0);
    assert!(before.gps.is_none());

    put(&beach.join("beach_02.jpg"), &dir.join("a.jpg"), 5);
    assert!(
        wait_until(WAIT, || {
            let p = env.catalog.get_photo(before.id).unwrap().unwrap();
            p.gps.is_some() && p.quick_key != before.quick_key
        }),
        "modified file was not re-read"
    );
    let after = env.catalog.get_photo(before.id).unwrap().unwrap();
    assert_eq!(after.id, before.id, "same row, refreshed metadata");
    assert_eq!(after.taken_at.unwrap().format("%d").to_string(), "02");
}

#[test]
fn watcher_detects_delete_marks_missing() {
    let env = env();
    let dir = env.subdir("lib");
    let beach = generated_basic().join("2021/beach");
    put(&beach.join("beach_01.jpg"), &dir.join("a.jpg"), 0);
    put(&beach.join("beach_02.jpg"), &dir.join("sub/b.jpg"), 0);
    let w = watch(&env, fast_watch());
    add_watch_root(&w, &dir);
    let photos = env.photos_under(&dir);
    assert_eq!(photos.len(), 2);

    std::fs::remove_file(dir.join("a.jpg")).unwrap();
    let a = photos
        .iter()
        .find(|p| p.path.ends_with("a.jpg"))
        .unwrap()
        .id;
    assert!(wait_until(WAIT, || env
        .catalog
        .get_photo(a)
        .unwrap()
        .unwrap()
        .status
        == PhotoStatus::Missing));

    std::fs::remove_dir_all(dir.join("sub")).unwrap();
    let b = photos
        .iter()
        .find(|p| p.path.ends_with("b.jpg"))
        .unwrap()
        .id;
    assert!(
        wait_until(WAIT, || env.catalog.get_photo(b).unwrap().unwrap().status
            == PhotoStatus::Missing),
        "deleting a directory marks everything below it missing"
    );
}

#[test]
fn watcher_ignores_hidden_and_unsupported_files() {
    let env = env();
    let dir = env.subdir("lib");
    let beach = generated_basic().join("2021/beach");
    let w = watch(&env, fast_watch());
    add_watch_root(&w, &dir);
    put(&beach.join("beach_01.jpg"), &dir.join(".hidden/h.jpg"), 0);
    put(&beach.join("beach_01.jpg"), &dir.join("notes.txt"), 0);
    put(&beach.join("beach_01.jpg"), &dir.join("visible.jpg"), 0);
    assert!(wait_until(WAIT, || env.photos_under(&dir).len() == 1));
    std::thread::sleep(Duration::from_millis(150));
    let names: Vec<_> = env
        .photos_under(&dir)
        .into_iter()
        .map(|p| p.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, ["visible.jpg"]);
}

#[test]
fn in_progress_copy_is_indexed_only_once_stable() {
    let env = env();
    let dir = env.subdir("lib");
    let bytes = std::fs::read(generated_basic().join("misc/large_6000x4000.jpg")).unwrap();
    let w = watch(
        &env,
        WatchConfig {
            debounce: Duration::from_millis(30),
            settle: Duration::from_millis(120),
        },
    );
    add_watch_root(&w, &dir);
    let target = dir.join("copying.jpg");
    let writing = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(true));
    let flag = writing.clone();
    let chunks: Vec<Vec<u8>> = bytes
        .chunks(bytes.len() / 20 + 1)
        .map(<[u8]>::to_vec)
        .collect();
    let writer = std::thread::spawn(move || {
        use std::io::Write;
        let mut file = std::fs::File::create(&target).unwrap();
        for chunk in chunks {
            file.write_all(&chunk).unwrap();
            file.flush().unwrap();
            std::thread::sleep(Duration::from_millis(15));
        }
        drop(file);
        flag.store(false, std::sync::atomic::Ordering::SeqCst);
    });
    let mut rows_while_writing = 0;
    while writing.load(std::sync::atomic::Ordering::SeqCst) {
        rows_while_writing += env.photos_under(&dir).len();
        std::thread::sleep(Duration::from_millis(5));
    }
    writer.join().unwrap();
    assert_eq!(
        rows_while_writing, 0,
        "nothing is indexed while the file is still growing"
    );
    assert!(
        wait_until(WAIT, || {
            env.photos_under(&dir)
                .first()
                .is_some_and(|p| p.status == PhotoStatus::Ok && p.width == Some(6000))
        }),
        "{:?}",
        env.photos_under(&dir)
    );
}

#[test]
fn folder_manager_watch_scans_and_watches() {
    let env = env();
    let dir = env.subdir("lib");
    lb_testkit::fixtures::copy_tree(&generated_basic().join("2021"), &dir).unwrap();
    let w = watch(&env, WatchConfig::default());
    let root = add_watch_root(&w, &dir);
    let initial = env.photos_under(&dir).len();
    assert_eq!(
        initial, 6,
        "all fixture images are indexed by adding the root"
    );
    assert_eq!(w.service.watched_paths(), std::slice::from_ref(&root.path));

    let start = std::time::Instant::now();
    put(
        &generated_basic().join("2022/city/city_01_o1.jpg"),
        &dir.join("added_later.jpg"),
        3,
    );
    assert!(
        wait_until(Duration::from_secs(2), || env.photos_under(&dir).len()
            == initial + 1),
        "a file added afterwards must appear within 2 s (default timings)"
    );
    assert!(start.elapsed() < Duration::from_secs(2));

    w.manager
        .set_mode(
            root.id,
            lb_core::RootMode::Once,
            &std::sync::atomic::AtomicBool::new(false),
            &mut |_| {},
        )
        .unwrap();
    w.service.sync_roots().unwrap();
    assert!(
        w.service.watched_paths().is_empty(),
        "switching to once stops watching"
    );
}
