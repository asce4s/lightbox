//! M10.2: videos in the library (SPEC §6.16).
#![cfg(feature = "video")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

use lb_app::{Lightbox, LightboxOptions};
use lb_core::{MediaType, RootMode};
use lb_jobs::JobQueue;
use lb_thumbs::ThumbSize;
use std::path::Path;

fn make_clip(path: &Path) -> bool {
    std::process::Command::new("ffmpeg")
        .args(["-v", "error", "-y", "-f", "lavfi"])
        .args(["-i", "testsrc=duration=2:size=320x240:rate=10"])
        .args(["-pix_fmt", "yuv420p"])
        .args(["-metadata", "creation_time=2021-06-15T10:20:30Z"])
        .arg(path)
        .status()
        .is_ok_and(|s| s.success())
}

#[test]
fn video_integration_ffmpeg() {
    if lb_video::Tools::detect(None).is_none() {
        eprintln!("ffmpeg not found; skipping");
        return;
    }
    let home = tempfile::tempdir().unwrap();
    let photos = tempfile::tempdir().unwrap();
    assert!(make_clip(&photos.path().join("clip.mp4")));
    let lb =
        Lightbox::open_with(home.path(), JobQueue::inline(), LightboxOptions::default()).unwrap();
    lb.library()
        .add_root(photos.path(), RootMode::Watch)
        .unwrap();
    let all = lb.photos().all().unwrap();
    assert_eq!(all.len(), 1);
    let video = &all[0];
    assert_eq!(video.media_type, MediaType::Video);
    assert_eq!((video.width, video.height), (Some(320), Some(240)));
    assert!((1900..=2100).contains(&video.duration_ms.unwrap()));
    assert_eq!(video.status, lb_core::PhotoStatus::Ok);
    assert_eq!(
        video.taken_at_src,
        Some(lb_core::TakenAtSource::Video),
        "the container creation time is the capture time"
    );

    let thumb = lb.thumbs().generate(video.id, ThumbSize::M).unwrap();
    let img = image::open(&thumb).unwrap();
    assert!(img.width() > 16 && img.height() > 16);

    let mut view = lb_app::ViewQuery::new(lb_app::ViewSource::AllPhotos);
    view.chips.video = true;
    assert_eq!(lb.search().query(&view).unwrap().total, 1);
}

#[test]
fn videos_without_ffmpeg_are_indexed_from_file_data() {
    let home = tempfile::tempdir().unwrap();
    let photos = tempfile::tempdir().unwrap();
    std::fs::write(
        photos.path().join("VID_20200102_030405.mp4"),
        b"\0\0\0\x18ftypisom\0\0\0\0isomiso2not really a video",
    )
    .unwrap();
    let missing = home.path().join("no-such-ffmpeg");
    let lb =
        Lightbox::open_with(home.path(), JobQueue::inline(), LightboxOptions::default()).unwrap();
    let mut settings = lb.settings().load().unwrap();
    settings.video.ffmpeg_path = Some(missing.display().to_string());
    lb.settings().save(&settings).unwrap();
    drop(lb);
    let lb =
        Lightbox::open_with(home.path(), JobQueue::inline(), LightboxOptions::default()).unwrap();
    lb.library()
        .add_root(photos.path(), RootMode::Watch)
        .unwrap();
    let all = lb.photos().all().unwrap();
    assert_eq!(all.len(), 1, "the video is listed");
    assert_eq!(all[0].media_type, MediaType::Video);
    assert_eq!(all[0].width, None);
    assert_eq!(all[0].duration_ms, None);
    assert_eq!(all[0].status, lb_core::PhotoStatus::Ok);
    assert_eq!(
        all[0].taken_at.map(|d| d.to_string()),
        Some("2020-01-02 03:04:05".to_owned()),
        "the file name still dates it"
    );
    assert!(
        lb.thumbs().generate(all[0].id, ThumbSize::S).is_err(),
        "no thumbnail without ffmpeg, but no crash either"
    );
}
