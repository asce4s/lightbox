//! M9.8: rubber-band selection in the grid.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, Bounds, Pixels, Point, TestAppContext, point, px};
use lb_ui::ids;

fn twelve() -> UiLib {
    let names: Vec<String> = (1..=12)
        .map(|d| format!("IMG_202106{d:02}_100000.jpg"))
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    ui_library(&[("all", &refs)])
}

fn cell_bounds(
    cx: &mut TestAppContext,
    handle: gpui_kit::AnyWindowHandle,
    id: lb_core::PhotoId,
) -> Bounds<Pixels> {
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.find(ids::grid_cell(id)).bounds()
    })
    .unwrap()
}

#[gpui_kit::test]
fn ui_rubber_band_selects_cells(cx: &mut TestAppContext) {
    let lib = twelve();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let (order, columns) = with_view(cx, handle, &view, |v, _, _| {
        (v.vm().order().to_vec(), v.vm().layout().columns)
    });
    assert!(columns >= 3 && order.len() > 2 * columns - 1, "{columns}");

    let first = cell_bounds(cx, handle, order[0]);
    let target = cell_bounds(cx, handle, order[columns + 1]);
    let start: Point<Pixels> = point(first.left() + px(10.0), first.top() - px(10.0));
    let end = target.center();

    cx.update_window(handle, |_, window, cx| {
        mouse_down(window, start, cx);
        mouse_drag_to(window, point(end.x, first.center().y), cx);
        mouse_drag_to(window, end, cx);
        window.render_frame(cx);
        assert!(
            window.try_find(ids::GRID_BAND).is_some(),
            "the band is drawn while dragging"
        );
    })
    .unwrap();
    settle(cx, handle, &view);
    let live = with_view(cx, handle, &view, |v, _, _| v.vm().selected_ids());
    let want = vec![order[0], order[1], order[columns], order[columns + 1]];
    assert_eq!(live, want, "two columns by two rows");

    cx.update_window(handle, |_, window, cx| mouse_up(window, end, cx))
        .unwrap();
    settle(cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(ids::GRID_BAND).is_none(), "band removed");
    })
    .unwrap();
    let kept = with_view(cx, handle, &view, |v, _, _| v.vm().selected_ids());
    assert_eq!(kept, want, "the selection stays after release");

    cx.update_window(handle, |_, window, cx| {
        mouse_down(window, start, cx);
        mouse_up(window, start, cx);
    })
    .unwrap();
    settle(cx, handle, &view);
    let cleared = with_view(cx, handle, &view, |v, _, _| v.vm().selected_ids());
    assert!(cleared.is_empty(), "a click on empty space clears");
}

#[gpui_kit::test]
fn ui_press_on_cell_does_not_start_band(cx: &mut TestAppContext) {
    let lib = twelve();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let order = with_view(cx, handle, &view, |v, _, _| v.vm().order().to_vec());
    let a = cell_bounds(cx, handle, order[0]).center();
    let b = cell_bounds(cx, handle, order[2]).center();
    cx.update_window(handle, |_, window, cx| {
        mouse_down(window, a, cx);
        mouse_drag_to(window, b, cx);
        window.render_frame(cx);
        assert!(window.try_find(ids::GRID_BAND).is_none());
        mouse_up(window, b, cx);
    })
    .unwrap();
    settle(cx, handle, &view);
    let selected = with_view(cx, handle, &view, |v, _, _| v.vm().selected_ids());
    assert_eq!(selected, vec![order[0]]);
}
