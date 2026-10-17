//! Movie maker (SPEC §6.12.3): the ffmpeg pipeline, with a fake process runner.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use image::{DynamicImage, Rgb, RgbImage};
use lb_video::{
    CommandOutput, CommandRunner, CommandSpec, MovieSettings, Tools, VideoError, VideoTools,
};
use parking_lot::Mutex;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Debug, Default)]
struct Recorder {
    spec: Mutex<Option<CommandSpec>>,
    input: Mutex<Vec<u8>>,
    fail_with: Option<&'static str>,
}

impl CommandRunner for Recorder {
    fn run(&self, _spec: &CommandSpec) -> std::io::Result<CommandOutput> {
        Err(std::io::Error::other("only piped runs expected"))
    }

    fn run_with_input(
        &self,
        spec: &CommandSpec,
        feed: &mut dyn FnMut(&mut dyn Write) -> std::io::Result<()>,
    ) -> std::io::Result<CommandOutput> {
        *self.spec.lock() = Some(spec.clone());
        feed(&mut *self.input.lock())?;
        Ok(CommandOutput {
            success: self.fail_with.is_none(),
            stdout: vec![],
            stderr: self.fail_with.unwrap_or_default().as_bytes().to_vec(),
        })
    }
}

fn tools() -> Tools {
    Tools {
        ffmpeg: PathBuf::from("/opt/ff/ffmpeg"),
        ffprobe: PathBuf::from("/opt/ff/ffprobe"),
    }
}

fn args(spec: &CommandSpec) -> Vec<&str> {
    spec.args.iter().map(|a| a.to_str().unwrap()).collect()
}

fn photo(color: [u8; 3]) -> DynamicImage {
    DynamicImage::ImageRgb8(RgbImage::from_pixel(64, 36, Rgb(color)))
}

#[test]
fn movie_maker_builds_expected_ffmpeg_pipeline() {
    let settings = MovieSettings {
        fps: 30,
        width: 1920,
        height: 1080,
        seconds_per_photo: 3.0,
        audio: None,
    };
    let spec = tools().movie_command(Path::new("/out/show.mp4"), &settings);
    assert_eq!(spec.program, PathBuf::from("/opt/ff/ffmpeg"));
    assert_eq!(
        args(&spec),
        [
            "-v",
            "error",
            "-y",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgb24",
            "-s",
            "1920x1080",
            "-framerate",
            "30",
            "-i",
            "pipe:0",
            "-c:v",
            "libx264",
            "-pix_fmt",
            "yuv420p",
            "-r",
            "30",
            "-movflags",
            "+faststart",
            "-f",
            "mp4",
            "/out/show.mp4"
        ]
    );

    let with_audio = MovieSettings {
        audio: Some(PathBuf::from("/music/song one.mp3")),
        ..settings
    };
    let spec = tools().movie_command(Path::new("/out/show.mp4"), &with_audio);
    let a = args(&spec);
    let at = |needle: &str| a.iter().position(|x| *x == needle).unwrap();
    assert_eq!(a[at("-i") + 1], "pipe:0");
    assert!(a.contains(&"/music/song one.mp3"));
    assert!(a.windows(2).any(|w| w == ["-c:a", "aac"]));
    assert!(a.contains(&"-shortest"));
    assert!(
        a.windows(4)
            .any(|w| w == ["-map", "0:v:0", "-map", "1:a:0"])
    );
    assert!(
        at("-shortest") < at("-c:v"),
        "audio options precede the video codec"
    );
}

#[test]
fn movie_maker_pipes_one_frame_per_tick() {
    let runner = Arc::new(Recorder::default());
    let video = VideoTools::with_runner(tools(), runner.clone());
    let settings = MovieSettings {
        fps: 10,
        width: 64,
        height: 36,
        seconds_per_photo: 0.5,
        audio: None,
    };
    let mut photos = vec![photo([255, 0, 0]), photo([0, 255, 0]), photo([0, 0, 255])].into_iter();
    let written = video
        .make_movie(Path::new("/out/a.mp4"), &settings, &mut photos)
        .unwrap();
    assert_eq!(written, 3);
    let input = runner.input.lock();
    let frame = settings.frame_len();
    assert_eq!(frame, 64 * 36 * 3);
    assert_eq!(
        input.len(),
        3 * 5 * frame,
        "3 photos x 5 frames of raw RGB24"
    );
    assert_eq!(&input[..3], [255, 0, 0]);
    assert_eq!(&input[5 * frame..5 * frame + 3], [0, 255, 0]);
    assert_eq!(&input[10 * frame..10 * frame + 3], [0, 0, 255]);
    let spec = runner.spec.lock().clone().unwrap();
    assert!(args(&spec).contains(&"64x36"));
}

#[test]
fn movie_maker_reports_ffmpeg_errors() {
    let runner = Arc::new(Recorder {
        fail_with: Some("Unknown encoder 'libx264'"),
        ..Recorder::default()
    });
    let video = VideoTools::with_runner(tools(), runner);
    let mut photos = vec![photo([1, 2, 3])].into_iter();
    let err = video
        .make_movie(
            Path::new("/out/a.mp4"),
            &MovieSettings::default(),
            &mut photos,
        )
        .unwrap_err();
    assert!(
        matches!(&err, VideoError::Tool { tool: "ffmpeg", message } if message.contains("libx264"))
    );
}

#[test]
fn movie_maker_with_a_runner_that_cannot_pipe_fails_cleanly() {
    #[derive(Debug)]
    struct NoPipe;
    impl CommandRunner for NoPipe {
        fn run(&self, _: &CommandSpec) -> std::io::Result<CommandOutput> {
            Err(std::io::Error::other("no"))
        }
    }
    let video = VideoTools::with_runner(tools(), Arc::new(NoPipe));
    let err = video
        .make_movie(
            Path::new("/out/a.mp4"),
            &MovieSettings::default(),
            &mut std::iter::empty(),
        )
        .unwrap_err();
    assert!(matches!(err, VideoError::Io(_)));
}
