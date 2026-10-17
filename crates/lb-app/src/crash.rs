//! Crash safety (SPEC §10): a panic hook that writes `{data}/crash-*.log`, and the startup
//! check that tells the user about logs left by a previous run.

use crate::atomic::write_bytes_atomic;
use std::backtrace::Backtrace;
use std::io;
use std::path::{Path, PathBuf};

const PREFIX: &str = "crash-";
const SUFFIX: &str = ".log";

/// Writes a crash log into `dir` and returns its path.
pub fn write_crash_log(
    dir: &Path,
    message: &str,
    location: Option<&str>,
    backtrace: &str,
) -> io::Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let now = chrono::Utc::now();
    let stem = format!(
        "{PREFIX}{}-{}",
        now.format("%Y%m%dT%H%M%S%.3f"),
        std::process::id()
    );
    let mut path = dir.join(format!("{stem}{SUFFIX}"));
    let mut n = 1;
    while path.exists() {
        path = dir.join(format!("{stem}-{n}{SUFFIX}"));
        n += 1;
    }
    let body = format!(
        "Lightbox {} crashed at {}\nmessage: {message}\nlocation: {}\n\nbacktrace:\n{backtrace}\n",
        env!("CARGO_PKG_VERSION"),
        now.to_rfc3339(),
        location.unwrap_or("unknown"),
    );
    write_bytes_atomic(&path, body.as_bytes())?;
    Ok(path)
}

/// Installs a panic hook that logs the panic through `tracing`, writes a crash log into
/// `data_dir`, and then runs the previously installed hook.
pub fn install_panic_hook(data_dir: PathBuf) {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let message = info
            .payload()
            .downcast_ref::<&str>()
            .map(|s| (*s).to_owned())
            .or_else(|| info.payload().downcast_ref::<String>().cloned())
            .unwrap_or_else(|| "panic".into());
        let location = info.location().map(ToString::to_string);
        let backtrace = Backtrace::force_capture().to_string();
        tracing::error!(%message, location = location.as_deref(), "panic");
        if let Err(e) = write_crash_log(&data_dir, &message, location.as_deref(), &backtrace) {
            tracing::error!(error = %e, "could not write crash log");
        }
        previous(info);
    }));
}

/// Crash logs left by earlier runs, oldest first. The UI shows a notice when this is not empty.
pub fn pending_crash_logs(data_dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(data_dir) else {
        return Vec::new();
    };
    let mut logs: Vec<PathBuf> = entries
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .is_some_and(|n| n.starts_with(PREFIX) && n.ends_with(SUFFIX))
        })
        .collect();
    logs.sort();
    logs
}

/// Removes the crash logs once the user has acknowledged the notice.
pub fn dismiss_crash_logs(data_dir: &Path) -> io::Result<()> {
    for path in pending_crash_logs(data_dir) {
        std::fs::remove_file(path)?;
    }
    Ok(())
}
