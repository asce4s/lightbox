//! M7.4 to M7.8: sidebar, search, chips, menus, dialogs, tray and export.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, TestAppContext, point, px};
use lb_app::ViewSource;
use lb_app::vm::SidebarKey;
use lb_ui::{Overlay, ids};

fn order(
    cx: &mut TestAppContext,
    h: gpui_kit::AnyWindowHandle,
    v: &gpui_kit::Entity<lb_ui::LightboxWindow>,
) -> Vec<lb_core::PhotoId> {
    with_view(cx, h, v, |v, _, _| v.vm().order().to_vec())
}

fn total(
    cx: &mut TestAppContext,
    h: gpui_kit::AnyWindowHandle,
    v: &gpui_kit::Entity<lb_ui::LightboxWindow>,
) -> usize {
    with_view(cx, h, v, |v, _, _| v.vm().result().total)
}

#[gpui_kit::test]
fn ui_sidebar_navigation_changes_source(cx: &mut TestAppContext) {
    let lib = basic_library();
    lib.app.photos().set_starred(&lib.ids[0..1], true).unwrap();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    assert_eq!(total(cx, handle, &view), 5);

    cx.update_window(handle, |_, window, cx| {
        window.click(ids::SIDEBAR_STARRED, cx)
    })
    .unwrap();
    settle(cx, handle, &view);
    assert_eq!(
        with_view(cx, handle, &view, |v, _, _| v.vm().view().source.clone()),
        ViewSource::Starred
    );
    assert_eq!(total(cx, handle, &view), 1);

    let july = with_view(cx, handle, &view, |v, _, _| {
        v.sidebar()
            .sections()
            .iter()
            .flat_map(|s| &s.items)
            .find(|i| i.label == "july")
            .map(|i| i.key)
            .unwrap()
    });
    let SidebarKey::Folder(july_id) = july else {
        panic!("not a folder: {july:?}")
    };
    cx.update_window(handle, |_, window, cx| {
        window.click(ids::folder_item(july_id), cx)
    })
    .unwrap();
    settle(cx, handle, &view);
    assert_eq!(
        with_view(cx, handle, &view, |v, _, _| v.vm().view().source.clone()),
        ViewSource::Folder(july_id)
    );
    assert_eq!(total(cx, handle, &view), 2);
    let count = with_view(cx, handle, &view, |v, _, _| {
        v.sidebar()
            .sections()
            .iter()
            .flat_map(|s| &s.items)
            .find(|i| i.key == july)
            .map(|i| i.count)
    });
    assert_eq!(count, Some(2), "folder badge shows the count");

    cx.update_window(handle, |_, window, cx| window.click(ids::SIDEBAR_ALL, cx))
        .unwrap();
    settle(cx, handle, &view);
    assert_eq!(total(cx, handle, &view), 5);
}

#[gpui_kit::test]
fn ui_new_album_and_add_selection(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let ids_in_order = order(cx, handle, &view);

    cx.update_window(handle, |_, window, cx| {
        window.click(ids::NEW_ALBUM_BUTTON, cx)
    })
    .unwrap();
    settle(cx, handle, &view);
    let albums = lib.app.albums().list().unwrap();
    assert_eq!(albums.len(), 1);
    assert_eq!(albums[0].name, "New album");
    let album = albums[0].id;
    cx.update_window(handle, |_, window, cx| {
        assert!(window.find(ids::album_item(album)).visible());
        window.click(ids::SIDEBAR_ALL, cx);
    })
    .unwrap();
    settle(cx, handle, &view);

    cx.update_window(handle, |_, window, cx| {
        window.click(ids::grid_cell(ids_in_order[0]), cx);
        click_with(window, ids::grid_cell(ids_in_order[1]), SHIFT, cx);
        window.click(ids::TRAY_ADD_ALBUM, cx);
        assert!(window.find(ids::ALBUM_PICKER).visible());
        window.click(ids::album_pick(album), cx);
        assert!(window.try_find(ids::ALBUM_PICKER).is_none());
    })
    .unwrap();
    settle(cx, handle, &view);
    let mut members = lib.app.albums().photo_ids(album).unwrap();
    members.sort();
    let mut expected = vec![ids_in_order[0], ids_in_order[1]];
    expected.sort();
    assert_eq!(members, expected);
    let badge = with_view(cx, handle, &view, |v, _, _| {
        v.sidebar()
            .sections()
            .iter()
            .flat_map(|s| &s.items)
            .find(|i| i.key == SidebarKey::Album(album))
            .map(|i| i.count)
    });
    assert_eq!(badge, Some(2));

    cx.update_window(handle, |_, window, cx| {
        window.click(ids::TRAY_ADD_ALBUM, cx);
        window.click(ids::ALBUM_PICKER_NEW, cx);
    })
    .unwrap();
    settle(cx, handle, &view);
    let albums = lib.app.albums().list().unwrap();
    assert_eq!(albums.len(), 2);
    assert_eq!(albums[1].name, "New album 2");
    assert_eq!(lib.app.albums().photo_ids(albums[1].id).unwrap().len(), 2);
}

