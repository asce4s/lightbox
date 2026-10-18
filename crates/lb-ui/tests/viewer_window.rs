//! M13.9: the standalone viewer window (SPEC-VIEWER §V3.2, §V6.1).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::write_jpeg;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, TestAppContext, px};
use lb_app::{InstanceServer, Launch, forward_launch};
use lb_ui::{LightboxWindow, ids};
use std::path::PathBuf;
use std::time::Duration;

fn outside_jpegs(names: &[&str]) -> (tempfile::TempDir, Vec<PathBuf>) {
    let dir = tempfile::tempdir().unwrap();
    let mut paths = Vec::new();
    for (i, name) in names.iter().enumerate() {
        let path = dir.path().join(name);
        write_jpeg(&path, u8::try_from(i).unwrap_or(0).wrapping_add(1));
        paths.push(path);
    }
    (dir, paths)
}

fn open_standalone(
    cx: &mut TestAppContext,
    app: &lb_app::Lightbox,
    paths: Vec<PathBuf>,
    instance: InstanceServer,
) -> (gpui_kit::AnyWindowHandle, gpui_kit::Entity<LightboxWindow>) {
    cx.update(gpui_kit::init);
    let app = app.clone();
    cx.update(|cx| LightboxWindow::open_standalone_for_test(app, paths, instance, cx))
}

#[gpui_kit::test]
fn ui_viewer_window_opens_path_outside_library(cx: &mut TestAppContext) {
    let lib = common::basic_library();
    let (_dir, paths) = outside_jpegs(&["a.jpg", "b.jpg", "c.jpg"]);
    let before = lib.app.photos().all().unwrap().len();
    let server = InstanceServer::bind(&lib.app.paths().data).unwrap();
    let (h, v) = open_standalone(cx, &lib.app, vec![paths[0].clone()], server);
    lib.clock.advance(Duration::from_millis(300));
    cx.update_window(h, |_, window, cx| {
        v.update(cx, |view, cx| view.pump(cx));
        window.render_frame(cx);
        assert_eq!(v.read(cx).standalone_title(), "a.jpg — Lightbox");
        assert_eq!(
            window.bounds().size,
            gpui_kit::size(px(1920.), px(1080.)),
            "the viewer covers the primary display"
        );
        assert!(!window.is_fullscreen());
        let title = window.find(ids::VIEWER_TITLE);
        assert_eq!(title.label(), Some("a.jpg"));
        let counter = window.find(ids::VIEWER_COUNTER);
        assert_eq!(counter.label(), Some("1 / 3"));
        assert!(window.try_find(ids::VIEWER_ADD_FOLDER).is_some());
        assert!(window.try_find(ids::VIEWER_STAR).is_some());
        window.press("f", cx);
    })
    .unwrap();
    cx.update_window(h, |_, window, cx| {
        v.update(cx, |view, cx| view.pump(cx));
        window.render_frame(cx);
        assert!(window.is_fullscreen());
        window.press("right", cx);
    })
    .unwrap();
    lib.clock.advance(Duration::from_millis(300));
    cx.update_window(h, |_, window, cx| {
        v.update(cx, |view, cx| view.pump(cx));
        window.render_frame(cx);
        window.click(ids::VIEWER_STAR, cx);
        assert_eq!(window.find(ids::VIEWER_TITLE).label(), Some("b.jpg"));
        assert_eq!(window.find(ids::VIEWER_COUNTER).label(), Some("2 / 3"));
        assert!(
            v.read(cx)
                .viewer()
                .is_some_and(|vm| vm.current_photo().is_none())
        );
    })
    .unwrap();
    assert_eq!(lib.app.photos().all().unwrap().len(), before);
}

#[gpui_kit::test]
fn ui_viewer_window_replaces_navset_on_second_launch(cx: &mut TestAppContext) {
    let lib = common::basic_library();
    let (_first, first) = outside_jpegs(&["a.jpg", "b.jpg"]);
    let (_second, second) = outside_jpegs(&["z.jpg"]);
    let server = InstanceServer::bind(&lib.app.paths().data).unwrap();
    let (h, v) = open_standalone(cx, &lib.app, vec![first[0].clone()], server);
    lib.clock.advance(Duration::from_millis(300));
    cx.update_window(h, |_, window, cx| {
        v.update(cx, |view, cx| view.pump(cx));
        window.render_frame(cx);
        assert_eq!(window.find(ids::VIEWER_COUNTER).label(), Some("1 / 2"));
    })
    .unwrap();
    forward_launch(
        &lib.app.paths().data,
        &Launch::View {
            paths: vec![second[0].clone()],
            perf: false,
        },
    )
    .unwrap();
    lib.clock.advance(Duration::from_millis(300));
    cx.update_window(h, |_, window, cx| {
        v.update(cx, |view, cx| view.pump(cx));
        window.render_frame(cx);
        assert_eq!(v.read(cx).standalone_title(), "z.jpg — Lightbox");
        assert_eq!(window.find(ids::VIEWER_TITLE).label(), Some("z.jpg"));
        assert_eq!(window.find(ids::VIEWER_COUNTER).label(), Some("1 / 1"));
        assert!(window.try_find(ids::VIEWER_ADD_FOLDER).is_some());
    })
    .unwrap();
}
