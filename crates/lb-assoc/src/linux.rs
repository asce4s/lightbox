//! Freedesktop desktop entry, icon and mimeapps.list editor (SPEC-VIEWER §V10.5).

use crate::{
    AppInstall, AssocError, AssocStatus, FormatAssoc, MakeDefaultOutcome, MakeDefaultResult,
    Result, SavedDefaults,
};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

/// Desktop-file id installed under `applications/`.
pub const DESKTOP_ID: &str = "app.lightbox.Viewer.desktop";

/// Icon name installed under `hicolor/512x512/apps/`.
pub const ICON_NAME: &str = "app.lightbox.Lightbox";

/// Directories the freedesktop lookup consults. Tests pass a temp tree.
#[derive(Debug, Clone)]
pub struct LinuxDirs {
    /// `$XDG_DATA_HOME` (default `~/.local/share`).
    pub data_home: PathBuf,
    /// `$XDG_CONFIG_HOME` (default `~/.config`).
    pub config_home: PathBuf,
    /// `$XDG_CONFIG_DIRS`, in search order.
    pub config_dirs: Vec<PathBuf>,
    /// `$XDG_CURRENT_DESKTOP`, lower-cased, already split on `:`.
    pub desktops: Vec<String>,
}

impl LinuxDirs {
    /// Reads the XDG variables. `HOME` must be set.
    pub fn from_env() -> Result<Self> {
        let home = std::env::var_os("HOME")
            .filter(|value| !value.is_empty())
            .map(PathBuf::from)
            .ok_or_else(|| AssocError::Invalid("HOME is not set".into()))?;
        let data_home = env_path("XDG_DATA_HOME", home.join(".local/share"));
        let config_home = env_path("XDG_CONFIG_HOME", home.join(".config"));
        let config_dirs = match std::env::var_os("XDG_CONFIG_DIRS") {
            Some(value) if !value.is_empty() => std::env::split_paths(&value)
                .filter(|path| !path.as_os_str().is_empty())
                .collect(),
            _ => vec![PathBuf::from("/etc/xdg")],
        };
        let desktops = std::env::var("XDG_CURRENT_DESKTOP")
            .unwrap_or_default()
            .split(':')
            .filter(|part| !part.is_empty())
            .map(|part| part.to_ascii_lowercase())
            .collect();
        Ok(Self {
            data_home,
            config_home,
            config_dirs,
            desktops,
        })
    }
}

fn env_path(key: &str, fallback: PathBuf) -> PathBuf {
    match std::env::var_os(key) {
        Some(value) if !value.is_empty() => PathBuf::from(value),
        _ => fallback,
    }
}

/// Quotes an executable path for a desktop `Exec` key (SPEC-VIEWER §V10.5).
pub fn quote_exec(path: &str) -> String {
    let mut escaped = String::new();
    let mut needs_quotes = false;
    for ch in path.chars() {
        match ch {
            '%' => escaped.push_str("%%"),
            '"' | '`' | '$' | '\\' => {
                needs_quotes = true;
                escaped.push('\\');
                escaped.push(ch);
            }
            ' ' => {
                needs_quotes = true;
                escaped.push(ch);
            }
            _ => escaped.push(ch),
        }
    }
    if needs_quotes {
        format!("\"{escaped}\"")
    } else {
        escaped
    }
}

/// The viewer `.desktop` file for `exec` and `formats`.
pub fn desktop_entry(exec: &str, formats: &[FormatAssoc]) -> String {
    let mimes = mime_field(formats);
    format!(
        "\
[Desktop Entry]
Type=Application
Name=Lightbox Photo Viewer
GenericName=Image Viewer
Comment=View and organise photos
Exec={exec} view %F
Icon={ICON_NAME}
Terminal=false
Categories=Graphics;Viewer;Photography;
MimeType={mimes}
StartupNotify=true
StartupWMClass=lightbox
",
        exec = quote_exec(exec),
    )
}