#[gpui_kit::test]
fn ui_search_filters_grid(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.click(ids::SEARCH_INPUT, cx);
        window.input("folder:july", cx);
    })
    .unwrap();
    settle(cx, handle, &view);
    assert_eq!(total(cx, handle, &view), 5, "debounced: nothing yet");
    lib.clock.advance(std::time::Duration::from_millis(200));
    settle(cx, handle, &view);
    assert_eq!(total(cx, handle, &view), 2);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(ids::group_header(1)).is_none());
    })
    .unwrap();

    cx.update_window(handle, |_, window, cx| {
        window.press("ctrl-a", cx);
        window.input("zzzz-no-match", cx);
        window.press("enter", cx);
    })
    .unwrap();
    settle(cx, handle, &view);
    assert_eq!(total(cx, handle, &view), 0);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find(ids::GRID_EMPTY).visible());
    })
    .unwrap();
}

#[gpui_kit::test]
fn ui_chip_starred_filters(cx: &mut TestAppContext) {
    let lib = basic_library();
    lib.app.photos().set_starred(&lib.ids[1..3], true).unwrap();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    assert_eq!(total(cx, handle, &view), 5);
    cx.update_window(handle, |_, window, cx| window.click(ids::CHIP_STARRED, cx))
        .unwrap();
    settle(cx, handle, &view);
    assert_eq!(total(cx, handle, &view), 2);
    assert!(with_view(cx, handle, &view, |v, _, _| v
        .vm()
        .view()
        .chips
        .starred));
    cx.update_window(handle, |_, window, cx| window.click(ids::CHIP_STARRED, cx))
        .unwrap();
    settle(cx, handle, &view);
    assert_eq!(total(cx, handle, &view), 5);
}

#[gpui_kit::test]
fn ui_delete_shows_confirm_and_cancels(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let target = order(cx, handle, &view)[0];
    let path = lib.app.photos().get(target).unwrap().unwrap().path;

    cx.update_window(handle, |_, window, cx| {
        window.click(ids::grid_cell(target), cx);
        window.press("delete", cx);
        assert!(window.find(ids::CONFIRM_DIALOG).visible());
        window.click(ids::CONFIRM_CANCEL, cx);
        assert!(window.try_find(ids::CONFIRM_DIALOG).is_none());
    })
    .unwrap();
    assert!(path.exists());
    assert!(lib.app.photos().get(target).unwrap().is_some());
    assert!(lib.trash.trashed().is_empty());

    cx.update_window(handle, |_, window, cx| {
        window.press("delete", cx);
        assert!(window.find(ids::CONFIRM_DIALOG).visible());
        window.click(ids::CONFIRM_OK, cx);
    })
    .unwrap();
    settle(cx, handle, &view);
    assert!(!path.exists());
    assert!(lib.app.photos().get(target).unwrap().is_none());
    assert_eq!(lib.trash.trashed().len(), 1);
    assert_eq!(total(cx, handle, &view), 4);
}

