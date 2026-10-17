//! Locating `ffmpeg` and `ffprobe`, and building their command lines (SPEC §6.16).

use crate::command::CommandSpec;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// Where the two executables are.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tools {
    /// The `ffmpeg` executable.
    pub ffmpeg: PathBuf,
    /// The `ffprobe` executable.
    pub ffprobe: PathBuf,
}

fn exe_name(base: &str) -> String {
    if cfg!(windows) {
        format!("{base}.exe")
    } else {
        base.to_owned()
    }
}

fn is_file(path: &Path) -> bool {
    path.is_file()
}

fn find_in(dirs: impl IntoIterator<Item = PathBuf>, base: &str) -> Option<PathBuf> {
    let name = exe_name(base);
    dirs.into_iter().map(|d| d.join(&name)).find(|p| is_file(p))
}

impl Tools {
    /// Finds both tools. `configured` is the settings value (`video.ffmpeg_path`): either the
    /// ffmpeg executable or the folder holding it; empty means search `PATH`. `ffprobe` is looked
    /// up next to ffmpeg first. Returns `None` when either is missing.
    pub fn detect(configured: Option<&Path>) -> Option<Self> {
        let path_dirs = || {
            std::env::var_os("PATH")
                .map(|p| std::env::split_paths(&p).collect::<Vec<_>>())
                .unwrap_or_default()
        };
        Self::detect_in(configured, path_dirs())
    }

    /// [`Self::detect`] with an explicit search path (tests).
    pub fn detect_in(configured: Option<&Path>, path_dirs: Vec<PathBuf>) -> Option<Self> {
        let configured = configured.filter(|p| !p.as_os_str().is_empty());
        let (ffmpeg, extra_dir) = match configured {
            Some(p) if p.is_dir() => (find_in([p.to_owned()], "ffmpeg")?, Some(p.to_owned())),
            Some(p) if is_file(p) => (p.to_owned(), p.parent().map(Path::to_owned)),
            Some(_) => return None,
            None => (find_in(path_dirs.clone(), "ffmpeg")?, None),
        };
        let beside = ffmpeg.parent().map(Path::to_owned);
        let ffprobe = find_in(
            beside.into_iter().chain(extra_dir).chain(path_dirs),
            "ffprobe",
        )?;
        Some(Self { ffmpeg, ffprobe })
    }

    /// `ffprobe -v error -print_format json -show_format -show_streams <file>`.
    pub fn probe_command(&self, video: &Path) -> CommandSpec {
        let mut args: Vec<OsString> = [
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
        ]
        .iter()
        .map(OsString::from)
        .collect();
        args.push(video.as_os_str().to_owned());
        CommandSpec {
            program: self.ffprobe.clone(),
            args,
        }
    }

    /// One PNG frame at `at_secs`, scaled to fit `edge` pixels, written to standard output.
    /// `-ss` comes before `-i` so ffmpeg seeks instead of decoding up to that point.
    pub fn thumbnail_command(&self, video: &Path, at_secs: f64, edge: u32) -> CommandSpec {
        let mut args: Vec<OsString> = vec!["-v".into(), "error".into(), "-nostdin".into()];
        args.extend(["-ss".into(), format!("{at_secs:.3}").into()]);
        args.extend(["-i".into(), video.as_os_str().to_owned()]);
        args.extend(["-frames:v".into(), "1".into()]);
        args.extend([
            "-vf".into(),
            format!("scale={edge}:{edge}:force_original_aspect_ratio=decrease").into(),
        ]);
        args.extend(["-f".into(), "image2pipe".into()]);
        args.extend(["-c:v".into(), "png".into(), "pipe:1".into()]);
        CommandSpec {
            program: self.ffmpeg.clone(),
            args,
        }
    }
}

/// The moment a thumbnail is taken: 10 % into the video (SPEC §6.16).
pub fn thumbnail_time(duration_ms: u64) -> f64 {
    duration_ms as f64 / 1000.0 * 0.1
}
