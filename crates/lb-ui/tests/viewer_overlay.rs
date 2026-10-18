//! M13.6: the viewer is an overlay on the library (SPEC-VIEWER §V3.1, §V4.2, §V8).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, AppContext as _, Bounds, Entity, InputEvent as _, MouseMoveEvent, Pixels,
    TestAppContext, point, px,
};
use lb_app::viewer::{RectF, fit_area, fit_rect, stage_insets};
use lb_core::PhotoId;
use lb_ui::{LightboxWindow, Route, ids};
use std::time::Duration;

type View = Entity<LightboxWindow>;

fn press(cx: &mut TestAppContext, h: AnyWindowHandle, v: &View, key: &str) {
    cx.update_window(h, |_, window, cx| window.press(key, cx))
        .unwrap();
    settle(cx, h, v);
}

fn open_viewer_on(cx: &mut TestAppContext, h: AnyWindowHandle, v: &View, id: PhotoId) {
    cx.update_window(h, |_, window, cx| {
        window.double_click(ids::grid_cell(id), cx)
    })
    .unwrap();
    settle(cx, h, v);
}

fn advance(lib: &UiLib, cx: &mut TestAppContext, h: AnyWindowHandle, v: &View, ms: u64) {
    lib.clock.advance(Duration::from_millis(ms));
    settle(cx, h, v);
}

fn rect_of(bounds: Bounds<Pixels>) -> RectF {
    RectF {
        x: f32::from(bounds.origin.x),
        y: f32::from(bounds.origin.y),
        w: f32::from(bounds.size.width),
        h: f32::from(bounds.size.height),
    }
}

fn assert_near(actual: RectF, expected: RectF, slack: f32) {
    for (name, a, e) in [
        ("x", actual.x, expected.x),
        ("y", actual.y, expected.y),
        ("w", actual.w, expected.w),
        ("h", actual.h, expected.h),
    ] {
        assert!(
            (a - e).abs() <= slack,
            "{name}: {a} vs {e} (slack {slack}); actual {actual:?} expected {expected:?}"
        );
    }
}

fn image_rect(cx: &mut TestAppContext, h: AnyWindowHandle) -> RectF {
    cx.update_window(h, |_, window, _| {
        rect_of(window.find(ids::VIEWER_IMAGE).bounds())
    })
    .unwrap()
}

fn stage_size(cx: &mut TestAppContext, h: AnyWindowHandle) -> (f32, f32) {
    cx.update_window(h, |_, window, _| {
        let b = window.find(ids::VIEWER_STAGE).bounds();
        (f32::from(b.size.width), f32::from(b.size.height))
    })
    .unwrap()
}

fn fit_for(stage: (f32, f32)) -> RectF {
    let area = fit_area(stage, stage_insets(stage, true, false));
    fit_rect((64.0, 48.0), area, false)
}

fn route(cx: &mut TestAppContext, h: AnyWindowHandle, v: &View) -> Route {
    with_view(cx, h, v, |v, _, _| v.route())
}

fn day_library(count: usize) -> UiLib {
    let names: Vec<String> = (0..count)
        .map(|i| format!("IMG_20210601_{:06}.jpg", 100_000 + i))
        .collect();
    let borrowed: Vec<&str> = names.iter().map(String::as_str).collect();
    ui_library(&[("day", borrowed.as_slice())])
}

#[gpui_kit::test]
fn ui_double_click_opens_overlay_from_thumbnail_rect(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let id = with_view(cx, handle, &view, |v, _, _| v.vm().order()[0]);
    let thumb = with_view(cx, handle, &view, |v, _, _| {
        v.thumbnail_content_rect(id).expect("cell on screen")
    });

    open_viewer_on(cx, handle, &view, id);
    assert_eq!(route(cx, handle, &view), Route::Viewer);
    let at_open = image_rect(cx, handle);
    assert_near(at_open, thumb, 0.5);

    advance(&lib, cx, handle, &view, 260);
    let stage = stage_size(cx, handle);
    assert_near(image_rect(cx, handle), fit_for(stage), 0.5);
}

