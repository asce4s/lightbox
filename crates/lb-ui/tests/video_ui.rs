//! M10.2: videos in the grid.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AppContext as _, TestAppContext};
use lb_testkit::OpenAction;
use lb_ui::{Route, ids};

fn add_clip(lib: &UiLib, with_ffmpeg: bool) -> lb_core::PhotoId {
    let path = lib.photos.path().join("clip.mp4");
    if with_ffmpeg {
        let ok = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-f", "lavfi"])
            .args(["-i", "testsrc=duration=75:size=160x120:rate=1"])
            .args(["-pix_fmt", "yuv420p", "-g", "1"])
            .arg(&path)
            .status()
            .is_ok_and(|s| s.success());
        assert!(ok, "ffmpeg could not make the clip");
    } else {
        std::fs::write(&path, b"\0\0\0\x18ftypisom\0\0\0\0isomiso2junk").unwrap();
    }
    let root = lib.app.library().roots().unwrap()[0].id;
    lib.app.library().rescan(root).unwrap();
    lib.app
        .photos()
        .all()
        .unwrap()
        .into_iter()
        .find(|p| p.path.ends_with("clip.mp4"))
        .unwrap()
        .id
}

#[gpui_kit::test]
fn ui_opening_a_video_uses_the_external_player(cx: &mut TestAppContext) {
    let lib = basic_library();
    let id = add_clip(&lib, false);
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    with_view(cx, handle, &view, |v, _, cx| v.open_photo(id, cx));
    settle(cx, handle, &view);
    with_view(cx, handle, &view, |v, _, _| {
        assert_eq!(v.route(), Route::Library, "no in-app playback");
    });
    let calls = lib.opener.calls();
    assert_eq!(calls.len(), 1);
    assert_eq!(calls[0].0, OpenAction::Open);
    assert!(calls[0].1.ends_with("clip.mp4"));
}

#[gpui_kit::test]
fn ui_video_cell_shows_duration_badge(cx: &mut TestAppContext) {
    if !lb_video_available() {
        eprintln!("ffmpeg not found; skipping");
        return;
    }
    let lib = basic_library();
    let id = add_clip(&lib, true);
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);
    cx.update_window(handle, |_, window, cx| {
        window.render_frame(cx);
        assert!(window.try_find(ids::video_badge(id)).is_some());
        assert!(
            window.try_find(ids::video_badge(lib.ids[0])).is_none(),
            "photos get no badge"
        );
    })
    .unwrap();
}

fn lb_video_available() -> bool {
    std::process::Command::new("ffprobe")
        .arg("-version")
        .output()
        .is_ok_and(|o| o.status.success())
}
