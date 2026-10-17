//! M13.4: navigation sets, path lookup and file proxies.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::write_jpeg;
use lb_app::Lightbox;
use lb_app::viewer::FolderRefresh;
use lb_app::vm::ViewerVm;
use lb_jobs::JobQueue;
use std::fs;
use std::path::{Path, PathBuf};

fn library() -> (tempfile::TempDir, Lightbox) {
    let home = tempfile::tempdir().unwrap();
    let lb = Lightbox::open(home.path(), JobQueue::inline()).unwrap();
    (home, lb)
}

fn names(vm: &ViewerVm) -> Vec<String> {
    vm.entries()
        .iter()
        .map(|e| e.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect()
}

fn touch(dir: &Path, name: &str, seed: u8) -> PathBuf {
    let path = dir.join(name);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).unwrap();
    }
    write_jpeg(&path, seed);
    path
}

#[test]
fn viewer_folder_siblings_natural_sort_skip_hidden_and_unsupported() {
    let (_home, lb) = library();
    let dir = tempfile::tempdir().unwrap();
    touch(dir.path(), "img10.jpg", 1);
    touch(dir.path(), "img2.jpg", 2);
    touch(dir.path(), "img02.jpg", 3);
    touch(dir.path(), ".hidden.jpg", 4);
    fs::write(dir.path().join("notes.txt"), b"nope").unwrap();
    let vm = ViewerVm::open_paths(&lb, &[dir.path().join("img10.jpg")]).unwrap();
    assert_eq!(
        names(&vm),
        ["img2.jpg", "img02.jpg", "img10.jpg"],
        "hidden and non-images stay out; natural order"
    );
    assert_eq!(vm.current_entry().path, dir.path().join("img10.jpg"));
    assert!(vm.current_photo().is_none());
}

#[test]
fn viewer_multi_file_open_limits_navigation() {
    let (_home, lb) = library();
    let dir = tempfile::tempdir().unwrap();
    let img10 = touch(dir.path(), "img10.jpg", 1);
    let img2 = touch(dir.path(), "img2.jpg", 2);
    let _sibling = touch(dir.path(), "img3.jpg", 3);
    let mut vm = ViewerVm::open_paths(&lb, &[img10.clone(), img2.clone()]).unwrap();
    assert_eq!(names(&vm), ["img10.jpg", "img2.jpg"]);
    assert!(vm.go_next());
    assert_eq!(vm.current_entry().path, img2);
    assert!(!vm.go_next(), "does not walk into the rest of the folder");
    assert!(vm.go_prev());
    assert_eq!(vm.current_entry().path, img10);
}

#[test]
fn viewer_directory_argument_opens_first_file() {
    let (_home, lb) = library();
    let dir = tempfile::tempdir().unwrap();
    touch(dir.path(), "img10.jpg", 1);
    touch(dir.path(), "img2.jpg", 2);
    touch(dir.path(), ".hidden.jpg", 3);
    fs::write(dir.path().join("notes.txt"), b"nope").unwrap();
    let vm = ViewerVm::open_paths(&lb, &[dir.path()]).unwrap();
    assert_eq!(vm.current_entry().path, dir.path().join("img2.jpg"));
    assert_eq!(names(&vm), ["img2.jpg", "img10.jpg"]);
}

#[test]
fn viewer_entry_resolves_catalog_photo_by_path() {
    let t = common::library(2);
    let photo = t.lb.photos().get(t.ids[0]).unwrap().unwrap();
    let vm = ViewerVm::open_paths(&t.lb, std::slice::from_ref(&photo.path)).unwrap();
    assert_eq!(vm.current_photo(), Some(t.ids[0]));
    assert_eq!(vm.current_entry().path, photo.path);
}