fn mime_field(formats: &[FormatAssoc]) -> String {
    let mut out = crate::format::mime_types(formats).join(";");
    if !out.is_empty() {
        out.push(';');
    }
    out
}

/// One previous default captured while editing a mimeapps file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MimePrevious {
    /// MIME type that was changed.
    pub mime: String,
    /// The previous value, including any trailing semicolon. `None` if the line was added.
    pub previous: Option<String>,
}

/// Result of editing one mimeapps file in memory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MimeEdit {
    /// File contents after the edit, always ending in a newline when non-empty.
    pub text: String,
    /// The `[Default Applications]` section was not in the original file.
    pub created_section: bool,
    /// Previous values, in the same order as the requested types.
    pub previous: Vec<MimePrevious>,
}

/// Sets `<mime>=<desktop_id>;` under `[Default Applications]`, preserving every other line.
pub fn edit_mimeapps(original: &str, desktop_id: &str, mimes: &[&str]) -> MimeEdit {
    let value = if desktop_id.ends_with(';') {
        desktop_id.to_owned()
    } else {
        format!("{desktop_id};")
    };
    let mut lines = split_lines(original);
    let (created_section, start, mut end) = ensure_section(&mut lines);
    let mut previous = Vec::new();
    for mime in mimes {
        if let Some(index) = find_key(&lines, start, end, mime) {
            previous.push(MimePrevious {
                mime: (*mime).to_owned(),
                previous: value_of(&lines[index]),
            });
            lines[index] = format!("{mime}={value}");
        } else {
            previous.push(MimePrevious {
                mime: (*mime).to_owned(),
                previous: None,
            });
            let at = insert_at(&lines, start, end);
            lines.insert(at, format!("{mime}={value}"));
            end += 1;
        }
    }
    MimeEdit {
        text: join_lines(&lines),
        created_section,
        previous,
    }
}

/// Puts the captured values back. Lines we added are removed.
pub fn restore_mimeapps(
    original: &str,
    created_section: bool,
    previous: &[MimePrevious],
) -> String {
    let mut lines = split_lines(original);
    for entry in previous {
        let Some((_, start, end)) = section_bounds(&lines, "Default Applications") else {
            if let Some(value) = &entry.previous {
                let _ = ensure_section(&mut lines);
                if let Some((_, start, end)) = section_bounds(&lines, "Default Applications")
                    && find_key(&lines, start, end, &entry.mime).is_none()
                {
                    let at = insert_at(&lines, start, end);
                    lines.insert(at, format!("{}={value}", entry.mime));
                }
            }
            continue;
        };
        if let Some(index) = find_key(&lines, start, end, &entry.mime) {
            match &entry.previous {
                Some(value) => lines[index] = format!("{}={value}", entry.mime),
                None => {
                    lines.remove(index);
                }
            }
        } else if let Some(value) = &entry.previous {
            let at = insert_at(&lines, start, end);
            lines.insert(at, format!("{}={value}", entry.mime));
        }
    }
    if created_section
        && section_has_no_keys(&lines)
        && let Some((header, _, end)) = section_bounds(&lines, "Default Applications")
    {
        lines.drain(header..end);
    }
    join_lines(&lines)
}

/// Removes `desktop_id` from default values. Other handlers on the same line stay.
pub fn strip_desktop_id(original: &str, desktop_id: &str) -> String {
    let lines: Vec<String> = split_lines(original)
        .into_iter()
        .filter_map(|line| strip_line(&line, desktop_id))
        .collect();
    join_lines(&lines)
}

