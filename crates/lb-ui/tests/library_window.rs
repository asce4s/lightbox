//! M7.1 to M7.3: the library window, grid and selection.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use gpui_kit::AppContext as _;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{Modifiers, TestAppContext, point, px};
use lb_ui::ids;

#[gpui_kit::test]
fn ui_launch_shows_grid_with_groups(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        for id in [
            ids::TITLE_BAR,
            ids::CHIP_BAR,
            ids::SIDEBAR,
            ids::GRID,
            ids::TRAY,
            ids::STATUS_BAR,
        ] {
            assert!(window.find(id).visible(), "{id} is visible");
        }
        assert!(window.find(ids::group_header(0)).visible());
        assert!(window.find(ids::group_header(1)).visible());
        for id in &lib.ids {
            assert!(window.find(ids::grid_cell(*id)).visible());
        }
        assert_eq!(window.find(ids::STATUS_TEXT).label(), Some("5 photos"));
    })
    .unwrap();
    let groups = with_view(cx, handle, &view, |v, _, _| {
        v.vm()
            .result()
            .groups
            .iter()
            .map(|g| g.ids.len())
            .collect::<Vec<_>>()
    });
    assert_eq!(groups, vec![2, 3], "July first (newest), then June");
}

fn many(n: usize) -> UiLib {
    let names: Vec<String> = (0..n)
        .map(|i| {
            format!(
                "IMG_20210601_{:02}{:02}{:02}.jpg",
                i / 3600 % 24,
                i / 60 % 60,
                i % 60
            )
        })
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    ui_library(&[("bulk", &refs)])
}

#[gpui_kit::test]
fn ui_grid_renders_only_visible_rows(cx: &mut TestAppContext) {
    let lib = many(300);
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let (columns, order) = with_view(cx, handle, &view, |v, _, _| {
        (v.vm().columns(), v.vm().order().to_vec())
    });
    assert!(columns >= 4, "{columns}");
    let observed = |cx: &mut TestAppContext| {
        cx.update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            order
                .iter()
                .filter(|id| window.try_find(ids::grid_cell(**id)).is_some())
                .count()
        })
        .unwrap()
    };
    let before = observed(cx);
    let visible_rows = 6;
    assert!(before > 0);
    assert!(
        before <= columns * (visible_rows + 3),
        "{before} cells for {columns} columns"
    );
    assert!(before < order.len() / 4, "{before} of {}", order.len());
    let first = order[0];
    cx.update_window(handle, |_, window, cx| {
        assert!(window.find(ids::grid_cell(first)).visible());
        window.scroll(
            ids::GRID,
            gpui_kit::ScrollDelta::Pixels(point(px(0.), px(-4000.))),
            cx,
        );
        assert!(
            window.try_find(ids::grid_cell(first)).is_none(),
            "scrolled away"
        );
    })
    .unwrap();
    let after = observed(cx);
    assert!(
        after > 0 && after <= columns * (visible_rows + 3),
        "{after}"
    );
}

#[gpui_kit::test]
fn ui_zoom_changes_column_count(cx: &mut TestAppContext) {
    let lib = many(40);
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let start = with_view(cx, handle, &view, |v, _, _| v.vm().columns());
    cx.update_window(handle, |_, window, cx| {
        window.click_at(ids::ZOOM_SLIDER, point(px(135.), px(8.)), cx);
    })
    .unwrap();
    settle(cx, handle, &view);
    let (zoomed_in, zoom) = with_view(cx, handle, &view, |v, _, _| {
        (v.vm().columns(), v.vm().zoom())
    });
    assert!(zoom > 250, "{zoom}");
    assert!(zoomed_in < start, "{zoomed_in} < {start}");
    cx.update_window(handle, |_, window, cx| {
        window.click_at(ids::ZOOM_SLIDER, point(px(2.), px(8.)), cx);
    })
    .unwrap();
    settle(cx, handle, &view);
    let (zoomed_out, zoom) = with_view(cx, handle, &view, |v, _, _| {
        (v.vm().columns(), v.vm().zoom())
    });
    assert!(zoom < 120, "{zoom}");
    assert!(zoomed_out > start, "{zoomed_out} > {start}");
}