#[gpui_kit::test]
fn ui_overlay_draws_library_beneath_until_opaque(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let id = with_view(cx, handle, &view, |v, _, _| v.vm().order()[0]);
    open_viewer_on(cx, handle, &view, id);
    cx.update_window(handle, |_, window, _| {
        assert!(
            window.try_find(ids::GRID).is_some(),
            "library under the opening overlay"
        );
        assert!(window.try_find(ids::VIEWER_STAGE).is_some());
        assert!(window.try_find(ids::VIEWER).is_some());
    })
    .unwrap();
    advance(&lib, cx, handle, &view, 260);
    cx.update_window(handle, |_, window, _| {
        assert!(
            window.try_find(ids::GRID).is_none(),
            "library unmounted once the backdrop is opaque"
        );
        assert!(window.try_find(ids::VIEWER_STAGE).is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn ui_viewer_backdrop_click_closes(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let id = with_view(cx, handle, &view, |v, _, _| v.vm().order()[0]);
    open_viewer_on(cx, handle, &view, id);
    advance(&lib, cx, handle, &view, 300);
    cx.update_window(handle, |_, window, cx| {
        window.click_at(ids::VIEWER_BACKDROP, point(px(8.0), px(8.0)), cx);
    })
    .unwrap();
    settle(cx, handle, &view);
    assert_eq!(route(cx, handle, &view), Route::Viewer, "close plays out");
    advance(&lib, cx, handle, &view, 300);
    assert_eq!(route(cx, handle, &view), Route::Library);
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find(ids::VIEWER).is_none());
        assert!(window.try_find(ids::grid_cell(id)).is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn ui_viewer_click_on_image_does_not_close(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let id = with_view(cx, handle, &view, |v, _, _| v.vm().order()[0]);
    open_viewer_on(cx, handle, &view, id);
    advance(&lib, cx, handle, &view, 300);
    cx.update_window(handle, |_, window, cx| window.click(ids::VIEWER_IMAGE, cx))
        .unwrap();
    settle(cx, handle, &view);
    advance(&lib, cx, handle, &view, 300);
    assert_eq!(route(cx, handle, &view), Route::Viewer);
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find(ids::VIEWER_IMAGE).is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn ui_viewer_drag_pan_does_not_close(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let id = with_view(cx, handle, &view, |v, _, _| v.vm().order()[0]);
    open_viewer_on(cx, handle, &view, id);
    advance(&lib, cx, handle, &view, 300);
    press(cx, handle, &view, "+");
    advance(&lib, cx, handle, &view, 200);
    let before = image_rect(cx, handle);
    cx.update_window(handle, |_, window, cx| {
        let origin = window.find(ids::VIEWER_IMAGE).bounds().origin;
        let start = point(origin.x + px(8.0), origin.y + px(8.0));
        let end = point(start.x + px(36.0), start.y + px(18.0));
        mouse_down(window, start, cx);
        mouse_drag_to(window, end, cx);
        mouse_up(window, end, cx);
    })
    .unwrap();
    settle(cx, handle, &view);
    advance(&lib, cx, handle, &view, 300);
    assert_eq!(route(cx, handle, &view), Route::Viewer);
    let after = image_rect(cx, handle);
    assert!(
        (after.x - before.x).abs() > 10.0 || (after.y - before.y).abs() > 10.0,
        "drag pans the image: before {before:?} after {after:?}"
    );
}

#[gpui_kit::test]
fn ui_viewer_close_animates_back_to_cell(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let id = with_view(cx, handle, &view, |v, _, _| v.vm().order()[0]);
    open_viewer_on(cx, handle, &view, id);
    advance(&lib, cx, handle, &view, 300);
    let fitted = image_rect(cx, handle);
    let thumb = with_view(cx, handle, &view, |v, _, _| {
        v.thumbnail_content_rect(id).expect("cell on screen")
    });
    press(cx, handle, &view, "escape");
    assert_eq!(route(cx, handle, &view), Route::Viewer);
    cx.update_window(handle, |_, window, _| {
        assert!(
            window.try_find(ids::GRID).is_some(),
            "library returns on the first close frame"
        );
    })
    .unwrap();
    advance(&lib, cx, handle, &view, 110);
    let mid = image_rect(cx, handle);
    let span = (fitted.x - thumb.x).abs() + (fitted.y - thumb.y).abs();
    let left = (mid.x - thumb.x).abs() + (mid.y - thumb.y).abs();
    assert!(
        left < span * 0.75,
        "image moves toward the cell: fit {fitted:?} mid {mid:?} cell {thumb:?}"
    );
    advance(&lib, cx, handle, &view, 200);
    assert_eq!(route(cx, handle, &view), Route::Library);
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find(ids::grid_cell(id)).is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn ui_viewer_close_after_navigation_reveals_cell(cx: &mut TestAppContext) {
    let lib = day_library(36);
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let all = with_view(cx, handle, &view, |v, _, _| v.vm().order().to_vec());
    let first = all[0];
    let last = *all.last().unwrap();
    cx.update_window(handle, |_, window, _| {
        assert!(
            window.try_find(ids::grid_cell(last)).is_none(),
            "last cell starts off screen"
        );
    })
    .unwrap();
    open_viewer_on(cx, handle, &view, first);
    advance(&lib, cx, handle, &view, 300);
    press(cx, handle, &view, "end");
    advance(&lib, cx, handle, &view, 400);
    assert_eq!(
        with_view(cx, handle, &view, |v, _, _| v.viewer().unwrap().current()),
        last
    );
    press(cx, handle, &view, "escape");
    let revealed = with_view(cx, handle, &view, |v, _, _| v.thumbnail_content_rect(last));
    assert!(
        revealed.is_some(),
        "close scrolls the current cell into view"
    );
    advance(&lib, cx, handle, &view, 300);
    assert_eq!(route(cx, handle, &view), Route::Library);
    let (focused, selected) = with_view(cx, handle, &view, |v, _, _| {
        (v.vm().focused(), v.vm().selected_ids())
    });
    assert_eq!(focused, Some(last));
    assert_eq!(selected, vec![last]);
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find(ids::grid_cell(last)).is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn ui_viewer_arrow_keys_navigate_with_slide(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let all = with_view(cx, handle, &view, |v, _, _| v.vm().order().to_vec());
    open_viewer_on(cx, handle, &view, all[0]);
    advance(&lib, cx, handle, &view, 300);
    let fitted = image_rect(cx, handle);
    let stage_w = stage_size(cx, handle).0;
    press(cx, handle, &view, "right");
    assert_eq!(
        with_view(cx, handle, &view, |v, _, _| v.viewer().unwrap().current()),
        all[1]
    );
    let incoming = image_rect(cx, handle);
    assert!(
        (incoming.x - (fitted.x + 0.06 * stage_w)).abs() <= 1.0,
        "slide starts at +6% of the stage: {incoming:?} fit {fitted:?} stage {stage_w}"
    );
    cx.update_window(handle, |_, window, _| {
        assert!(
            !window.find(ids::VIEWER_IMAGE).visible(),
            "incoming photo starts transparent"
        );
    })
    .unwrap();
    advance(&lib, cx, handle, &view, 200);
    let settled = image_rect(cx, handle);
    assert_near(settled, fitted, 1.0);
    cx.update_window(handle, |_, window, _| {
        assert!(window.find(ids::VIEWER_IMAGE).visible());
    })
    .unwrap();
}

#[gpui_kit::test]
fn ui_viewer_toolbar_autohides(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let id = with_view(cx, handle, &view, |v, _, _| v.vm().order()[0]);
    open_viewer_on(cx, handle, &view, id);
    advance(&lib, cx, handle, &view, 300);
    cx.update_window(handle, |_, window, _| {
        assert!(window.find(ids::VIEWER_TOOLBAR).visible());
        assert!(window.find(ids::VIEWER_FILMSTRIP).visible());
        assert!(window.try_find(ids::VIEWER_NEXT).is_some());
        assert!(window.try_find(ids::VIEWER_STAR).is_some());
    })
    .unwrap();
    advance(&lib, cx, handle, &view, 2_500);
    cx.update_window(handle, |_, window, _| {
        assert!(
            !window.find(ids::VIEWER_TOOLBAR).visible(),
            "toolbar fades out after 2.5 s idle"
        );
        assert!(!window.find(ids::VIEWER_FILMSTRIP).visible());
    })
    .unwrap();
    cx.update_window(handle, |_, window, cx| {
        let at = window.find(ids::VIEWER_STAGE).bounds().center();
        window.dispatch_event(
            MouseMoveEvent {
                position: at,
                pressed_button: None,
                modifiers: Default::default(),
            }
            .to_platform_input(),
            cx,
        );
    })
    .unwrap();
    advance(&lib, cx, handle, &view, 150);
    cx.update_window(handle, |_, window, _| {
        assert!(window.find(ids::VIEWER_TOOLBAR).visible());
    })
    .unwrap();
}

#[gpui_kit::test]
fn ui_viewer_error_card_for_missing_file(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let all = with_view(cx, handle, &view, |v, _, _| v.vm().order().to_vec());
    let missing = lib.app.photos().get(all[0]).unwrap().expect("photo").path;
    std::fs::remove_file(&missing).unwrap();
    open_viewer_on(cx, handle, &view, all[0]);
    advance(&lib, cx, handle, &view, 300);
    cx.update_window(handle, |_, window, _| {
        let card = window.find(ids::VIEWER_ERROR_CARD);
        assert_eq!(card.label(), Some("File not found"));
        assert!(window.try_find(ids::VIEWER_IMAGE).is_none());
    })
    .unwrap();
    press(cx, handle, &view, "right");
    advance(&lib, cx, handle, &view, 400);
    assert_eq!(
        with_view(cx, handle, &view, |v, _, _| v.viewer().unwrap().current()),
        all[1]
    );
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find(ids::VIEWER_ERROR_CARD).is_none());
        assert!(window.try_find(ids::VIEWER_IMAGE).is_some());
    })
    .unwrap();
}

#[gpui_kit::test]
fn ui_viewer_toolbar_star_and_rotate(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let id = with_view(cx, handle, &view, |v, _, _| v.vm().order()[0]);
    open_viewer_on(cx, handle, &view, id);
    advance(&lib, cx, handle, &view, 300);
    cx.update_window(handle, |_, window, cx| {
        window.click(ids::VIEWER_STAR, cx);
        window.click(ids::VIEWER_ROTATE_RIGHT, cx);
    })
    .unwrap();
    settle(cx, handle, &view);
    let photo = lib.app.photos().get(id).unwrap().unwrap();
    assert!(photo.starred);
    assert_eq!(
        lib.app.edits().stack(id).unwrap().ops,
        vec![lb_core::EditOp::RotateQuarter { turns: 1 }]
    );
    cx.update_window(handle, |_, window, _| {
        assert!(
            window.try_find(ids::VIEWER_HINT).is_none(),
            "a catalog rotation is saved, so there is no hint"
        );
    })
    .unwrap();
}
