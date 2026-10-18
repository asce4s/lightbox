//! How the GUI process was started (SPEC-VIEWER §V2.2, §V2.4).

use crate::error::Result;
use crate::lightbox::Lightbox;
use lb_scan::WatchConfig;
use percent_encoding::percent_decode_str;
use std::ffi::OsStr;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// How long macOS waits for `on_open_urls` when `argv` has no paths.
pub const MACOS_OPEN_URL_WAIT: Duration = Duration::from_millis(250);

/// Host the grammar is parsed for. `-psn_*` is ignored on every host; macOS is the one that adds it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Platform {
    /// macOS, including Finder launches.
    Macos,
    /// Linux, Windows and everything else.
    Other,
}

/// What the process should open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Launch {
    /// The library window, with the startup rescan and watchers.
    Library,
    /// The viewer only. No rescan and no watchers until the library is opened.
    View {
        /// Files and folders, absolute.
        paths: Vec<PathBuf>,
        /// Write a perf report instead of opening a window (§V12).
        perf: bool,
    },
    /// Remove file-type registrations and exit (§V10).
    UnregisterFileTypes,
}

/// Outcome of the macOS launch deferral.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MacosDecision {
    /// `argv` has no paths and the wait has not elapsed.
    Wait,
    /// Open this launch.
    Ready(Launch),
}

/// Parses the GUI binary's arguments.
///
/// `args` includes the program name. Relative paths are resolved against `cwd`.
/// A path that does not exist still opens the viewer so the error card can explain why.
pub fn parse_launch<S: AsRef<OsStr>>(args: &[S], cwd: &Path, platform: Platform) -> Launch {
    let _ = platform;
    let rest = args.get(1..).unwrap_or(&[]);
    let mut tokens = Vec::new();
    for arg in rest {
        let text = arg.as_ref().to_string_lossy();
        if text.starts_with("-psn_") {
            continue;
        }
        tokens.push(text.into_owned());
    }
    if tokens
        .iter()
        .any(|token| token == "--unregister-file-types")
    {
        return Launch::UnregisterFileTypes;
    }
    if tokens.is_empty() || tokens[0] == "--library" {
        return Launch::Library;
    }
    if tokens[0] == "view" {
        return parse_view(&tokens[1..], cwd);
    }
    let paths = tokens
        .iter()
        .filter(|token| !token.starts_with('-'))
        .map(|token| resolve_arg(token, cwd))
        .collect::<Vec<_>>();
    if paths.is_empty() {
        Launch::Library
    } else {
        Launch::View { paths, perf: false }
    }
}

fn parse_view(args: &[String], cwd: &Path) -> Launch {
    let mut perf = false;
    let mut paths = Vec::new();
    for arg in args {
        if arg == "--perf-json" {
            perf = true;
            continue;
        }
        if arg.starts_with('-') {
            continue;
        }
        paths.push(resolve_arg(arg, cwd));
    }
    Launch::View { paths, perf }
}

fn resolve_arg(arg: &str, cwd: &Path) -> PathBuf {
    if let Some(path) = file_url(arg) {
        return path;
    }
    let path = PathBuf::from(arg);
    if path.is_absolute() {
        path
    } else {
        cwd.join(path)
    }
}

/// Percent-decodes a `file://` URL into a path. Other strings return `None`.
pub fn file_url(arg: &str) -> Option<PathBuf> {
    let rest = arg.strip_prefix("file://")?;
    let rest = rest.strip_prefix("localhost").unwrap_or(rest);
    let decoded = percent_decode_str(rest).decode_utf8_lossy();
    let decoded = decoded.as_ref();
    #[cfg(windows)]
    let decoded = decoded
        .strip_prefix('/')
        .filter(|rest| rest.chars().nth(1) == Some(':'))
        .unwrap_or(decoded);
    if decoded.is_empty() {
        None
    } else {
        Some(PathBuf::from(decoded))
    }
}

/// Decides whether macOS should keep waiting for document URLs.
///
/// `urls` are the file URLs `on_open_urls` has delivered. `reopen` is a Dock click.
pub fn macos_route(
    argv: &Launch,
    urls: &[String],
    elapsed: Duration,
    reopen: bool,
) -> MacosDecision {
    if reopen {
        return MacosDecision::Ready(Launch::Library);
    }
    if let Launch::UnregisterFileTypes = argv {
        return MacosDecision::Ready(argv.clone());
    }
    if let Launch::View { paths, perf } = argv
        && (!paths.is_empty() || *perf)
    {
        return MacosDecision::Ready(argv.clone());
    }
    if !urls.is_empty() && elapsed <= MACOS_OPEN_URL_WAIT {
        let paths = urls
            .iter()
            .map(|url| file_url(url).unwrap_or_else(|| PathBuf::from(url)))
            .collect();
        return MacosDecision::Ready(Launch::View { paths, perf: false });
    }
    if elapsed >= MACOS_OPEN_URL_WAIT {
        return MacosDecision::Ready(Launch::Library);
    }
    MacosDecision::Wait
}

/// Applies the fast path. [`Launch::View`] does not rescan and does not start watchers.
pub fn apply_startup(lb: &Lightbox, launch: &Launch) -> Result<()> {
    if matches!(launch, Launch::Library) {
        lb.library().rescan_all()?;
        lb.library().start_watching(WatchConfig::default())?;
    }
    Ok(())
}

/// Opens the library from the viewer: the startup rescan and the watchers begin here.
pub fn open_library_from_viewer(lb: &Lightbox) -> Result<()> {
    apply_startup(lb, &Launch::Library)
}

/// Removes the file-type registrations this install added.
///
/// The platform executors land in M13.10. The flag is recognised now so a second
/// binary can already parse it.
pub fn unregister_file_types() -> Result<()> {
    tracing::info!("file type registration is removed by the association service");
    Ok(())
}
