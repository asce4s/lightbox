//! File-type registration for the running install (SPEC-VIEWER §V10).
//!
//! Startup calls [`register_available_viewer`], which never changes the user's
//! defaults. Becoming the default is [`AssocService::make_default`].

use crate::error::{AppError, Result};
#[cfg(target_os = "linux")]
use lb_assoc::LinuxAssoc;
#[cfg(target_os = "macos")]
use lb_assoc::MacosAssoc;
use lb_assoc::{AssocError, AssocStatus, FormatAssoc, MakeDefaultOutcome, SavedDefaults};

/// Registers Lightbox as an available viewer. Does not change the default handler.
pub fn register_available_viewer() -> Result<()> {
    let app = lb_assoc::AppInstall::current()?;
    let formats = current_formats();
    register_with(&app, &formats)
}

/// Removes the associations this install recorded.
pub(crate) fn unregister_current() -> Result<()> {
    let app = lb_assoc::AppInstall::current()?;
    unregister_with(&app)
}

/// Settings-facing association commands.
pub struct AssocService;

impl AssocService {
    /// Formats this build can decode and therefore register.
    pub fn formats() -> Vec<FormatAssoc> {
        current_formats()
    }

    /// Status of every registrable format on this machine.
    pub fn status() -> Result<Vec<(FormatAssoc, AssocStatus)>> {
        status_of(&current_formats())
    }

    /// Registers availability only.
    pub fn register_available() -> Result<()> {
        register_available_viewer()
    }

    /// Makes Lightbox the default for `extensions`.
    pub fn make_default(
        extensions: &[String],
    ) -> Result<(MakeDefaultOutcome, Option<SavedDefaults>)> {
        let selected = select(&current_formats(), extensions);
        let app = lb_assoc::AppInstall::current()?;
        make_default_with(&app, &selected)
    }

    /// Restores defaults captured by [`AssocService::make_default`].
    pub fn restore(saved: &SavedDefaults) -> Result<()> {
        restore_with(saved)
    }

    /// Removes everything this install added.
    pub fn unregister() -> Result<()> {
        unregister_current()
    }
}

fn current_formats() -> Vec<FormatAssoc> {
    lb_assoc::formats(
        lb_media::heif_decoder_available(),
        lb_media::raw_decoder_available(),
    )
}

fn select(formats: &[FormatAssoc], extensions: &[String]) -> Vec<FormatAssoc> {
    formats
        .iter()
        .copied()
        .filter(|fmt| {
            extensions
                .iter()
                .any(|ext| ext.eq_ignore_ascii_case(fmt.ext))
        })
        .collect()
}

#[cfg(target_os = "linux")]
fn register_with(app: &lb_assoc::AppInstall, formats: &[FormatAssoc]) -> Result<()> {
    LinuxAssoc::from_env()?
        .register_available(app, formats)
        .map_err(AppError::from)
}

#[cfg(target_os = "windows")]
fn register_with(app: &lb_assoc::AppInstall, formats: &[FormatAssoc]) -> Result<()> {
    windows_register(app, formats)
}

