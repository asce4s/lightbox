//! M9.2: the timeline scrubber under the grid.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use chrono::NaiveDate;
use common::*;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, InputEvent as _, TestAppContext, point};
use lb_ui::ids;

fn months_library() -> UiLib {
    let names: Vec<String> = (1..=6)
        .flat_map(|m| (1..=10).map(move |d| format!("IMG_2021{m:02}{d:02}_100000.jpg")))
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    ui_library(&[("all", &refs)])
}

#[gpui_kit::test]
fn ui_timeline_drag_scrolls_grid(cx: &mut TestAppContext) {
    let lib = months_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let (buckets, order) = with_view(cx, handle, &view, |v, _, _| {
        (v.vm().timeline().to_vec(), v.vm().order().to_vec())
    });
    assert_eq!(buckets.len(), 6);
    assert_eq!(buckets[0].month, 6, "newest month first");
    let newest = buckets[0].first;
    let target = buckets[4].first;
    assert_eq!(buckets[4].month, 2);
    assert_eq!(order[0], newest);

    let (from, to) = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            assert!(window.find(ids::grid_cell(newest)).visible());
            assert!(window.try_find(ids::grid_cell(target)).is_none());
            (
                window.find(ids::timeline_month(0)).bounds().center(),
                window.find(ids::timeline_month(4)).bounds().center(),
            )
        })
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        mouse_down(window, from, cx);
        mouse_drag_to(window, point(to.x, to.y), cx);
        mouse_up(window, to, cx);
    })
    .unwrap();
    settle(cx, handle, &view);
    settle(cx, handle, &view);

    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(
            window.try_find(ids::grid_cell(target)).is_some(),
            "the month's first photo is rendered"
        );
        assert!(window.find(ids::grid_cell(target)).visible());
        assert!(
            window.try_find(ids::grid_cell(newest)).is_none(),
            "the top of the grid scrolled away"
        );
    })
    .unwrap();
}

#[gpui_kit::test]
fn ui_timeline_shift_drag_sets_date_range(cx: &mut TestAppContext) {
    let lib = months_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let (from, to) = cx
        .update_window(handle, |_, window, cx| {
            window.render_frame(cx);
            (
                window.find(ids::timeline_month(1)).bounds().center(),
                window.find(ids::timeline_month(3)).bounds().center(),
            )
        })
        .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        window.dispatch_event(
            gpui_kit::MouseDownEvent {
                button: gpui_kit::MouseButton::Left,
                position: from,
                modifiers: SHIFT,
                click_count: 1,
                first_mouse: false,
            }
            .to_platform_input(),
            cx,
        );
        window.dispatch_event(
            gpui_kit::MouseUpEvent {
                button: gpui_kit::MouseButton::Left,
                position: to,
                modifiers: SHIFT,
                click_count: 1,
            }
            .to_platform_input(),
            cx,
        );
    })
    .unwrap();
    settle(cx, handle, &view);
    let (chips, count) = with_view(cx, handle, &view, |v, _, _| {
        (v.vm().view().chips.date, v.vm().order().len())
    });
    assert_eq!(
        chips,
        Some((
            NaiveDate::from_ymd_opt(2021, 3, 1),
            NaiveDate::from_ymd_opt(2021, 5, 31)
        ))
    );
    assert_eq!(count, 30);

    cx.update_window(handle, |_, window, cx| {
        window.click(ids::TIMELINE_CLEAR, cx)
    })
    .unwrap();
    settle(cx, handle, &view);
    assert_eq!(
        with_view(cx, handle, &view, |v, _, _| v.vm().order().len()),
        60
    );
}