#[gpui_kit::test]
fn ui_click_selects_and_shift_click_extends(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let order = with_view(cx, handle, &view, |v, _, _| v.vm().order().to_vec());
    cx.update_window(handle, |_, window, cx| {
        window.click(ids::grid_cell(order[1]), cx);
    })
    .unwrap();
    let selected = with_view(cx, handle, &view, |v, _, _| v.vm().selected_ids());
    assert_eq!(selected, vec![order[1]]);
    cx.update_window(handle, |_, window, cx| {
        click_with(window, ids::grid_cell(order[3]), SHIFT, cx);
    })
    .unwrap();
    let selected = with_view(cx, handle, &view, |v, _, _| v.vm().selected_ids());
    assert_eq!(
        selected,
        vec![order[1], order[2], order[3]],
        "range across the group boundary"
    );
    cx.update_window(handle, |_, window, cx| {
        click_with(window, ids::grid_cell(order[0]), CTRL, cx);
    })
    .unwrap();
    let selected = with_view(cx, handle, &view, |v, _, _| v.vm().selected_ids());
    assert_eq!(selected.len(), 4);
    assert!(selected.contains(&order[0]));
    assert_eq!(
        with_view(cx, handle, &view, |v, _, _| v.vm().selection().ids.len()),
        4
    );
    let _ = Modifiers::default();
}

#[gpui_kit::test]
fn ui_keyboard_rating_and_star(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let first = with_view(cx, handle, &view, |v, _, _| v.vm().order()[0]);
    cx.update_window(handle, |_, window, cx| {
        window.click(ids::grid_cell(first), cx);
        window.press("3", cx);
    })
    .unwrap();
    settle(cx, handle, &view);
    assert_eq!(lib.app.photos().get(first).unwrap().unwrap().rating, 3);
    cx.update_window(handle, |_, window, cx| {
        window.press("s", cx);
    })
    .unwrap();
    settle(cx, handle, &view);
    assert!(lib.app.photos().get(first).unwrap().unwrap().starred);
    cx.update_window(handle, |_, window, cx| {
        window.press("0", cx);
    })
    .unwrap();
    settle(cx, handle, &view);
    assert_eq!(lib.app.photos().get(first).unwrap().unwrap().rating, 0);
}

#[gpui_kit::test]
fn ui_arrow_keys_move_focus(cx: &mut TestAppContext) {
    let lib = many(40);
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let (order, columns) = with_view(cx, handle, &view, |v, _, _| {
        (v.vm().order().to_vec(), v.vm().columns())
    });
    cx.update_window(handle, |_, window, cx| {
        window.click(ids::grid_cell(order[0]), cx);
        window.press("right", cx);
    })
    .unwrap();
    assert_eq!(
        with_view(cx, handle, &view, |v, _, _| v.vm().focused()),
        Some(order[1])
    );
    cx.update_window(handle, |_, window, cx| {
        window.press("down", cx);
    })
    .unwrap();
    assert_eq!(
        with_view(cx, handle, &view, |v, _, _| v.vm().focused()),
        Some(order[1 + columns])
    );
    cx.update_window(handle, |_, window, cx| {
        window.press("end", cx);
    })
    .unwrap();
    let last = *order.last().unwrap();
    assert_eq!(
        with_view(cx, handle, &view, |v, _, _| v.vm().focused()),
        Some(last)
    );
    settle(cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find(ids::grid_cell(last)).is_some(),
            "focus scrolled into view"
        );
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.press("escape", cx);
    })
    .unwrap();
    assert!(with_view(cx, handle, &view, |v, _, _| v.vm().selected_ids()).is_empty());
}
