//! Launch grammar, viewer-only start and macOS URL routing (SPEC-VIEWER §V2).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::write_jpeg;
use lb_app::{
    Launch, MACOS_OPEN_URL_WAIT, MacosDecision, Platform, apply_startup, macos_route,
    open_library_from_viewer, parse_launch,
};
use std::path::PathBuf;
use std::time::Duration;

#[test]
fn launch_args_parse_table() {
    let cwd = PathBuf::from("/work");
    let rows = [
        (vec!["lightbox"], Launch::Library),
        (vec!["lightbox", "--library"], Launch::Library),
        (vec!["lightbox", "-psn_0_192837"], Launch::Library),
        (vec!["lightbox", "-psn_0_1", "--library"], Launch::Library),
        (
            vec!["lightbox", "--unregister-file-types"],
            Launch::UnregisterFileTypes,
        ),
        (
            vec!["lightbox", "view", "pics/a.jpg"],
            Launch::View {
                paths: vec![cwd.join("pics/a.jpg")],
                perf: false,
            },
        ),
        (
            vec!["lightbox", "view", "--perf-json", "pics/a.jpg"],
            Launch::View {
                paths: vec![cwd.join("pics/a.jpg")],
                perf: true,
            },
        ),
        (
            vec!["lightbox", "view", "/tmp/album"],
            Launch::View {
                paths: vec![PathBuf::from("/tmp/album")],
                perf: false,
            },
        ),
        (
            vec!["lightbox", "file:///tmp/a%20b.jpg"],
            Launch::View {
                paths: vec![PathBuf::from("/tmp/a b.jpg")],
                perf: false,
            },
        ),
        (
            vec!["lightbox", "view", "file:///tmp/My%20Photos/a.jpg"],
            Launch::View {
                paths: vec![PathBuf::from("/tmp/My Photos/a.jpg")],
                perf: false,
            },
        ),
        (
            vec!["lightbox", "missing.jpg"],
            Launch::View {
                paths: vec![cwd.join("missing.jpg")],
                perf: false,
            },
        ),
        (
            vec!["lightbox", "/tmp/notes.txt"],
            Launch::View {
                paths: vec![PathBuf::from("/tmp/notes.txt")],
                perf: false,
            },
        ),
    ];
    for (args, expected) in rows {
        assert_eq!(
            parse_launch(&args, &cwd, Platform::Macos),
            expected,
            "args {args:?}"
        );
        assert_eq!(
            parse_launch(&args, &cwd, Platform::Other),
            expected,
            "args {args:?}"
        );
    }
}

#[test]
fn view_launch_does_not_scan_or_watch() {
    let t = common::library(1);
    let extra = t.photos.path().join("p99.jpg");
    write_jpeg(&extra, 9);
    let before = t.lb.photos().all().unwrap().len();
    apply_startup(
        &t.lb,
        &Launch::View {
            paths: vec![extra.clone()],
            perf: false,
        },
    )
    .unwrap();
    assert!(!t.lb.library().is_watching());
    assert_eq!(t.lb.photos().all().unwrap().len(), before);
    assert!(t.lb.viewer().find_by_path(&extra).unwrap().is_none());
}

#[test]
fn open_library_from_viewer_starts_scan() {
    let t = common::library(1);
    let extra = t.photos.path().join("p99.jpg");
    write_jpeg(&extra, 9);
    assert!(!t.lb.library().is_watching());
    open_library_from_viewer(&t.lb).unwrap();
    assert!(t.lb.library().is_watching());
    assert!(t.lb.viewer().find_by_path(&extra).unwrap().is_some());
}

#[test]
fn macos_open_urls_route_to_viewer() {
    let library = Launch::Library;
    let early = macos_route(
        &library,
        &["file:///tmp/a%20b.jpg".to_owned()],
        Duration::from_millis(10),
        false,
    );
    assert_eq!(
        early,
        MacosDecision::Ready(Launch::View {
            paths: vec![PathBuf::from("/tmp/a b.jpg")],
            perf: false,
        })
    );
    assert_eq!(
        macos_route(&library, &[], Duration::ZERO, false),
        MacosDecision::Wait
    );
    assert_eq!(
        macos_route(&library, &[], MACOS_OPEN_URL_WAIT, false),
        MacosDecision::Ready(Launch::Library)
    );
    assert!(matches!(
        macos_route(
            &library,
            &["file:///tmp/c.jpg".to_owned()],
            MACOS_OPEN_URL_WAIT,
            false,
        ),
        MacosDecision::Ready(Launch::View { .. })
    ));
    assert_eq!(
        macos_route(
            &library,
            &["file:///tmp/late.jpg".to_owned()],
            Duration::from_millis(251),
            false,
        ),
        MacosDecision::Ready(Launch::Library)
    );
    let argv = Launch::View {
        paths: vec![PathBuf::from("/already.jpg")],
        perf: false,
    };
    assert_eq!(
        macos_route(
            &argv,
            &["file:///tmp/other.jpg".to_owned()],
            Duration::ZERO,
            false,
        ),
        MacosDecision::Ready(argv)
    );
    assert_eq!(
        macos_route(&library, &[], Duration::ZERO, true),
        MacosDecision::Ready(Launch::Library)
    );
}
