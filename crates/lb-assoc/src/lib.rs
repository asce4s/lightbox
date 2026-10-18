//! Per-user file-type associations for the photo viewer (SPEC-VIEWER §V10).
//!
//! Plans are pure data. Executors apply them to a sandbox (tests) or the real
//! per-user location (HKCU, XDG, or the macOS bundle plist).
#![warn(missing_docs)]

mod error;
mod format;
mod linux;
mod macos;
mod windows;

pub use error::{AssocError, Result};
pub use format::{base_formats, formats, mime_types, progid_family};
pub use linux::{
    DESKTOP_ID, ICON_NAME, LinuxAssoc, LinuxDirs, MimeEdit, MimePrevious, current_exec,
    desktop_entry, edit_mimeapps, quote_exec, restore_mimeapps, strip_desktop_id,
};
pub use macos::{BUNDLE_ID, MacosAssoc, current_is_bundled, document_types_xml, info_plist};
#[cfg(windows)]
pub use windows::Hkcu;
pub use windows::{
    FRIENDLY_APP_NAME, MemoryRegistry, RegData, RegWrite, WindowsPlan, make_default_plan,
    memory_status, progid_for, register_plan, settings_uri,
};

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Paths and names of this Lightbox install.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AppInstall {
    /// Executable passed to the OS (`current_exe`, or `$APPIMAGE`).
    pub exe: PathBuf,
    /// macOS bundle id (`app.lightbox.Lightbox`).
    pub bundle_id: String,
    /// Name shown in Open With and Default Apps.
    pub display_name: String,
    /// PNG copied to the hicolor icon theme. `None` skips the copy.
    pub icon: Option<PathBuf>,
}

impl AppInstall {
    /// The running process: `$APPIMAGE` or `current_exe`, with a sibling icon if one exists.
    pub fn current() -> Result<Self> {
        let exe = current_exec()?;
        let icon = exe.parent().and_then(|dir| {
            ["app.lightbox.Lightbox.png", "lightbox.png"]
                .into_iter()
                .map(|name| dir.join(name))
                .find(|path| path.is_file())
        });
        Ok(Self {
            exe,
            bundle_id: BUNDLE_ID.to_owned(),
            display_name: FRIENDLY_APP_NAME.to_owned(),
            icon,
        })
    }
}

/// One extension Lightbox can register.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FormatAssoc {
    /// Extension without the dot.
    pub ext: &'static str,
    /// MIME type.
    pub mime: &'static str,
    /// macOS uniform type identifier.
    pub uti: &'static str,
    /// Human label used in ProgIDs and document types.
    pub label: &'static str,
}

/// Whether Lightbox is the default handler for a type.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AssocStatus {
    /// The OS default points at Lightbox.
    Default,
    /// Lightbox is listed, and no default is set.
    Available,
    /// Another application is the default. The string is its id or ProgID.
    OtherDefault(String),
    /// Lightbox is not registered for this type.
    Unregistered,
}

/// What `make_default` was able to do. Windows and macOS cannot write the default themselves.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MakeDefaultOutcome {
    /// The default was changed.
    Done,
    /// The OS is showing its own confirmation.
    UserConfirmationShown,
    /// Default Apps (or the platform equivalent) was opened.
    OpenedSystemSettings,
}

/// Outcome of `make_default`, including defaults to restore later.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MakeDefaultResult {
    /// How far the OS let us go.
    pub outcome: MakeDefaultOutcome,
    /// Previous defaults, when the OS allowed us to read them.
    pub saved: Option<SavedDefaults>,
}

/// Previous OS defaults, stored as a backend JSON payload (SPEC-VIEWER §V10, §V11).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SavedDefaults {
    /// Backend-specific JSON. Empty until a successful `make_default` stores one.
    pub payload: String,
}

/// Backend that reads and writes one operating system's associations.
pub trait AssocBackend {
    /// Current status of each format.
    fn status(&self, formats: &[FormatAssoc]) -> Result<Vec<(FormatAssoc, AssocStatus)>>;
    /// Register as an available handler. Must not change the user's defaults.
    fn register_available(&self, app: &AppInstall, formats: &[FormatAssoc]) -> Result<()>;
    /// Become the default for `formats`.
    fn make_default(&self, app: &AppInstall, formats: &[FormatAssoc]) -> Result<MakeDefaultResult>;
    /// Put back defaults captured by [`AssocBackend::make_default`].
    fn restore_previous(&self, saved: &SavedDefaults) -> Result<()>;
    /// Remove everything this install added.
    fn unregister(&self, app: &AppInstall) -> Result<()>;
}

impl AssocBackend for LinuxAssoc {
    fn status(&self, formats: &[FormatAssoc]) -> Result<Vec<(FormatAssoc, AssocStatus)>> {
        LinuxAssoc::status(self, formats)
    }

    fn register_available(&self, app: &AppInstall, formats: &[FormatAssoc]) -> Result<()> {
        LinuxAssoc::register_available(self, app, formats)
    }

    fn make_default(&self, app: &AppInstall, formats: &[FormatAssoc]) -> Result<MakeDefaultResult> {
        LinuxAssoc::make_default(self, app, formats)
    }

    fn restore_previous(&self, saved: &SavedDefaults) -> Result<()> {
        LinuxAssoc::restore_previous(self, saved)
    }

    fn unregister(&self, _app: &AppInstall) -> Result<()> {
        LinuxAssoc::unregister(self)
    }
}

impl AssocBackend for MacosAssoc {
    fn status(&self, formats: &[FormatAssoc]) -> Result<Vec<(FormatAssoc, AssocStatus)>> {
        MacosAssoc::status(self, formats)
    }

    fn register_available(&self, _app: &AppInstall, formats: &[FormatAssoc]) -> Result<()> {
        MacosAssoc::register_available(self, formats)
    }

    fn make_default(
        &self,
        _app: &AppInstall,
        formats: &[FormatAssoc],
    ) -> Result<MakeDefaultResult> {
        MacosAssoc::make_default(self, formats)
    }

    fn restore_previous(&self, _saved: &SavedDefaults) -> Result<()> {
        Ok(())
    }

    fn unregister(&self, _app: &AppInstall) -> Result<()> {
        MacosAssoc::unregister(self)
    }
}