fn strip_line(line: &str, desktop_id: &str) -> Option<String> {
    let Some((key, value)) = line.split_once('=') else {
        return Some(line.to_owned());
    };
    if key.trim_start().starts_with('#') {
        return Some(line.to_owned());
    }
    let kept: Vec<&str> = value
        .split(';')
        .map(str::trim)
        .filter(|part| !part.is_empty() && *part != desktop_id)
        .collect();
    if kept.len()
        == value
            .split(';')
            .filter(|part| !part.trim().is_empty())
            .count()
    {
        return Some(line.to_owned());
    }
    if kept.is_empty() {
        return None;
    }
    Some(format!("{}={};", key.trim(), kept.join(";")))
}

fn split_lines(text: &str) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }
    let mut lines: Vec<String> = text
        .split('\n')
        .map(|line| line.trim_end_matches('\r').to_owned())
        .collect();
    if text.ends_with('\n') && lines.last().is_some_and(String::is_empty) {
        lines.pop();
    }
    lines
}

fn join_lines(lines: &[String]) -> String {
    if lines.is_empty() {
        return String::new();
    }
    let mut out = lines.join("\n");
    if !out.ends_with('\n') {
        out.push('\n');
    }
    out
}

fn ensure_section(lines: &mut Vec<String>) -> (bool, usize, usize) {
    if let Some((header, start, end)) = section_bounds(lines, "Default Applications") {
        let _ = header;
        return (false, start, end);
    }
    if lines.last().is_some_and(|line| !line.is_empty()) {
        lines.push(String::new());
    }
    lines.push("[Default Applications]".to_owned());
    let start = lines.len();
    (true, start, start)
}

fn section_bounds(lines: &[String], name: &str) -> Option<(usize, usize, usize)> {
    let header = format!("[{name}]");
    let header_at = lines.iter().position(|line| line.trim() == header)?;
    let end = lines
        .iter()
        .enumerate()
        .skip(header_at + 1)
        .find(|(_, line)| {
            let trimmed = line.trim();
            trimmed.starts_with('[') && trimmed.ends_with(']')
        })
        .map_or(lines.len(), |(index, _)| index);
    Some((header_at, header_at + 1, end))
}

fn find_key(lines: &[String], start: usize, end: usize, mime: &str) -> Option<usize> {
    lines
        .iter()
        .enumerate()
        .take(end)
        .skip(start)
        .find_map(|(index, line)| {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                return None;
            }
            let (key, _) = trimmed.split_once('=')?;
            (key.trim() == mime).then_some(index)
        })
}

fn value_of(line: &str) -> Option<String> {
    line.trim()
        .split_once('=')
        .map(|(_, value)| value.to_owned())
}

fn insert_at(lines: &[String], start: usize, end: usize) -> usize {
    let mut at = end;
    while at > start && lines[at - 1].trim().is_empty() {
        at -= 1;
    }
    at
}

fn section_has_no_keys(lines: &[String]) -> bool {
    let Some((_, start, end)) = section_bounds(lines, "Default Applications") else {
        return true;
    };
    !lines.iter().take(end).skip(start).any(|line| {
        let trimmed = line.trim();
        !trimmed.is_empty() && !trimmed.starts_with('#') && trimmed.contains('=')
    })
}

#[derive(Debug, Serialize, Deserialize)]
struct LinuxSaved {
    files: Vec<LinuxFileSaved>,
}

#[derive(Debug, Serialize, Deserialize)]
struct LinuxFileSaved {
    path: String,
    created_section: bool,
    entries: Vec<LinuxEntrySaved>,
}

#[derive(Debug, Serialize, Deserialize)]
struct LinuxEntrySaved {
    mime: String,
    previous: Option<String>,
}

/// Linux association backend rooted at [`LinuxDirs`].
#[derive(Debug, Clone)]
pub struct LinuxAssoc {
    /// XDG directories this backend reads and writes.
    pub dirs: LinuxDirs,
}

impl LinuxAssoc {
    /// Backend for the process environment.
    pub fn from_env() -> Result<Self> {
        Ok(Self {
            dirs: LinuxDirs::from_env()?,
        })
    }

