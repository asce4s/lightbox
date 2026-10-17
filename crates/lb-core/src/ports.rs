//! Interfaces to the operating system that tests replace with fakes.

use std::io;
use std::path::Path;

/// Moves files to the operating system's trash. Implementations must never delete permanently.
pub trait TrashBackend: Send + Sync {
    /// Moves `path` to the trash.
    fn trash(&self, path: &Path) -> io::Result<()>;
}

/// Opens files in other applications.
pub trait Opener: Send + Sync {
    /// Opens `path` with the default application.
    fn open(&self, path: &Path) -> io::Result<()>;

    /// Shows `path` selected in the system file manager.
    fn reveal(&self, path: &Path) -> io::Result<()>;
}
