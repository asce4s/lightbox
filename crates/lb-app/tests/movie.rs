//! M12.1: slideshow movies (SPEC §6.12.3).
#![cfg(feature = "video")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use lb_app::MovieSettings;
use std::cell::Cell;
use std::path::Path;

fn ffmpeg_present() -> bool {
    let found = lb_video::Tools::detect(None).is_some();
    if !found {
        eprintln!("ffmpeg not found; skipping");
    }
    found
}

fn small() -> MovieSettings {
    MovieSettings {
        fps: 10,
        width: 320,
        height: 180,
        seconds_per_photo: 0.5,
        audio: None,
    }
}

fn probe(path: &Path) -> (u32, u32, f64) {
    let out = std::process::Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0"])
        .args([
            "-show_entries",
            "stream=width,height,codec_name:format=duration",
        ])
        .args(["-of", "default=nw=1"])
        .arg(path)
        .output()
        .unwrap();
    let text = String::from_utf8_lossy(&out.stdout).to_string();
    assert!(text.contains("codec_name=h264"), "{text}");
    let value = |key: &str| -> String {
        text.lines()
            .find_map(|l| l.strip_prefix(&format!("{key}=")))
            .unwrap()
            .to_string()
    };
    (
        value("width").parse().unwrap(),
        value("height").parse().unwrap(),
        value("duration").parse().unwrap(),
    )
}

fn nothing(_: f32, _: &str) {}

#[test]
fn movie_renders_photos_in_order_to_mp4() {
    if !ffmpeg_present() {
        return;
    }
    let t = common::library(3);
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("show.mp4");
    let ticks = Cell::new(0usize);
    let report =
        t.lb.movie()
            .render(&t.ids, &small(), &out, &|| false, &|_, _| {
                ticks.set(ticks.get() + 1);
            })
            .unwrap();
    assert_eq!(report.photos, 3);
    assert!(report.skipped.is_empty() && !report.cancelled);
    assert_eq!(report.path.as_deref(), Some(out.as_path()));
    assert_eq!(ticks.get(), 3);
    let (w, h, secs) = probe(&out);
    assert_eq!((w, h), (320, 180));
    assert!((1.2..=1.8).contains(&secs), "{secs}");
    assert!((report.duration_secs - 1.5).abs() < 1e-9);
    let leftovers: Vec<_> = std::fs::read_dir(out_dir.path())
        .unwrap()
        .map(|e| e.unwrap().file_name())
        .collect();
    assert_eq!(leftovers.len(), 1, "{leftovers:?}");
}

#[test]
fn movie_cancel_leaves_no_file() {
    if !ffmpeg_present() {
        return;
    }
    let t = common::library(3);
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("show.mp4");
    let polls = Cell::new(0usize);
    let report =
        t.lb.movie()
            .render(
                &t.ids,
                &small(),
                &out,
                &|| {
                    polls.set(polls.get() + 1);
                    polls.get() > 1
                },
                &nothing,
            )
            .unwrap();
    assert!(report.cancelled);
    assert_eq!(report.path, None);
    assert_eq!(std::fs::read_dir(out_dir.path()).unwrap().count(), 0);
}

#[test]
fn movie_skips_unreadable_photos_and_rejects_empty_lists() {
    if !ffmpeg_present() {
        return;
    }
    let t = common::library(2);
    let out_dir = tempfile::tempdir().unwrap();
    let out = out_dir.path().join("show.mp4");
    let movie = t.lb.movie();
    assert!(
        movie
            .render(&[], &small(), &out, &|| false, &nothing)
            .is_err()
    );

    let missing = lb_core::PhotoId::new(9999);
    let ids = [t.ids[0], missing, t.ids[1]];
    let report = movie
        .render(&ids, &small(), &out, &|| false, &nothing)
        .unwrap();
    assert_eq!(report.photos, 2);
    assert_eq!(report.skipped, vec![missing]);

    let only_missing = movie.render(
        &[missing],
        &small(),
        &out_dir.path().join("x.mp4"),
        &|| false,
        &nothing,
    );
    assert!(only_missing.is_err());
    assert!(!out_dir.path().join("x.mp4").exists());
}