    /// Resolves each format to Default, another app, Available, or Unregistered.
    pub fn status(&self, formats: &[FormatAssoc]) -> Result<Vec<(FormatAssoc, AssocStatus)>> {
        let installed = self.desktop_path().is_file();
        let mut out = Vec::new();
        for fmt in formats {
            let status = if let Some(id) = self.resolved_default(fmt.mime)? {
                if id == DESKTOP_ID {
                    AssocStatus::Default
                } else {
                    AssocStatus::OtherDefault(id)
                }
            } else if installed {
                AssocStatus::Available
            } else {
                AssocStatus::Unregistered
            };
            out.push((*fmt, status));
        }
        Ok(out)
    }

    /// Writes the desktop file and icon. Does not edit mimeapps lists.
    pub fn register_available(&self, app: &AppInstall, formats: &[FormatAssoc]) -> Result<()> {
        let desktop = self.desktop_path();
        let text = desktop_entry(&path_to_string(&app.exe)?, formats);
        atomic_write(&desktop, text.as_bytes())?;
        if let Some(icon) = &app.icon {
            let bytes = fs::read(icon)?;
            atomic_write(&self.icon_path(), &bytes)?;
        }
        spawn_update_desktop_database(desktop.parent());
        Ok(())
    }

    /// Sets the checked types as the default and returns the previous values.
    pub fn make_default(
        &self,
        _app: &AppInstall,
        formats: &[FormatAssoc],
    ) -> Result<MakeDefaultResult> {
        let mimes = crate::format::mime_types(formats);
        let mut files = Vec::new();
        for path in self.user_lists() {
            let original = if path.is_file() {
                fs::read_to_string(&path)?
            } else {
                String::new()
            };
            let edit = edit_mimeapps(&original, DESKTOP_ID, &mimes);
            atomic_write(&path, edit.text.as_bytes())?;
            files.push(LinuxFileSaved {
                path: path_to_string(&path)?,
                created_section: edit.created_section,
                entries: edit
                    .previous
                    .into_iter()
                    .map(|entry| LinuxEntrySaved {
                        mime: entry.mime,
                        previous: entry.previous,
                    })
                    .collect(),
            });
        }
        let payload = serde_json::to_string(&LinuxSaved { files }).map_err(|error| {
            AssocError::Invalid(format!("could not store previous defaults: {error}"))
        })?;
        Ok(MakeDefaultResult {
            outcome: MakeDefaultOutcome::Done,
            saved: Some(SavedDefaults { payload }),
        })
    }

    /// Writes the captured defaults back.
    pub fn restore_previous(&self, saved: &SavedDefaults) -> Result<()> {
        let parsed: LinuxSaved = serde_json::from_str(&saved.payload).map_err(|error| {
            AssocError::Invalid(format!("saved defaults are not readable: {error}"))
        })?;
        for file in parsed.files {
            let path = PathBuf::from(&file.path);
            let original = if path.is_file() {
                fs::read_to_string(&path)?
            } else {
                String::new()
            };
            let previous = file
                .entries
                .into_iter()
                .map(|entry| MimePrevious {
                    mime: entry.mime,
                    previous: entry.previous,
                })
                .collect::<Vec<_>>();
            let text = restore_mimeapps(&original, file.created_section, &previous);
            if text.is_empty() {
                if path.is_file() {
                    fs::remove_file(&path)?;
                }
            } else {
                atomic_write(&path, text.as_bytes())?;
            }
        }
        Ok(())
    }

    /// Removes the desktop file, the icon, and mimeapps entries that name us.
    pub fn unregister(&self) -> Result<()> {
        remove_if_file(&self.desktop_path())?;
        remove_if_file(&self.icon_path())?;
        for path in self.user_lists() {
            if !path.is_file() {
                continue;
            }
            let original = fs::read_to_string(&path)?;
            let text = strip_desktop_id(&original, DESKTOP_ID);
            if text.is_empty() {
                fs::remove_file(&path)?;
            } else {
                atomic_write(&path, text.as_bytes())?;
            }
        }
        Ok(())
    }

