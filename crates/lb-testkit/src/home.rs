//! Temporary Lightbox home directory (SPEC §4.4, §12.2).

use parking_lot::{Mutex, MutexGuard};
use std::ffi::OsString;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

/// Environment variable that redirects data and cache directories (SPEC §4.4).
pub const LIGHTBOX_HOME_ENV: &str = "LIGHTBOX_HOME";

static ENV_LOCK: Mutex<()> = Mutex::new(());

/// A temp directory exported as `LIGHTBOX_HOME` for the lifetime of the value.
///
/// The environment is process-global, so a `TestHome` holds a process-wide lock:
/// tests that create one run one at a time within a test binary. The previous value of the
/// variable is restored on drop.
#[derive(Debug)]
pub struct TestHome {
    dir: TempDir,
    previous: Option<OsString>,
    _guard: MutexGuard<'static, ()>,
}

impl TestHome {
    /// Creates `<tmp>/data` and `<tmp>/cache` and points `LIGHTBOX_HOME` at `<tmp>`.
    pub fn new() -> Self {
        let guard = ENV_LOCK.lock();
        let dir = tempfile::Builder::new()
            .prefix("lightbox-test-")
            .tempdir()
            .unwrap();
        std::fs::create_dir_all(dir.path().join("data")).unwrap();
        std::fs::create_dir_all(dir.path().join("cache")).unwrap();
        let previous = std::env::var_os(LIGHTBOX_HOME_ENV);
        set_env(Some(dir.path().as_os_str()));
        Self {
            dir,
            previous,
            _guard: guard,
        }
    }

    /// Root of the temporary home (the value of `LIGHTBOX_HOME`).
    pub fn path(&self) -> &Path {
        self.dir.path()
    }

    /// `<home>/data`.
    pub fn data_dir(&self) -> PathBuf {
        self.dir.path().join("data")
    }

    /// `<home>/cache`.
    pub fn cache_dir(&self) -> PathBuf {
        self.dir.path().join("cache")
    }
}

impl Default for TestHome {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for TestHome {
    fn drop(&mut self) {
        set_env(self.previous.as_deref());
    }
}

#[allow(unsafe_code)]
fn set_env(value: Option<&std::ffi::OsStr>) {
    // SAFETY: callers hold `ENV_LOCK`, so no other `TestHome` mutates the environment
    // concurrently. Test code is the only place this crate is used.
    unsafe {
        match value {
            Some(v) => std::env::set_var(LIGHTBOX_HOME_ENV, v),
            None => std::env::remove_var(LIGHTBOX_HOME_ENV),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn testhome_sets_lightbox_home() {
        let outer = std::env::var_os(LIGHTBOX_HOME_ENV);
        let path;
        {
            let home = TestHome::new();
            path = home.path().to_path_buf();
            assert_eq!(
                std::env::var_os(LIGHTBOX_HOME_ENV),
                Some(path.clone().into_os_string())
            );
            assert!(home.data_dir().is_dir());
            assert!(home.cache_dir().is_dir());
            assert_eq!(home.data_dir(), path.join("data"));
        }
        assert!(!path.exists(), "temp dir removed on drop");
        assert_eq!(
            std::env::var_os(LIGHTBOX_HOME_ENV),
            outer,
            "env restored on drop"
        );
    }

    #[test]
    fn testhomes_do_not_overlap() {
        let a = TestHome::new();
        let a_path = a.path().to_path_buf();
        drop(a);
        let b = TestHome::new();
        assert_ne!(a_path, b.path());
    }
}
