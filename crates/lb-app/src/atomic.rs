//! Atomic file writes (SPEC §10): write to a sibling temp file, then rename over the target.

use std::fs::File;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COUNTER: AtomicU64 = AtomicU64::new(0);

/// Suffix of the temporary files this module creates next to the destination.
pub const TEMP_SUFFIX: &str = ".lbtmp";

fn temp_path(dest: &Path) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::Relaxed);
    let name = dest
        .file_name()
        .map_or_else(|| "file".into(), |n| n.to_string_lossy().into_owned());
    dest.with_file_name(format!(".{name}.{}-{n}{TEMP_SUFFIX}", std::process::id()))
}

/// Writes `dest` atomically. `write` fills a temporary file next to `dest`; only when it
/// succeeds and the data is flushed to disk is the file renamed over `dest`. On any error the
/// temp file is removed and an existing `dest` is left untouched.
pub fn write_atomic<F>(dest: &Path, write: F) -> io::Result<()>
where
    F: FnOnce(&mut File) -> io::Result<()>,
{
    let tmp = temp_path(dest);
    let result = (|| {
        let mut file = File::create(&tmp)?;
        write(&mut file)?;
        file.flush()?;
        file.sync_all()?;
        drop(file);
        std::fs::rename(&tmp, dest)
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&tmp);
    }
    result
}

/// Convenience wrapper around [`write_atomic`] for in-memory contents.
pub fn write_bytes_atomic(dest: &Path, bytes: &[u8]) -> io::Result<()> {
    write_atomic(dest, |f| f.write_all(bytes))
}
