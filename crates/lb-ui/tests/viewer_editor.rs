//! M8: viewer and editor.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};
use lb_app::vm::ZoomMode;
use lb_core::PhotoId;
use lb_ui::{LightboxWindow, Route, ids};

type View = Entity<LightboxWindow>;

fn order(cx: &mut TestAppContext, h: AnyWindowHandle, v: &View) -> Vec<PhotoId> {
    with_view(cx, h, v, |v, _, _| v.vm().order().to_vec())
}

fn current(cx: &mut TestAppContext, h: AnyWindowHandle, v: &View) -> PhotoId {
    with_view(cx, h, v, |v, _, _| v.viewer().unwrap().current())
}

fn press(cx: &mut TestAppContext, h: AnyWindowHandle, v: &View, key: &str) {
    cx.update_window(h, |_, window, cx| window.press(key, cx))
        .unwrap();
    settle(cx, h, v);
}

fn click(
    cx: &mut TestAppContext,
    h: AnyWindowHandle,
    v: &View,
    id: impl Into<gpui_kit::ElementId>,
) {
    let id = id.into();
    cx.update_window(h, |_, window, cx| window.click(id, cx))
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

#[gpui_kit::test]
fn ui_open_viewer_and_navigate(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let all = order(cx, handle, &view);
    assert_eq!(all.len(), 5);

    open_viewer_on(cx, handle, &view, all[0]);
    assert_eq!(
        with_view(cx, handle, &view, |v, _, _| v.route()),
        Route::Viewer
    );
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find(ids::VIEWER).is_some());
        assert!(window.try_find(ids::GRID).is_none(), "library is hidden");
        assert!(
            window.try_find(ids::VIEWER_PREV).is_none(),
            "first photo has no previous"
        );
        assert!(window.try_find(ids::VIEWER_NEXT).is_some());
        for id in &all {
            assert!(window.try_find(ids::filmstrip_cell(*id)).is_some());
        }
    })
    .unwrap();
    assert_eq!(current(cx, handle, &view), all[0]);

    press(cx, handle, &view, "right");
    assert_eq!(current(cx, handle, &view), all[1]);
    click(cx, handle, &view, ids::VIEWER_NEXT);
    assert_eq!(current(cx, handle, &view), all[2]);
    click(cx, handle, &view, ids::VIEWER_PREV);
    assert_eq!(current(cx, handle, &view), all[1]);
    press(cx, handle, &view, "end");
    assert_eq!(current(cx, handle, &view), all[4]);
    press(cx, handle, &view, "right");
    assert_eq!(current(cx, handle, &view), all[4], "no wrap at the end");
    press(cx, handle, &view, "home");
    assert_eq!(current(cx, handle, &view), all[0]);
    click(cx, handle, &view, ids::filmstrip_cell(all[3]));
    assert_eq!(current(cx, handle, &view), all[3]);

    let cache = lib.app.paths().cache.join("previews");
    let image = with_view(cx, handle, &view, |v, _, _| v.viewer_image()).expect("an image");
    assert!(image.starts_with(&cache), "proxy rendered: {image:?}");
    assert!(image.exists());

    press(cx, handle, &view, "i");
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find(ids::VIEWER_INFO_PANEL).is_some());
    })
    .unwrap();
    press(cx, handle, &view, "i");
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find(ids::VIEWER_INFO_PANEL).is_none());
    })
    .unwrap();

    press(cx, handle, &view, "1");
    assert_eq!(
        with_view(cx, handle, &view, |v, _, _| v.viewer().unwrap().zoom()),
        ZoomMode::Actual
    );
    press(cx, handle, &view, "1");
    press(cx, handle, &view, "+");
    let zoom = with_view(cx, handle, &view, |v, _, _| v.viewer().unwrap().zoom());
    assert!(matches!(zoom, ZoomMode::Factor(f) if f > 0.0), "{zoom:?}");
}

