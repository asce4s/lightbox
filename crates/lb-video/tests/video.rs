//! Video metadata and thumbnails (SPEC §6.16).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use lb_core::GeoPoint;
use lb_media::{Decoder, VideoProber};
use lb_video::{
    CommandOutput, CommandRunner, CommandSpec, Tools, VideoDecoder, VideoTools, parse_ffprobe,
    parse_iso6709, thumbnail_time,
};
use parking_lot::Mutex;
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use std::sync::Arc;

const PHONE_JSON: &str = r#"{
  "streams": [
    {"index": 0, "codec_name": "h264", "codec_type": "video", "width": 1920, "height": 1080,
     "duration": "12.480000", "tags": {"creation_time": "2021-06-15T10:20:30.000000Z"},
     "side_data_list": [{"side_data_type": "Display Matrix", "rotation": -90}]},
    {"index": 1, "codec_name": "aac", "codec_type": "audio", "duration": "12.480000"}
  ],
  "format": {"filename": "x.mov", "duration": "12.500000",
    "tags": {"creation_time": "2021-06-15T10:20:30.000000Z",
             "com.apple.quicktime.location.ISO6709": "+37.7749-122.4194+012.500/"}}
}"#;

#[test]
fn ffprobe_json_parsed() {
    let info = parse_ffprobe(PHONE_JSON).unwrap();
    assert_eq!((info.width, info.height), (1920, 1080));
    assert_eq!(info.duration_ms, 12_500, "the container duration wins");
    assert_eq!(info.rotation, 90);
    assert_eq!(
        info.creation_time_utc.unwrap().to_string(),
        "2021-06-15 10:20:30"
    );
    assert_eq!(
        info.location,
        Some(GeoPoint {
            lat: 37.7749,
            lon: -122.4194
        })
    );

    let plain = r#"{"streams":[{"codec_type":"video","width":640,"height":360,"duration":"3.5"}],"format":{}}"#;
    let info = parse_ffprobe(plain).unwrap();
    assert_eq!(info.duration_ms, 3500, "falls back to the stream duration");
    assert!(info.creation_time_utc.is_none() && info.location.is_none());
    assert_eq!(info.rotation, 0);

    for bad in [
        "not json",
        "{}",
        r#"{"streams":[{"codec_type":"audio"}]}"#,
        r#"{"streams":[{"codec_type":"video","width":0,"height":10}]}"#,
        r#"{"streams":[{"codec_type":"video","width":10,"height":10,"disposition":{"attached_pic":1}}]}"#,
    ] {
        assert!(parse_ffprobe(bad).is_err(), "{bad}");
    }
}

