//! M9.1: theme switching in the window.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, TestAppContext};
use lb_app::ThemeChoice;
use lb_ui::ids;

fn is_dark(cx: &mut TestAppContext) -> bool {
    cx.update(|cx| cx.theme().is_dark())
}

#[gpui_kit::test]
fn ui_theme_toggle(cx: &mut TestAppContext) {
    let lib = basic_library();
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    assert_eq!(
        with_view(cx, handle, &view, |v, _, _| v.theme()),
        ThemeChoice::System
    );

    cx.update_window(handle, |_, window, cx| window.click(ids::THEME_TOGGLE, cx))
        .unwrap();
    settle(cx, handle, &view);
    assert_eq!(
        with_view(cx, handle, &view, |v, _, _| v.theme()),
        ThemeChoice::Light
    );
    assert!(!is_dark(cx));

    cx.update_window(handle, |_, window, cx| window.click(ids::THEME_TOGGLE, cx))
        .unwrap();
    settle(cx, handle, &view);
    assert_eq!(
        with_view(cx, handle, &view, |v, _, _| v.theme()),
        ThemeChoice::Dark
    );
    assert!(is_dark(cx));
    assert_eq!(
        lib.app.settings().load().unwrap().theme,
        ThemeChoice::Dark,
        "stored"
    );

    cx.update_window(handle, |_, window, cx| window.click(ids::THEME_TOGGLE, cx))
        .unwrap();
    settle(cx, handle, &view);
    assert_eq!(
        with_view(cx, handle, &view, |v, _, _| v.theme()),
        ThemeChoice::System
    );

    // a change made elsewhere (CLI, another window) reaches this window through the event bus
    lib.app.settings().set_theme(ThemeChoice::Dark).unwrap();
    lib.app.bus().tick();
    settle(cx, handle, &view);
    assert_eq!(
        with_view(cx, handle, &view, |v, _, _| v.theme()),
        ThemeChoice::Dark
    );
    assert!(is_dark(cx));
}
