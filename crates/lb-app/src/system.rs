//! Real implementations of the operating-system ports.

use lb_core::{Opener, TrashBackend};
use std::io;
use std::path::Path;

/// Moves files to the platform trash with the `trash` crate.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemTrash;

impl TrashBackend for SystemTrash {
    fn trash(&self, path: &Path) -> io::Result<()> {
        trash::delete(path).map_err(|e| io::Error::other(e.to_string()))
    }
}

/// Opens files with the platform's default application and file manager.
#[derive(Debug, Default, Clone, Copy)]
pub struct SystemOpener;

impl Opener for SystemOpener {
    fn open(&self, path: &Path) -> io::Result<()> {
        opener::open(path).map_err(|e| io::Error::other(e.to_string()))
    }

    fn reveal(&self, path: &Path) -> io::Result<()> {
        #[cfg(target_os = "macos")]
        {
            std::process::Command::new("open")
                .arg("-R")
                .arg(path)
                .status()
                .map(drop)
        }
        #[cfg(target_os = "windows")]
        {
            let mut arg = std::ffi::OsString::from("/select,");
            arg.push(path);
            std::process::Command::new("explorer")
                .arg(arg)
                .status()
                .map(drop)
        }
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        {
            let dir = path.parent().unwrap_or(path);
            opener::open(dir).map_err(|e| io::Error::other(e.to_string()))
        }
    }
}