#[test]
fn video_rotation_swaps_dimensions() {
    let with = |rot: &str| {
        let json = format!(
            r#"{{"streams":[{{"codec_type":"video","width":1920,"height":1080,{rot}}}],"format":{{"duration":"1"}}}}"#
        );
        parse_ffprobe(&json).unwrap()
    };
    let cases = [
        (r#""tags":{"rotate":"90"}"#, 90, (1080, 1920)),
        (r#""tags":{"rotate":"270"}"#, 270, (1080, 1920)),
        (r#""tags":{"rotate":"180"}"#, 180, (1920, 1080)),
        (r#""tags":{"rotate":"0"}"#, 0, (1920, 1080)),
        (r#""side_data_list":[{"rotation":-90}]"#, 90, (1080, 1920)),
        (r#""side_data_list":[{"rotation":90}]"#, 270, (1080, 1920)),
        (r#""side_data_list":[{"rotation":180}]"#, 180, (1920, 1080)),
        (
            r#""side_data_list":[{"side_data_type":"Other"}]"#,
            0,
            (1920, 1080),
        ),
    ];
    for (rot, degrees, size) in cases {
        let info = with(rot);
        assert_eq!(info.rotation, degrees, "{rot}");
        assert_eq!(info.display_size(), size, "{rot}");
    }
}

#[test]
fn video_iso6709_location_parsed() {
    let cases: &[(&str, Option<(f64, f64)>)] = &[
        ("+37.7749-122.4194/", Some((37.7749, -122.4194))),
        ("+37.7749-122.4194+012.500/", Some((37.7749, -122.4194))),
        ("-33.8688+151.2093CRSWGS_84/", Some((-33.8688, 151.2093))),
        (
            "+4852.000+00221.000/",
            Some((48.0 + 52.0 / 60.0, 2.0 + 21.0 / 60.0)),
        ),
        (
            "+485230-0022100/",
            Some((48.0 + 52.0 / 60.0 + 30.0 / 3600.0, -(2.0 + 21.0 / 60.0))),
        ),
        ("+00.0000+000.0000/", Some((0.0, 0.0))),
        ("+95.0000+010.0000/", None),
        ("+10.0+200.0/", None),
        ("+10.0/", None),
        ("garbage", None),
        ("", None),
    ];
    for (text, want) in cases {
        let got = parse_iso6709(text).map(|p| (p.lat, p.lon));
        match (got, want) {
            (Some(g), Some(w)) => assert!(
                (g.0 - w.0).abs() < 1e-6 && (g.1 - w.1).abs() < 1e-6,
                "{text}: {g:?} vs {w:?}"
            ),
            (g, w) => assert_eq!(g, *w, "{text}"),
        }
    }
}

#[derive(Debug, Default)]
struct FakeRunner {
    calls: Mutex<Vec<CommandSpec>>,
    probe_json: String,
    frame_fails_at: Option<String>,
}

impl CommandRunner for FakeRunner {
    fn run(&self, spec: &CommandSpec) -> std::io::Result<CommandOutput> {
        self.calls.lock().push(spec.clone());
        let ok = |stdout: Vec<u8>| CommandOutput {
            success: true,
            stdout,
            stderr: vec![],
        };
        if spec.program.ends_with("ffprobe") {
            return Ok(ok(self.probe_json.clone().into_bytes()));
        }
        let ss = spec
            .args
            .iter()
            .position(|a| a == "-ss")
            .and_then(|i| spec.args.get(i + 1))
            .and_then(|a| a.to_str())
            .unwrap_or("");
        if self.frame_fails_at.as_deref() == Some(ss) {
            return Ok(CommandOutput {
                success: false,
                stdout: vec![],
                stderr: b"Output file is empty".to_vec(),
            });
        }
        let img = image::RgbImage::from_pixel(64, 36, image::Rgb([10, 200, 30]));
        let mut png = Vec::new();
        img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
            .unwrap();
        Ok(ok(png))
    }
}

fn tools() -> Tools {
    Tools {
        ffmpeg: PathBuf::from("/opt/ff/ffmpeg"),
        ffprobe: PathBuf::from("/opt/ff/ffprobe"),
    }
}

#[test]
fn video_thumbnail_command_built_correctly() {
    let spec = tools().thumbnail_command(Path::new("/v/clip one.mp4"), 1.25, 320);
    assert_eq!(spec.program, PathBuf::from("/opt/ff/ffmpeg"));
    let args: Vec<&str> = spec.args.iter().map(|a| a.to_str().unwrap()).collect();
    let pos = |needle: &str| args.iter().position(|a| *a == needle).unwrap();
    assert!(pos("-ss") < pos("-i"), "seek before the input: {args:?}");
    assert_eq!(args[pos("-ss") + 1], "1.250");
    assert_eq!(
        args[pos("-i") + 1],
        "/v/clip one.mp4",
        "one argument, no shell"
    );
    assert_eq!(args[pos("-frames:v") + 1], "1");
    assert_eq!(
        args[pos("-vf") + 1],
        "scale=320:320:force_original_aspect_ratio=decrease"
    );
    assert_eq!(args.last(), Some(&"pipe:1"));
    assert!(args.contains(&"png") && args.contains(&"image2pipe"));

    let probe = tools().probe_command(Path::new("/v/a.mov"));
    let args: Vec<OsString> = probe.args;
    assert_eq!(args.last().unwrap(), "/v/a.mov");
    for flag in ["-print_format", "json", "-show_format", "-show_streams"] {
        assert!(args.iter().any(|a| a == flag), "{flag}");
    }
    assert!(
        (thumbnail_time(20_000) - 2.0).abs() < 1e-9,
        "10 % of the duration"
    );
    assert_eq!(thumbnail_time(0), 0.0);
}

#[test]
fn video_frame_seeks_to_ten_percent_then_falls_back() {
    let runner = Arc::new(FakeRunner {
        probe_json: PHONE_JSON.into(),
        ..FakeRunner::default()
    });
    let vt = VideoTools::with_runner(tools(), runner.clone());
    let img = vt.frame(Path::new("/v/a.mov"), 256).unwrap();
    assert_eq!((img.width(), img.height()), (64, 36));
    let calls = runner.calls.lock().clone();
    assert_eq!(calls.len(), 2, "one ffprobe, one ffmpeg");
    assert!(calls[1].args.iter().any(|a| a == "1.250"), "10 % of 12.5 s");
    drop(calls);

    let runner = Arc::new(FakeRunner {
        probe_json: PHONE_JSON.into(),
        frame_fails_at: Some("1.250".into()),
        ..FakeRunner::default()
    });
    let vt = VideoTools::with_runner(tools(), runner.clone());
    vt.frame(Path::new("/v/a.mov"), 256).unwrap();
    let calls = runner.calls.lock().clone();
    assert!(calls.last().unwrap().args.iter().any(|a| a == "0.000"));

    let meta = vt.probe(Path::new("/v/a.mov")).unwrap();
    assert_eq!((meta.width, meta.height), (1080, 1920), "rotation applied");
    assert_eq!(meta.duration_ms, 12_500);
    assert!(meta.location.is_some() && meta.creation_time.is_some());

    let dec = VideoDecoder::new(vt);
    let dir = tempfile::tempdir().unwrap();
    let still = dir.path().join("a.png");
    image::RgbImage::new(8, 8).save(&still).unwrap();
    let img = dec.decode_raw(&still, lb_core::Format::Png, None).unwrap();
    assert_eq!(img.width(), 8, "stills bypass ffmpeg");
}

#[test]
fn tools_detect_in_searches_given_folders() {
    let dir = tempfile::tempdir().unwrap();
    assert!(Tools::detect_in(None, vec![dir.path().to_owned()]).is_none());
    let name = |n: &str| {
        if cfg!(windows) {
            format!("{n}.exe")
        } else {
            n.to_owned()
        }
    };
    std::fs::write(dir.path().join(name("ffmpeg")), b"").unwrap();
    assert!(
        Tools::detect_in(None, vec![dir.path().to_owned()]).is_none(),
        "both executables are required"
    );
    std::fs::write(dir.path().join(name("ffprobe")), b"").unwrap();
    let found = Tools::detect_in(None, vec![dir.path().to_owned()]).unwrap();
    assert_eq!(found.ffmpeg, dir.path().join(name("ffmpeg")));
    let by_dir = Tools::detect_in(Some(dir.path()), vec![]).unwrap();
    assert_eq!(by_dir, found);
    let by_file = Tools::detect_in(Some(&dir.path().join(name("ffmpeg"))), vec![]).unwrap();
    assert_eq!(by_file, found);
    assert!(Tools::detect_in(Some(Path::new("/definitely/missing")), vec![]).is_none());
}

#[test]
fn video_integration_ffmpeg() {
    let Some(tools) = Tools::detect(None) else {
        eprintln!("ffmpeg not found; skipping video_integration_ffmpeg");
        return;
    };
    let dir = tempfile::tempdir().unwrap();
    let file = dir.path().join("clip.mp4");
    let status = std::process::Command::new(&tools.ffmpeg)
        .args([
            "-v",
            "error",
            "-f",
            "lavfi",
            "-i",
            "testsrc=duration=2:size=320x240:rate=10",
        ])
        .args([
            "-pix_fmt",
            "yuv420p",
            "-metadata",
            "creation_time=2021-06-15T10:20:30Z",
        ])
        .arg(&file)
        .status()
        .unwrap();
    assert!(status.success());
    let vt = VideoTools::new(tools);
    let meta = vt.probe(&file).unwrap();
    assert_eq!((meta.width, meta.height), (320, 240));
    assert!(
        (1900..=2100).contains(&meta.duration_ms),
        "{}",
        meta.duration_ms
    );
    assert!(meta.creation_time.is_some());
    let frame = vt.frame(&file, 160).unwrap();
    assert_eq!((frame.width(), frame.height()), (160, 120));
    assert!(vt.probe(dir.path().join("missing.mp4").as_path()).is_err());
}
