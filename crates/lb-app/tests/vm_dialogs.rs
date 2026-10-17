//! Sidebar, confirmation and rename view-models.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use lb_app::ViewSource;
use lb_app::vm::{ConfirmVm, RenameVm, SidebarKey, SidebarVm};

#[test]
fn vm_sidebar_sections_counts_and_selection() {
    let lib = build(&[("a", 3), ("b", 2)], lb_app::LightboxOptions::default());
    lib.lb.photos().set_starred(&lib.ids[0..1], true).unwrap();
    let mut sb = SidebarVm::new(&lib.lb).unwrap();
    let titles: Vec<_> = sb.sections().iter().map(|s| s.title).collect();
    assert_eq!(titles, ["Library", "Albums", "Folders"]);
    let lib_items = &sb.sections()[0].items;
    assert_eq!(lib_items[0].count, 5);
    assert_eq!(lib_items[1].count, 1);
    assert_eq!(sb.select(SidebarKey::Starred), Some(ViewSource::Starred));
    assert_eq!(sb.selected(), SidebarKey::Starred);
    assert_eq!(sb.select(SidebarKey::Year(None)), None, "headings are not selectable");

    let (album, source) = sb.create_album().unwrap();
    assert_eq!(album.name, "New album");
    assert_eq!(source, ViewSource::Album(album.id));
    assert_eq!(sb.selected(), SidebarKey::Album(album.id));
    let (second, _) = sb.create_album().unwrap();
    assert_eq!(second.name, "New album 2");
    lib.lb.albums().delete(second.id).unwrap();
    sb.refresh().unwrap();
    assert_eq!(sb.sections()[1].items.len(), 1);
    assert_eq!(SidebarVm::key_for(&ViewSource::Starred), Some(SidebarKey::Starred));
}

#[test]
fn vm_confirm_delete_message_and_accept() {
    let lib = library(3);
    let one = ConfirmVm::delete(lib.ids[0..1].to_vec());
    assert_eq!(one.title(), "Delete photo?");
    assert!(one.message().contains("cannot be undone"));
    let many = ConfirmVm::delete(lib.ids.clone());
    assert!(many.message().contains("these 3 photos"));
    assert_eq!(many.accept(&lib.lb).unwrap(), 3);
}

#[test]
fn vm_rename_validates_live() {
    let lib = library(2);
    let mut vm = RenameVm::new(&lib.lb, lib.ids[0]).unwrap();
    assert_eq!(vm.text(), "p00");
    assert!(!vm.can_accept(), "unchanged");
    vm.set_text(&lib.lb, "a/b");
    assert!(vm.error().is_some() && !vm.can_accept());
    vm.set_text(&lib.lb, "p01");
    assert!(vm.error().unwrap().contains("already exists"), "{:?}", vm.error());
    vm.set_text(&lib.lb, "beach");
    assert!(vm.error().is_none() && vm.can_accept());
    vm.accept(&lib.lb).unwrap();
    let photo = lib.lb.photos().get(lib.ids[0]).unwrap().unwrap();
    assert_eq!(photo.path.file_name().unwrap(), "beach.jpg");
}
