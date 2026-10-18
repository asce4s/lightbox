//! M13.7: catalog actions versus view-only files, and adding a folder.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::write_jpeg;
use lb_app::vm::ViewerVm;
use lb_app::{JobQueue, Lightbox};
use lb_core::{EditOp, RootMode};
use std::fs;
use std::path::{Path, PathBuf};

fn outside(name: &str) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(name);
    write_jpeg(&path, 3);
    (dir, path)
}

fn bytes(path: &Path) -> Vec<u8> {
    fs::read(path).unwrap()
}

#[test]
fn viewer_rotate_catalog_pushes_edit_op() {
    let t = common::library(1);
    let mut vm = ViewerVm::open(&t.lb, t.ids.clone(), t.ids[0]).unwrap();
    vm.rotate(1).unwrap();
    assert_eq!(
        t.lb.edits().stack(t.ids[0]).unwrap().ops,
        vec![EditOp::RotateQuarter { turns: 1 }]
    );
    assert!(vm.rotation_hint().is_none());
    assert_eq!(vm.view_turns(), 0);
}

#[test]
fn viewer_rotate_file_is_view_only() {
    let home = tempfile::tempdir().unwrap();
    let lb = Lightbox::open(home.path(), JobQueue::inline()).unwrap();
    let (_dir, path) = outside("img2.jpg");
    let original = bytes(&path);
    let mut vm = ViewerVm::open_paths(&lb, std::slice::from_ref(&path)).unwrap();
    assert!(vm.current_photo().is_none());
    vm.rotate(1).unwrap();
    assert_eq!(vm.view_turns(), 1);
    assert_eq!(
        vm.rotation_hint(),
        Some("Rotation isn't saved — add this folder to Lightbox to keep edits")
    );
    assert_eq!(bytes(&path), original);
    assert!(lb.photos().all().unwrap().is_empty());
    vm.rotate(1).unwrap();
    assert_eq!(vm.view_turns(), 2);
    vm.rotate(-1).unwrap();
    assert_eq!(vm.view_turns(), 1);
}

#[test]
fn viewer_star_disabled_for_non_catalog() {
    let home = tempfile::tempdir().unwrap();
    let lb = Lightbox::open(home.path(), JobQueue::inline()).unwrap();
    let (_dir, path) = outside("img2.jpg");
    let mut vm = ViewerVm::open_paths(&lb, std::slice::from_ref(&path)).unwrap();
    assert!(!vm.can_star());
    assert!(!vm.can_edit());
    assert!(!vm.toggle_star().unwrap());
    assert!(lb.photos().all().unwrap().is_empty());
}

#[test]
fn viewer_delete_file_uses_trash_backend() {
    let f = common::fakes();
    let home = tempfile::tempdir().unwrap();
    let lb = Lightbox::open_with(home.path(), JobQueue::inline(), f.options).unwrap();
    let (_dir, path) = outside("loose.jpg");
    let mut vm = ViewerVm::open_paths(&lb, std::slice::from_ref(&path)).unwrap();
    assert!(vm.current_photo().is_none());
    vm.delete_current().unwrap();
    assert_eq!(f.trash.trashed(), vec![path.clone()]);
    assert!(!path.exists());
    assert!(lb.photos().all().unwrap().is_empty());
}

#[test]
fn viewer_add_folder_enables_catalog_actions() {
    let home = tempfile::tempdir().unwrap();
    let lb = Lightbox::open(home.path(), JobQueue::inline()).unwrap();
    let (dir, path) = outside("img2.jpg");
    let mut vm = ViewerVm::open_paths(&lb, std::slice::from_ref(&path)).unwrap();
    assert!(!vm.can_star());
    let id = vm.add_folder(RootMode::Once).unwrap().expect("indexed");
    assert!(vm.can_star());
    assert_eq!(vm.current_photo(), Some(id));
    assert_eq!(vm.current_entry().path, path);
    assert!(vm.toggle_star().unwrap());
    assert!(lb.photos().get(id).unwrap().unwrap().starred);
    vm.rotate(1).unwrap();
    assert_eq!(
        lb.edits().stack(id).unwrap().ops,
        vec![EditOp::RotateQuarter { turns: 1 }]
    );
    assert!(vm.rotation_hint().is_none());
    assert!(dir.path().join("img2.jpg").is_file());
}