#[test]
fn viewer_find_by_path_uses_index() {
    let t = common::library(1);
    let photo = t.lb.photos().get(t.ids[0]).unwrap().unwrap();
    let found = t.lb.viewer().find_by_path(&photo.path).unwrap().unwrap();
    assert_eq!(found.id, photo.id);
    let plan = t.lb.viewer().find_by_path_plan().unwrap();
    let text = plan.join("\n");
    assert!(
        text.to_ascii_uppercase().contains("INDEX"),
        "plan should use an index: {text}"
    );
    assert!(
        !text.to_ascii_uppercase().contains("SCAN"),
        "plan should not scan: {text}"
    );
}

#[test]
fn viewer_folder_change_inserts_new_sibling() {
    let (_home, lb) = library();
    let dir = tempfile::tempdir().unwrap();
    touch(dir.path(), "img2.jpg", 1);
    let img10 = touch(dir.path(), "img10.jpg", 2);
    let mut vm = ViewerVm::open_paths(&lb, &[img10]).unwrap();
    assert_eq!(names(&vm), ["img2.jpg", "img10.jpg"]);
    touch(dir.path(), "img02.jpg", 3);
    assert_eq!(vm.refresh().unwrap(), FolderRefresh::Kept);
    assert_eq!(names(&vm), ["img2.jpg", "img02.jpg", "img10.jpg"]);
    assert_eq!(
        vm.current_entry().path.file_name().unwrap(),
        "img10.jpg",
        "the current file stays put"
    );
}

#[test]
fn viewer_current_file_deleted_advances() {
    let (_home, lb) = library();
    let dir = tempfile::tempdir().unwrap();
    let img2 = touch(dir.path(), "img2.jpg", 1);
    touch(dir.path(), "img02.jpg", 2);
    touch(dir.path(), "img10.jpg", 3);
    let mut vm = ViewerVm::open_paths(&lb, std::slice::from_ref(&img2)).unwrap();
    fs::remove_file(&img2).unwrap();
    assert_eq!(vm.refresh().unwrap(), FolderRefresh::Advanced);
    assert_eq!(vm.current_entry().path, dir.path().join("img02.jpg"));
    let last = dir.path().join("img10.jpg");
    let mut vm = ViewerVm::open_paths(&lb, std::slice::from_ref(&last)).unwrap();
    fs::remove_file(&last).unwrap();
    assert_eq!(vm.refresh().unwrap(), FolderRefresh::Advanced);
    assert_eq!(
        vm.current_entry().path,
        dir.path().join("img02.jpg"),
        "deleting the last file steps back"
    );
}

#[test]
fn viewer_proxy_for_path_cached_by_quick_key() {
    let (_home, lb) = library();
    let dir = tempfile::tempdir().unwrap();
    let path = touch(dir.path(), "outside.jpg", 5);
    let key = lb_media::quick_key(&path).unwrap();
    let first = lb.viewer().proxy_for_path(&path, 32).unwrap();
    assert!(
        first
            .file_name()
            .unwrap()
            .to_string_lossy()
            .starts_with(&format!("{key}-32"))
    );
    fs::write(&first, b"sentinel").unwrap();
    let again = lb.viewer().proxy_for_path(&path, 32).unwrap();
    assert_eq!(again, first);
    assert_eq!(fs::read(&again).unwrap(), b"sentinel");
    write_jpeg(&path, 9);
    let fresh = lb.viewer().proxy_for_path(&path, 32).unwrap();
    assert_ne!(fresh, first);
    assert!(fresh.is_file());
    assert_ne!(fs::read(&fresh).unwrap(), b"sentinel");
}

#[test]
fn viewer_unicode_and_space_paths() {
    let (_home, lb) = library();
    let dir = tempfile::tempdir().unwrap();
    let spaced = touch(dir.path(), "my photo.jpg", 1);
    let unicode = touch(dir.path(), "café.png", 2);
    let mut vm = ViewerVm::open_paths(&lb, &[spaced.clone(), unicode.clone()]).unwrap();
    assert_eq!(vm.current_entry().path, spaced);
    assert!(vm.go_next());
    assert_eq!(vm.current_entry().path, unicode);
    let found = lb.viewer().find_by_path(&unicode).unwrap();
    assert!(found.is_none(), "not in the library");
    let _exif = lb.viewer().exif_for_path(&spaced);
}