    fn desktop_path(&self) -> PathBuf {
        self.dirs.data_home.join("applications").join(DESKTOP_ID)
    }

    fn icon_path(&self) -> PathBuf {
        self.dirs
            .data_home
            .join("icons/hicolor/512x512/apps")
            .join(format!("{ICON_NAME}.png"))
    }

    fn user_lists(&self) -> Vec<PathBuf> {
        let mut files = vec![self.dirs.config_home.join("mimeapps.list")];
        for desktop in &self.dirs.desktops {
            let path = self
                .dirs
                .config_home
                .join(format!("{desktop}-mimeapps.list"));
            if path.is_file() {
                files.push(path);
            }
        }
        files
    }

    fn resolved_default(&self, mime: &str) -> Result<Option<String>> {
        for path in self.status_files() {
            if !path.is_file() {
                continue;
            }
            let text = fs::read_to_string(&path)?;
            if let Some(value) = default_in(&text, mime)
                && let Some(id) = first_desktop(value)
            {
                return Ok(Some(id.to_owned()));
            }
        }
        Ok(None)
    }

    fn status_files(&self) -> Vec<PathBuf> {
        let mut files = Vec::new();
        for desktop in &self.dirs.desktops {
            files.push(
                self.dirs
                    .config_home
                    .join(format!("{desktop}-mimeapps.list")),
            );
            for dir in &self.dirs.config_dirs {
                files.push(dir.join(format!("{desktop}-mimeapps.list")));
            }
        }
        files.push(self.dirs.config_home.join("mimeapps.list"));
        for dir in &self.dirs.config_dirs {
            files.push(dir.join("mimeapps.list"));
        }
        files
    }
}

fn default_in<'a>(text: &'a str, mime: &str) -> Option<&'a str> {
    let mut in_section = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with('[') && trimmed.ends_with(']') {
            in_section = trimmed == "[Default Applications]";
            continue;
        }
        if !in_section || trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let Some((key, value)) = trimmed.split_once('=') else {
            continue;
        };
        if key.trim() == mime {
            return Some(value.trim());
        }
    }
    None
}

fn first_desktop(value: &str) -> Option<&str> {
    value
        .split(';')
        .map(str::trim)
        .find(|part| !part.is_empty())
}

fn path_to_string(path: &Path) -> Result<String> {
    path.to_str()
        .map(str::to_owned)
        .ok_or_else(|| AssocError::Invalid("path is not utf-8".into()))
}

fn atomic_write(path: &Path, bytes: &[u8]) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, bytes)?;
    if let Err(error) = fs::rename(&tmp, path) {
        let _ = fs::remove_file(&tmp);
        return Err(AssocError::Io(error));
    }
    Ok(())
}

fn remove_if_file(path: &Path) -> Result<()> {
    if path.is_file() {
        fs::remove_file(path)?;
    }
    Ok(())
}

fn spawn_update_desktop_database(applications: Option<&Path>) {
    let Some(dir) = applications else {
        return;
    };
    let dir = dir.to_owned();
    std::thread::spawn(move || {
        match std::process::Command::new("update-desktop-database")
            .arg(&dir)
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
        {
            Ok(status) if status.success() => {}
            Ok(status) => {
                tracing::debug!(%status, "update-desktop-database did not succeed");
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                tracing::debug!(%error, "update-desktop-database was not run");
            }
        }
    });
}

/// Executable path: `$APPIMAGE` when set, otherwise `current_exe`.
pub fn current_exec() -> Result<PathBuf> {
    if let Some(appimage) = std::env::var_os("APPIMAGE")
        && !appimage.is_empty()
    {
        return Ok(PathBuf::from(appimage));
    }
    std::env::current_exe().map_err(AssocError::Io)
}