#[gpui_kit::test]
fn ui_rename_dialog_validates(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let target = order(cx, handle, &view)[0];
    let old = lib.app.photos().get(target).unwrap().unwrap().path;

    cx.update_window(handle, |_, window, cx| {
        window.right_click(ids::grid_cell(target), cx);
        assert!(window.find(ids::CONTEXT_MENU).visible());
        window.click(ids::MENU_RENAME, cx);
        assert!(window.find(ids::RENAME_DIALOG).visible());
        window.click(ids::RENAME_OK, cx);
        assert!(
            window.find(ids::RENAME_DIALOG).visible(),
            "unchanged name cannot be accepted"
        );
        window.press("ctrl-a", cx);
        window.input("bad/name", cx);
    })
    .unwrap();
    settle(cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find(ids::RENAME_ERROR).visible());
        window.click(ids::RENAME_OK, cx);
        assert!(
            window.find(ids::RENAME_DIALOG).visible(),
            "invalid name cannot be accepted"
        );
        window.press("ctrl-a", cx);
        window.input("holiday", cx);
    })
    .unwrap();
    settle(cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(ids::RENAME_ERROR).is_none());
        window.click(ids::RENAME_OK, cx);
    })
    .unwrap();
    settle(cx, handle, &view);
    let new = lib.app.photos().get(target).unwrap().unwrap().path;
    assert_eq!(new.file_name().unwrap(), "holiday.jpg");
    assert!(new.exists() && !old.exists());
    assert!(with_view(cx, handle, &view, |v, _, _| matches!(
        v.overlay(),
        Overlay::None
    )));
    assert_eq!(std::fs::read_dir(old.parent().unwrap()).unwrap().count(), 2);
}

#[gpui_kit::test]
fn ui_tray_hold_and_clear(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let ids_in_order = order(cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.click(ids::grid_cell(ids_in_order[0]), cx);
        click_with(window, ids::grid_cell(ids_in_order[1]), SHIFT, cx);
        window.click(ids::TRAY_HOLD, cx);
        assert!(window.find(ids::tray_cell(ids_in_order[0])).visible());
        assert!(window.find(ids::tray_cell(ids_in_order[1])).visible());
        window.click(ids::SIDEBAR_STARRED, cx);
    })
    .unwrap();
    settle(cx, handle, &view);
    assert_eq!(
        with_view(cx, handle, &view, |v, _, _| v.vm().tray().len()),
        2,
        "tray survives view changes"
    );
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.find(ids::tray_cell(ids_in_order[0])).visible());
        window.click(ids::TRAY_CLEAR, cx);
        assert!(window.try_find(ids::tray_cell(ids_in_order[0])).is_none());
    })
    .unwrap();
    assert!(with_view(cx, handle, &view, |v, _, _| v
        .vm()
        .tray()
        .is_empty()));
}

#[gpui_kit::test]
fn ui_star_from_tray_updates_catalog(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let o = order(cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.click(ids::grid_cell(o[0]), cx);
        click_with(window, ids::grid_cell(o[1]), CTRL, cx);
        window.click(ids::TRAY_HOLD, cx);
        window.click(ids::grid_cell(o[3]), cx);
        window.click(ids::TRAY_STAR, cx);
    })
    .unwrap();
    settle(cx, handle, &view);
    let starred = |id| lib.app.photos().get(id).unwrap().unwrap().starred;
    assert!(starred(o[0]) && starred(o[1]), "the tray is the target");
    assert!(
        !starred(o[3]),
        "the selection is ignored while the tray holds photos"
    );
    cx.update_window(handle, |_, window, cx| window.click(ids::TRAY_CLEAR, cx))
        .unwrap();
    cx.update_window(handle, |_, window, cx| window.click(ids::TRAY_STAR, cx))
        .unwrap();
    settle(cx, handle, &view);
    assert!(
        starred(o[3]),
        "with an empty tray the selection is the target"
    );
}

#[gpui_kit::test]
fn ui_export_dialog_runs_job(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let o = order(cx, handle, &view);
    let out = tempfile::tempdir().unwrap();
    let dest = out.path().join("exported");
    cx.update_window(handle, |_, window, cx| {
        window.click(ids::grid_cell(o[0]), cx);
        click_with(window, ids::grid_cell(o[1]), SHIFT, cx);
        window.click(ids::TRAY_EXPORT, cx);
        assert!(window.find(ids::EXPORT_DIALOG).visible());
        window.click(ids::EXPORT_CONFIRM, cx);
        assert_eq!(
            window.find(ids::EXPORT_STATUS).label(),
            Some("Choose a destination folder."),
            "no destination yet, nothing starts"
        );
        window.click(ids::EXPORT_DEST, cx);
        window.input(dest.to_str().unwrap(), cx);
    })
    .unwrap();
    settle(cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.click(ids::EXPORT_CONFIRM, cx);
    })
    .unwrap();
    settle(cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert_eq!(
            window.find(ids::EXPORT_STATUS).label(),
            Some("Exported 2 photos")
        );
    })
    .unwrap();
    let written = std::fs::read_dir(&dest).unwrap().count();
    assert_eq!(written, 2);
    let _ = (point(px(0.), px(0.)), cx.update(|_| ()));
}
