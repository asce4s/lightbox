//! M9.5: the slideshow screen.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, TestAppContext};
use lb_ui::{Route, ids};
use std::time::Duration;

fn route(
    cx: &mut TestAppContext,
    h: gpui_kit::AnyWindowHandle,
    v: &gpui_kit::Entity<lb_ui::LightboxWindow>,
) -> Route {
    with_view(cx, h, v, |v, _, _| v.route())
}

fn press(cx: &mut TestAppContext, h: gpui_kit::AnyWindowHandle, key: &str) {
    cx.update_window(h, |_, window, cx| window.press(key, cx))
        .unwrap();
}

#[gpui_kit::test]
fn ui_slideshow_escape_exits(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    assert_eq!(route(cx, handle, &view), Route::Library);

    cx.update_window(handle, |_, window, cx| {
        window.click(ids::SLIDESHOW_BUTTON, cx)
    })
    .unwrap();
    settle(cx, handle, &view);
    assert_eq!(route(cx, handle, &view), Route::Slideshow);
    let first = with_view(cx, handle, &view, |v, _, _| {
        v.slideshow().and_then(|s| s.current())
    });
    assert!(first.is_some());

    lib.clock.advance(Duration::from_millis(3100));
    settle(cx, handle, &view);
    let second = with_view(cx, handle, &view, |v, _, _| {
        v.slideshow().and_then(|s| s.current())
    });
    assert_ne!(first, second, "advances with the clock");

    press(cx, handle, "space");
    lib.clock.advance(Duration::from_secs(10));
    settle(cx, handle, &view);
    let paused = with_view(cx, handle, &view, |v, _, _| {
        v.slideshow().and_then(|s| s.current())
    });
    assert_eq!(paused, second, "space pauses");

    press(cx, handle, "right");
    settle(cx, handle, &view);
    let moved = with_view(cx, handle, &view, |v, _, _| {
        v.slideshow().and_then(|s| s.current())
    });
    assert_ne!(moved, second, "arrow keys move");

    press(cx, handle, "escape");
    settle(cx, handle, &view);
    assert_eq!(route(cx, handle, &view), Route::Library);
    assert!(with_view(cx, handle, &view, |v, _, _| v
        .slideshow()
        .is_none()));
    assert_eq!(
        with_view(cx, handle, &view, |v, _, _| v.vm().focused()),
        moved,
        "the grid focuses the photo that was showing"
    );
}

#[gpui_kit::test]
fn ui_slideshow_controls_hide_when_idle(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.click(ids::SLIDESHOW_BUTTON, cx)
    })
    .unwrap();
    settle(cx, handle, &view);
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find(ids::SLIDESHOW_CONTROLS).is_some());
    })
    .unwrap();
    lib.clock.advance(Duration::from_millis(2100));
    settle(cx, handle, &view);
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find(ids::SLIDESHOW_CONTROLS).is_none());
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| window.press("space", cx))
        .unwrap();
    settle(cx, handle, &view);
    cx.update_window(handle, |_, window, _| {
        assert!(
            window.try_find(ids::SLIDESHOW_CONTROLS).is_some(),
            "activity shows them"
        );
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        window.click(ids::SLIDESHOW_EXIT, cx)
    })
    .unwrap();
    settle(cx, handle, &view);
    assert_eq!(route(cx, handle, &view), Route::Library);
}