#[gpui_kit::test]
fn ui_viewer_escape_returns_focus(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let all = order(cx, handle, &view);
    open_viewer_on(cx, handle, &view, all[2]);
    press(cx, handle, &view, "right");
    press(cx, handle, &view, "right");
    assert_eq!(current(cx, handle, &view), all[4]);

    press(cx, handle, &view, "escape");
    let (route, focused, selected) = with_view(cx, handle, &view, |v, _, _| {
        (v.route(), v.vm().focused(), v.vm().selected_ids())
    });
    assert_eq!(route, Route::Library);
    assert_eq!(focused, Some(all[4]));
    assert_eq!(selected, vec![all[4]]);
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find(ids::VIEWER).is_none());
        assert!(window.try_find(ids::grid_cell(all[4])).is_some());
    })
    .unwrap();
}

// ---- editor ----

use gpui_kit::{point, px};
use lb_app::vm::EditorTab;
use lb_core::{EditOp, EditStack};
use lb_ui::{Overlay, Tool};

struct Ed {
    lib: UiLib,
    handle: AnyWindowHandle,
    view: View,
    id: PhotoId,
}

fn open_editor(cx: &mut TestAppContext) -> Ed {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    let all = order(cx, handle, &view);
    open_viewer_on(cx, handle, &view, all[0]);
    press(cx, handle, &view, "e");
    assert_eq!(
        with_view(cx, handle, &view, |v, _, _| v.route()),
        Route::Editor
    );
    settle(cx, handle, &view);
    Ed {
        lib,
        handle,
        view,
        id: all[0],
    }
}

impl Ed {
    fn click(&self, cx: &mut TestAppContext, id: impl Into<gpui_kit::ElementId>) {
        click(cx, self.handle, &self.view, id);
    }

    fn stack(&self) -> EditStack {
        self.lib.app.edits().stack(self.id).unwrap()
    }

    fn ops(&self) -> Vec<EditOp> {
        self.stack().ops
    }

    fn image(&self, cx: &mut TestAppContext) -> Option<std::path::PathBuf> {
        with_view(cx, self.handle, &self.view, |v, _, _| v.editor_image())
    }

    fn tab(&self, cx: &mut TestAppContext, tab: EditorTab) {
        let id = match tab {
            EditorTab::Basic => ids::EDITOR_TAB_BASIC,
            EditorTab::Tuning => ids::EDITOR_TAB_TUNING,
            EditorTab::Effects => ids::EDITOR_TAB_EFFECTS,
        };
        self.click(cx, id);
        assert_eq!(
            with_view(cx, self.handle, &self.view, |v, _, _| v
                .editor()
                .unwrap()
                .tab()),
            tab
        );
    }

    fn draft(&self, cx: &mut TestAppContext) -> Option<EditOp> {
        with_view(cx, self.handle, &self.view, |v, _, _| {
            v.editor().unwrap().draft().cloned()
        })
    }

    fn overlay_is_confirm(&self, cx: &mut TestAppContext) -> bool {
        with_view(cx, self.handle, &self.view, |v, _, _| {
            matches!(v.overlay(), Overlay::Confirm(_))
        })
    }
}

#[gpui_kit::test]
fn ui_editor_lucky_then_undo(cx: &mut TestAppContext) {
    let ed = open_editor(cx);
    assert!(ed.ops().is_empty());
    let original = ed.image(cx).expect("a preview of the unedited photo");
    assert!(original.starts_with(ed.lib.app.paths().cache.join("previews")));

    ed.click(cx, ids::EDITOR_LUCKY);
    assert_eq!(ed.ops(), vec![EditOp::FeelingLucky]);
    let edited = ed.image(cx).expect("a preview of the edited photo");
    assert_ne!(edited, original, "preview follows the stack");

    ed.click(cx, ids::EDITOR_UNDO);
    assert!(ed.ops().is_empty());
    assert_eq!(ed.image(cx).as_ref(), Some(&original));
    assert!(with_view(cx, ed.handle, &ed.view, |v, _, _| v
        .editor()
        .unwrap()
        .can_redo()
        .unwrap()));

    ed.click(cx, ids::EDITOR_REDO);
    assert_eq!(ed.ops(), vec![EditOp::FeelingLucky]);
    assert_eq!(ed.image(cx).as_ref(), Some(&edited));

    ed.click(cx, ids::EDITOR_AUTO_CONTRAST);
    ed.click(cx, ids::EDITOR_AUTO_COLOR);
    assert_eq!(
        ed.ops(),
        vec![
            EditOp::FeelingLucky,
            EditOp::AutoContrast,
            EditOp::AutoColor
        ]
    );
    press(cx, ed.handle, &ed.view, "escape");
    assert_eq!(
        with_view(cx, ed.handle, &ed.view, |v, _, _| v.route()),
        Route::Viewer,
        "Back returns to the viewer it was opened from"
    );
}

