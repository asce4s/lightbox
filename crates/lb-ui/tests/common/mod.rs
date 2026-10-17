//! Shared fixtures for UI tests: a generated library and window helpers.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use gpui_kit::AppContext as _;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{
    AnyWindowHandle, App, ElementId, Entity, InputEvent as _, Modifiers, MouseButton,
    MouseDownEvent, MouseUpEvent, TestAppContext, Window,
};
use lb_app::{Lightbox, LightboxOptions};
use lb_core::{PhotoId, RootMode};
use lb_testkit::{FakeClock, FakeOpener, FakeTrash};
use lb_ui::LightboxWindow;
use std::path::{Path, PathBuf};
use std::sync::Arc;

/// An open library over generated JPEGs, driven by a fake clock.
pub struct UiLib {
    pub home: tempfile::TempDir,
    pub photos: tempfile::TempDir,
    pub app: Lightbox,
    pub clock: Arc<FakeClock>,
    pub trash: Arc<FakeTrash>,
    pub opener: Arc<FakeOpener>,
    pub trash_dir: tempfile::TempDir,
    /// Ids in the order of `files`.
    pub ids: Vec<PhotoId>,
    pub files: Vec<PathBuf>,
}

pub fn write_jpeg(path: &Path, seed: u8) {
    let img = image::RgbImage::from_fn(64, 48, |x, y| {
        image::Rgb([
            (x * 4) as u8,
            (y * 5) as u8,
            seed.wrapping_mul(37).wrapping_add((x + y) as u8),
        ])
    });
    img.save(path).unwrap();
}

/// `spec` lists `(folder, file names)`; names like `IMG_20210615_120000.jpg` fix the capture date.
pub fn ui_library(spec: &[(&str, &[&str])]) -> UiLib {
    let home = tempfile::tempdir().unwrap();
    let photos = tempfile::tempdir().unwrap();
    let trash_dir = tempfile::tempdir().unwrap();
    let mut files = Vec::new();
    let mut seed = 0u8;
    for (dir, names) in spec {
        let dir = photos.path().join(dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in *names {
            seed = seed.wrapping_add(1);
            write_jpeg(&dir.join(name), seed);
            files.push(dir.join(name));
        }
    }
    let clock = Arc::new(FakeClock::new());
    let trash = Arc::new(FakeTrash::new(trash_dir.path().join("trash")));
    let opener = Arc::new(FakeOpener::new());
    let options = LightboxOptions {
        clock: clock.clone(),
        spawn_event_ticker: false,
        trash: trash.clone(),
        opener: opener.clone(),
        ..LightboxOptions::default()
    };
    let app = Lightbox::open_with(home.path(), lb_app::JobQueue::inline(), options).unwrap();
    app.library()
        .add_root(photos.path(), RootMode::Watch)
        .unwrap();
    let all = app.photos().all().unwrap();
    let ids = files
        .iter()
        .map(|f| all.iter().find(|p| &p.path == f).unwrap().id)
        .collect();
    UiLib {
        home,
        photos,
        app,
        clock,
        trash,
        opener,
        trash_dir,
        ids,
        files,
    }
}

/// Two folders, three photos in June 2021 and two in July 2021.
pub fn basic_library() -> UiLib {
    ui_library(&[
        (
            "june",
            &[
                "IMG_20210601_100000.jpg",
                "IMG_20210602_100000.jpg",
                "IMG_20210603_100000.jpg",
            ],
        ),
        (
            "july",
            &["IMG_20210701_100000.jpg", "IMG_20210702_100000.jpg"],
        ),
    ])
}

pub fn open(cx: &mut TestAppContext, lib: &UiLib) -> (AnyWindowHandle, Entity<LightboxWindow>) {
    cx.update(gpui_kit::init);
    let app = lib.app.clone();
    cx.update(|cx| LightboxWindow::open_for_test(app, cx))
}

/// Runs `f` against the window and its view, then lets queued work finish.
pub fn with_view<R>(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &Entity<LightboxWindow>,
    f: impl FnOnce(&mut LightboxWindow, &mut Window, &mut gpui_kit::Context<LightboxWindow>) -> R,
) -> R {
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| f(v, window, cx))
    })
    .unwrap()
}

/// Pumps application events into the view and renders a frame.
pub fn settle(cx: &mut TestAppContext, handle: AnyWindowHandle, view: &Entity<LightboxWindow>) {
    cx.update_window(handle, |_, window, cx| {
        view.update(cx, |v, cx| v.pump(cx));
        window.render_frame(cx);
    })
    .unwrap();
    cx.run_until_parked();
}

/// A left click on `id` with keyboard modifiers held.
pub fn click_with(
    window: &mut Window,
    id: impl Into<ElementId>,
    modifiers: Modifiers,
    cx: &mut App,
) {
    window.render_frame(cx);
    let position = window.find(id).bounds().center();
    window.dispatch_event(
        MouseDownEvent {
            button: MouseButton::Left,
            position,
            modifiers,
            click_count: 1,
            first_mouse: false,
        }
        .to_platform_input(),
        cx,
    );
    window.dispatch_event(
        MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers,
            click_count: 1,
        }
        .to_platform_input(),
        cx,
    );
    window.render_frame(cx);
}

pub const SHIFT: Modifiers = Modifiers {
    control: false,
    alt: false,
    shift: true,
    platform: false,
    function: false,
};

pub const CTRL: Modifiers = Modifiers {
    control: true,
    alt: false,
    shift: false,
    platform: false,
    function: false,
};

/// Presses the left button at `position` without releasing it.
pub fn mouse_down(window: &mut Window, position: gpui_kit::Point<gpui_kit::Pixels>, cx: &mut App) {
    window.render_frame(cx);
    window.dispatch_event(
        MouseDownEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
            first_mouse: false,
        }
        .to_platform_input(),
        cx,
    );
}

/// Moves the pointer with the left button held.
pub fn mouse_drag_to(
    window: &mut Window,
    position: gpui_kit::Point<gpui_kit::Pixels>,
    cx: &mut App,
) {
    window.dispatch_event(
        gpui_kit::MouseMoveEvent {
            position,
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::default(),
        }
        .to_platform_input(),
        cx,
    );
}

/// Releases the left button at `position`.
pub fn mouse_up(window: &mut Window, position: gpui_kit::Point<gpui_kit::Pixels>, cx: &mut App) {
    window.dispatch_event(
        MouseUpEvent {
            button: MouseButton::Left,
            position,
            modifiers: Modifiers::default(),
            click_count: 1,
        }
        .to_platform_input(),
        cx,
    );
    window.render_frame(cx);
}