#[cfg(target_os = "macos")]
fn register_with(app: &lb_assoc::AppInstall, formats: &[FormatAssoc]) -> Result<()> {
    let _ = app;
    MacosAssoc::current()
        .register_available(formats)
        .map_err(AppError::from)
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn register_with(app: &lb_assoc::AppInstall, formats: &[FormatAssoc]) -> Result<()> {
    let _ = (app, formats);
    Ok(())
}

#[cfg(target_os = "linux")]
fn unregister_with(app: &lb_assoc::AppInstall) -> Result<()> {
    let _ = app;
    LinuxAssoc::from_env()?.unregister().map_err(AppError::from)
}

#[cfg(target_os = "windows")]
fn unregister_with(app: &lb_assoc::AppInstall) -> Result<()> {
    let _ = app;
    lb_assoc::Hkcu::unregister("Lightbox").map_err(AppError::from)
}

#[cfg(target_os = "macos")]
fn unregister_with(app: &lb_assoc::AppInstall) -> Result<()> {
    let _ = app;
    MacosAssoc::current().unregister().map_err(AppError::from)
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn unregister_with(app: &lb_assoc::AppInstall) -> Result<()> {
    let _ = app;
    Ok(())
}

#[cfg(target_os = "linux")]
fn status_of(formats: &[FormatAssoc]) -> Result<Vec<(FormatAssoc, AssocStatus)>> {
    LinuxAssoc::from_env()?
        .status(formats)
        .map_err(AppError::from)
}

#[cfg(target_os = "windows")]
fn status_of(formats: &[FormatAssoc]) -> Result<Vec<(FormatAssoc, AssocStatus)>> {
    windows_status(formats)
}

#[cfg(target_os = "macos")]
fn status_of(formats: &[FormatAssoc]) -> Result<Vec<(FormatAssoc, AssocStatus)>> {
    MacosAssoc::current()
        .status(formats)
        .map_err(AppError::from)
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn status_of(formats: &[FormatAssoc]) -> Result<Vec<(FormatAssoc, AssocStatus)>> {
    let _ = formats;
    Ok(Vec::new())
}

#[cfg(target_os = "linux")]
fn make_default_with(
    app: &lb_assoc::AppInstall,
    formats: &[FormatAssoc],
) -> Result<(MakeDefaultOutcome, Option<SavedDefaults>)> {
    let result = LinuxAssoc::from_env()?
        .make_default(app, formats)
        .map_err(AppError::from)?;
    Ok((result.outcome, result.saved))
}

#[cfg(target_os = "windows")]
fn make_default_with(
    app: &lb_assoc::AppInstall,
    formats: &[FormatAssoc],
) -> Result<(MakeDefaultOutcome, Option<SavedDefaults>)> {
    let _ = app;
    windows_make_default(formats)
}

#[cfg(target_os = "macos")]
fn make_default_with(
    app: &lb_assoc::AppInstall,
    formats: &[FormatAssoc],
) -> Result<(MakeDefaultOutcome, Option<SavedDefaults>)> {
    let _ = app;
    MacosAssoc::current()
        .make_default(formats)
        .map(|_| (MakeDefaultOutcome::UserConfirmationShown, None))
        .map_err(AppError::from)
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn make_default_with(
    app: &lb_assoc::AppInstall,
    formats: &[FormatAssoc],
) -> Result<(MakeDefaultOutcome, Option<SavedDefaults>)> {
    let _ = (app, formats);
    Ok((MakeDefaultOutcome::Done, None))
}

#[cfg(target_os = "linux")]
fn restore_with(saved: &SavedDefaults) -> Result<()> {
    LinuxAssoc::from_env()?
        .restore_previous(saved)
        .map_err(AppError::from)
}

#[cfg(target_os = "windows")]
fn restore_with(saved: &SavedDefaults) -> Result<()> {
    let _ = saved;
    lb_assoc::Hkcu::unregister("Lightbox")?;
    let plan = lb_assoc::make_default_plan("Lightbox", windows_11());
    if let Some(uri) = plan.settings_uri {
        lb_assoc::Hkcu::open_settings(&uri)?;
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn restore_with(saved: &SavedDefaults) -> Result<()> {
    let _ = saved;
    Ok(())
}

#[cfg(not(any(target_os = "linux", target_os = "windows", target_os = "macos")))]
fn restore_with(saved: &SavedDefaults) -> Result<()> {
    let _ = saved;
    Ok(())
}

#[cfg(target_os = "windows")]
fn windows_register(app: &lb_assoc::AppInstall, formats: &[FormatAssoc]) -> Result<()> {
    let exe = exe_string(&app.exe)?;
    let name = exe_file_name(&app.exe);
    let _ = lb_assoc::Hkcu::unregister("Lightbox");
    let plan = lb_assoc::register_plan(&exe, &name, "Lightbox", formats);
    lb_assoc::Hkcu::apply(&plan).map_err(AppError::from)
}

#[cfg(target_os = "windows")]
fn windows_make_default(
    _formats: &[FormatAssoc],
) -> Result<(MakeDefaultOutcome, Option<SavedDefaults>)> {
    let plan = lb_assoc::make_default_plan("Lightbox", windows_11());
    if let Some(uri) = plan.settings_uri {
        lb_assoc::Hkcu::open_settings(&uri)?;
    }
    Ok((MakeDefaultOutcome::OpenedSystemSettings, None))
}

#[cfg(target_os = "windows")]
fn windows_status(formats: &[FormatAssoc]) -> Result<Vec<(FormatAssoc, AssocStatus)>> {
    let mut out = Vec::new();
    for fmt in formats {
        let key = format!(
            "Software\\Microsoft\\Windows\\CurrentVersion\\Explorer\\FileExts\\.{}\\UserChoice",
            fmt.ext
        );
        let ours = lb_assoc::progid_for("Lightbox", fmt.ext);
        let status = match lb_assoc::Hkcu::read_sz(&key, "ProgId")? {
            Some(progid) if progid == ours => AssocStatus::Default,
            Some(progid) => AssocStatus::OtherDefault(progid),
            None => {
                let open_with = format!("Software\\Classes\\.{}\\OpenWithProgids", fmt.ext);
                if lb_assoc::Hkcu::has_value(&open_with, &ours)? {
                    AssocStatus::Available
                } else {
                    AssocStatus::Unregistered
                }
            }
        };
        out.push((*fmt, status));
    }
    Ok(out)
}

#[cfg(target_os = "windows")]
fn windows_11() -> bool {
    lb_assoc::Hkcu::read_sz(
        r"Software\Microsoft\Windows NT\CurrentVersion",
        "CurrentBuild",
    )
    .ok()
    .flatten()
    .and_then(|text| text.parse::<u32>().ok())
    .is_some_and(|build| build >= 22_000)
}

#[cfg(target_os = "windows")]
fn exe_string(path: &std::path::Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| AppError::Invalid("executable path is not utf-8".into()))
}

#[cfg(target_os = "windows")]
fn exe_file_name(path: &std::path::Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("lightbox.exe")
        .to_owned()
}

impl From<AssocError> for AppError {
    fn from(error: AssocError) -> Self {
        match error {
            AssocError::Io(error) => Self::Io(error),
            AssocError::Invalid(message) => Self::Invalid(message),
            AssocError::Unavailable => {
                Self::Invalid("file type registration is available in the installed app".into())
            }
        }
    }
}