#[gpui_kit::test]
fn ui_editor_revert_confirm(cx: &mut TestAppContext) {
    let ed = open_editor(cx);
    ed.click(cx, ids::EDITOR_LUCKY);
    ed.click(cx, ids::EDITOR_AUTO_CONTRAST);
    assert_eq!(ed.ops().len(), 2);

    ed.click(cx, ids::EDITOR_REVERT);
    assert!(ed.overlay_is_confirm(cx));
    ed.click(cx, ids::CONFIRM_CANCEL);
    assert!(!ed.overlay_is_confirm(cx));
    assert_eq!(ed.ops().len(), 2, "cancel keeps the edits");

    ed.click(cx, ids::EDITOR_REVERT);
    ed.click(cx, ids::CONFIRM_OK);
    assert!(ed.ops().is_empty());
    let original = EditStack::new();
    let shown = ed.image(cx).unwrap();
    let expected = ed
        .lib
        .app
        .edits()
        .preview_file(
            ed.id,
            &original,
            1280,
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(shown, expected.path);

    ed.click(cx, ids::EDITOR_REDO);
    assert_eq!(ed.ops().len(), 2, "revert is undoable once");
}

#[gpui_kit::test]
fn ui_editor_slider_commits_one_op(cx: &mut TestAppContext) {
    let ed = open_editor(cx);
    ed.tab(cx, EditorTab::Tuning);
    let bounds = cx
        .update_window(ed.handle, |_, window, _| {
            window.find(ids::SLIDER_FILL_LIGHT).bounds()
        })
        .unwrap();
    let start = bounds.center();
    let mid = point(bounds.origin.x + bounds.size.width * 0.6, start.y);
    let end = point(bounds.origin.x + bounds.size.width * 0.9, start.y);

    cx.update_window(ed.handle, |_, window, cx| {
        mouse_down(window, start, cx);
        mouse_drag_to(window, mid, cx);
    })
    .unwrap();
    settle(cx, ed.handle, &ed.view);
    assert!(ed.ops().is_empty(), "nothing is stored while dragging");
    let Some(EditOp::Tuning { fill_light, .. }) = ed.draft(cx) else {
        panic!("a tuning draft is expected");
    };
    assert!(fill_light > 0.3, "{fill_light}");

    cx.update_window(ed.handle, |_, window, cx| {
        mouse_drag_to(window, end, cx);
        mouse_up(window, end, cx);
    })
    .unwrap();
    settle(cx, ed.handle, &ed.view);
    let ops = ed.ops();
    assert_eq!(ops.len(), 1, "{ops:?}");
    let EditOp::Tuning {
        fill_light: stored, ..
    } = ops[0]
    else {
        panic!("{ops:?}");
    };
    assert!(stored > fill_light);
    assert!(ed.draft(cx).is_none());

    let again = point(bounds.origin.x + bounds.size.width * 0.2, start.y);
    cx.update_window(ed.handle, |_, window, cx| {
        mouse_down(window, again, cx);
        mouse_up(window, again, cx);
    })
    .unwrap();
    settle(cx, ed.handle, &ed.view);
    assert_eq!(ed.ops().len(), 1, "a second drag replaces the Tuning op");
}

#[gpui_kit::test]
fn ui_effect_apply_and_cancel(cx: &mut TestAppContext) {
    let ed = open_editor(cx);
    ed.tab(cx, EditorTab::Effects);
    settle(cx, ed.handle, &ed.view);
    for key in lb_app::vm::EFFECTS.map(|e| e.key) {
        cx.update_window(ed.handle, |_, window, _| {
            assert!(window.try_find(ids::effect_tile(key)).is_some(), "{key}");
        })
        .unwrap();
        let preview = with_view(cx, ed.handle, &ed.view, |v, _, _| {
            v.effect_tile_preview(key)
        });
        assert!(preview.is_some_and(|p| p.exists()), "mini preview of {key}");
    }

    ed.click(cx, ids::effect_tile("vignette"));
    assert!(with_view(cx, ed.handle, &ed.view, |v, _, _| v
        .effect_panel()
        .is_some()));
    assert!(matches!(ed.draft(cx), Some(EditOp::Vignette { .. })));
    ed.click(cx, ids::EFFECT_CANCEL);
    assert!(with_view(cx, ed.handle, &ed.view, |v, _, _| v
        .effect_panel()
        .is_none()));
    assert!(ed.ops().is_empty(), "cancel stores nothing");
    assert!(ed.draft(cx).is_none());

    ed.click(cx, ids::effect_tile("vignette"));
    let bounds = cx
        .update_window(ed.handle, |_, window, _| {
            window.find(ids::effect_param("amount")).bounds()
        })
        .unwrap();
    let target = point(bounds.origin.x + bounds.size.width * 0.9, bounds.center().y);
    cx.update_window(ed.handle, |_, window, cx| {
        mouse_down(window, target, cx);
        mouse_up(window, target, cx);
    })
    .unwrap();
    settle(cx, ed.handle, &ed.view);
    assert!(ed.ops().is_empty(), "moving a parameter does not store it");
    ed.click(cx, ids::EFFECT_APPLY);
    let ops = ed.ops();
    assert_eq!(ops.len(), 1, "{ops:?}");
    let EditOp::Vignette { amount, .. } = ops[0] else {
        panic!("{ops:?}");
    };
    assert!(amount > 0.8, "slider value was applied: {amount}");

    ed.click(cx, ids::effect_tile("sepia"));
    assert_eq!(ed.ops().last(), Some(&EditOp::Sepia));
    assert_eq!(ed.ops().len(), 2);
}

#[gpui_kit::test]
fn ui_crop_apply_pushes_crop_op(cx: &mut TestAppContext) {
    let ed = open_editor(cx);
    ed.click(cx, ids::EDITOR_CROP);
    ed.click(cx, ids::crop_aspect("1x1"));
    let ratio = |cx: &mut TestAppContext| {
        with_view(cx, ed.handle, &ed.view, |v, _, _| match v.editor_tool() {
            Some(Tool::Crop(c)) => c.pixel_ratio(),
            _ => panic!("crop tool expected"),
        })
    };
    assert!((ratio(cx) - 1.0).abs() < 0.01);

    let handle_pos = cx
        .update_window(ed.handle, |_, window, _| {
            window
                .find(ids::crop_handle("bottom-right"))
                .bounds()
                .center()
        })
        .unwrap();
    let area_before = with_view(cx, ed.handle, &ed.view, |v, _, _| match v.editor_tool() {
        Some(Tool::Crop(c)) => c.rect().w,
        _ => 0.0,
    });
    let to = point(handle_pos.x - px(60.), handle_pos.y - px(60.));
    cx.update_window(ed.handle, |_, window, cx| {
        mouse_down(window, handle_pos, cx);
        mouse_drag_to(window, to, cx);
        mouse_up(window, to, cx);
    })
    .unwrap();
    settle(cx, ed.handle, &ed.view);
    let area_after = with_view(cx, ed.handle, &ed.view, |v, _, _| match v.editor_tool() {
        Some(Tool::Crop(c)) => c.rect().w,
        _ => 0.0,
    });
    assert!(area_after < area_before, "{area_after} < {area_before}");
    assert!(
        (ratio(cx) - 1.0).abs() < 0.01,
        "aspect stays locked while dragging"
    );

    ed.click(cx, ids::TOOL_APPLY);
    let ops = ed.ops();
    assert_eq!(ops.len(), 1, "{ops:?}");
    let EditOp::Crop { w, h, .. } = ops[0] else {
        panic!("{ops:?}");
    };
    assert!(((w * 64.0) / (h * 48.0) - 1.0).abs() < 0.02, "{w} {h}");
    assert!(with_view(cx, ed.handle, &ed.view, |v, _, _| matches!(
        v.editor_tool(),
        Some(Tool::None)
    )));

    ed.click(cx, ids::EDITOR_CROP);
    ed.click(cx, ids::TOOL_CANCEL);
    assert_eq!(ed.ops().len(), 1, "cancel stores nothing");
}

#[gpui_kit::test]
fn ui_before_after_toggle_shows_original(cx: &mut TestAppContext) {
    let ed = open_editor(cx);
    ed.click(cx, ids::EDITOR_LUCKY);
    let edited = ed.image(cx).unwrap();
    ed.click(cx, ids::EDITOR_BEFORE_AFTER);
    assert!(with_view(cx, ed.handle, &ed.view, |v, _, _| v
        .editor()
        .unwrap()
        .showing_before()));
    let original = ed
        .lib
        .app
        .edits()
        .preview_file(
            ed.id,
            &EditStack::new(),
            1280,
            &std::sync::atomic::AtomicBool::new(false),
        )
        .unwrap();
    assert_eq!(ed.image(cx).unwrap(), original.path);
    assert_eq!(
        ed.ops(),
        vec![EditOp::FeelingLucky],
        "the stack is untouched"
    );

    ed.click(cx, ids::EDITOR_BEFORE_AFTER);
    assert_eq!(ed.image(cx).unwrap(), edited);
}

#[gpui_kit::test]
fn ui_redeye_drag_adds_region(cx: &mut TestAppContext) {
    let ed = open_editor(cx);
    ed.click(cx, ids::EDITOR_REDEYE);
    let b = cx
        .update_window(ed.handle, |_, window, _| {
            window.find(ids::REDEYE_OVERLAY).bounds()
        })
        .unwrap();
    let at = |fx: f32, fy: f32| {
        point(
            b.origin.x + b.size.width * fx,
            b.origin.y + b.size.height * fy,
        )
    };
    cx.update_window(ed.handle, |_, window, cx| {
        mouse_down(window, at(0.3, 0.3), cx);
        mouse_drag_to(window, at(0.4, 0.45), cx);
        mouse_up(window, at(0.4, 0.45), cx);
    })
    .unwrap();
    settle(cx, ed.handle, &ed.view);
    cx.update_window(ed.handle, |_, window, _| {
        assert!(window.try_find(ids::redeye_region(0)).is_some());
    })
    .unwrap();
    ed.click(cx, ids::TOOL_APPLY);
    let ops = ed.ops();
    let [EditOp::RedEye { regions }] = ops.as_slice() else {
        panic!("{ops:?}");
    };
    assert_eq!(regions.len(), 1);
    let r = regions[0];
    assert!(
        (r.x - 0.3).abs() < 0.02 && (r.y - 0.3).abs() < 0.02,
        "{r:?}"
    );
    assert!(
        (r.w - 0.1).abs() < 0.02 && (r.h - 0.15).abs() < 0.02,
        "{r:?}"
    );
}

#[gpui_kit::test]
fn ui_straighten_slider_previews_then_applies(cx: &mut TestAppContext) {
    let ed = open_editor(cx);
    ed.click(cx, ids::EDITOR_STRAIGHTEN);
    let b = cx
        .update_window(ed.handle, |_, window, _| {
            window.find(ids::SLIDER_STRAIGHTEN).bounds()
        })
        .unwrap();
    let target = point(b.origin.x + b.size.width * 0.75, b.center().y);
    cx.update_window(ed.handle, |_, window, cx| {
        mouse_down(window, target, cx);
        mouse_up(window, target, cx);
    })
    .unwrap();
    settle(cx, ed.handle, &ed.view);
    assert!(ed.ops().is_empty());
    assert!(matches!(ed.draft(cx), Some(EditOp::Straighten { degrees }) if degrees > 10.0));
    ed.click(cx, ids::TOOL_APPLY);
    assert!(matches!(ed.ops().as_slice(), [EditOp::Straighten { degrees }] if *degrees > 10.0));
}
