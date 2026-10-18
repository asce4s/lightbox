//! Windows per-user association plans and the HKCU executor (SPEC-VIEWER §V10.3).

#[cfg(windows)]
use crate::AssocError;
use crate::format::progid_family;
use crate::{FormatAssoc, Result};
use std::collections::BTreeMap;

/// Display name written into `FriendlyAppName`.
pub const FRIENDLY_APP_NAME: &str = "Lightbox Photo Viewer";

/// A registry value to create or replace.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RegWrite {
    /// Key under `HKCU`, without the hive prefix.
    pub key: String,
    /// Value name. Empty is the key's default value.
    pub name: String,
    /// Data stored for `name`.
    pub data: RegData,
}

/// Registry data the association plan writes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RegData {
    /// `REG_SZ`.
    Sz(String),
    /// `REG_NONE` with an empty payload (OpenWithProgids / SupportedTypes).
    None,
}

impl RegData {
    fn kind(&self) -> &'static str {
        match self {
            Self::Sz(_) => "SZ",
            Self::None => "NONE",
        }
    }

    fn text(&self) -> &str {
        match self {
            Self::Sz(text) => text,
            Self::None => "",
        }
    }
}

/// The list of HKCU changes for one association action.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WindowsPlan {
    /// `Lightbox` in production, `LightboxTest` in the HKCU round-trip.
    pub app_key: String,
    /// Values to write. Unregister deletes exactly these, plus [`Self::owned_keys`].
    pub writes: Vec<RegWrite>,
    /// Keys this install owns outright (ProgID trees, `Applications\lightbox.exe`, `Software\<app>`).
    pub owned_keys: Vec<String>,
    /// `SHChangeNotify(SHCNE_ASSOCCHANGED)` after a successful apply.
    pub notify: bool,
    /// Settings page opened by `make_default` / restore. Empty for the register plan.
    pub settings_uri: Option<String>,
}

impl WindowsPlan {
    /// Manifest payload stored at `Software\<app>\AssocManifest`.
    pub fn manifest(&self) -> String {
        let mut lines = Vec::new();
        for write in &self.writes {
            lines.push(format!(
                "V\t{}\t{}\t{}\t{}",
                write.key,
                write.name,
                write.data.kind(),
                write.data.text()
            ));
        }
        for key in &self.owned_keys {
            lines.push(format!("K\t{key}"));
        }
        lines.join("\n")
    }

    /// Stable text form used by the plan snapshot.
    pub fn render(&self) -> String {
        let mut out = String::new();
        out.push_str(&format!("app_key: {}\n", self.app_key));
        out.push_str(&format!("notify: {}\n", self.notify));
        if let Some(uri) = &self.settings_uri {
            out.push_str(&format!("settings: {uri}\n"));
        }
        out.push_str("writes:\n");
        for write in &self.writes {
            let name = if write.name.is_empty() {
                "(Default)"
            } else {
                write.name.as_str()
            };
            out.push_str(&format!("  {}\\{name} = {}", write.key, write.data.kind()));
            if !write.data.text().is_empty() {
                out.push(' ');
                out.push_str(write.data.text());
            }
            out.push('\n');
        }
        out.push_str("owned:\n");
        for key in &self.owned_keys {
            out.push_str(&format!("  {key}\n"));
        }
        out.push_str("manifest:\n");
        for line in self.manifest().lines() {
            out.push_str(&format!("  {line}\n"));
        }
        out
    }
}

/// `ms-settings` page for this registration. Windows 11 can deep-link to the app.
pub fn settings_uri(app_key: &str, windows_11: bool) -> String {
    if windows_11 {
        format!("ms-settings:defaultapps?registeredAppUser={app_key}")
    } else {
        "ms-settings:defaultapps".to_owned()
    }
}

/// Plan that lists Lightbox as an available handler. It does not touch `UserChoice`.
pub fn register_plan(
    exe: &str,
    exe_name: &str,
    app_key: &str,
    formats: &[FormatAssoc],
) -> WindowsPlan {
    let mut writes = Vec::new();
    let mut owned_keys = Vec::new();
    let command = open_command(exe);
    let icon = format!("\"{exe}\",0");
    for (family, label) in families(formats) {
        let progid = format!("{app_key}.Image.{family}");
        let root = format!(r"Software\Classes\{progid}");
        writes.push(sz(&root, "", &format!("{label} (Lightbox)")));
        writes.push(sz(&root, "FriendlyTypeName", label));
        writes.push(sz(&format!(r"{root}\DefaultIcon"), "", &icon));
        writes.push(sz(
            &format!(r"{root}\shell\open"),
            "FriendlyAppName",
            FRIENDLY_APP_NAME,
        ));
        writes.push(sz(&format!(r"{root}\shell\open\command"), "", &command));
        owned_keys.push(root);
    }
    for fmt in formats {
        let family = progid_family(fmt.ext);
        let progid = format!("{app_key}.Image.{family}");
        writes.push(none(
            &format!("Software\\Classes\\.{}\\OpenWithProgids", fmt.ext),
            &progid,
        ));
    }
    let apps = format!(r"Software\Classes\Applications\{exe_name}");
    writes.push(sz(&apps, "FriendlyAppName", FRIENDLY_APP_NAME));
    for fmt in formats {
        writes.push(none(
            &format!(r"{apps}\SupportedTypes"),
            &format!(".{}", fmt.ext),
        ));
    }
    writes.push(sz(&format!(r"{apps}\shell\open\command"), "", &command));
    owned_keys.push(apps);

    let caps = format!(r"Software\{app_key}\Capabilities");
    writes.push(sz(&caps, "ApplicationName", FRIENDLY_APP_NAME));
    writes.push(sz(
        &caps,
        "ApplicationDescription",
        "View and organise photos",
    ));
    for fmt in formats {
        let family = progid_family(fmt.ext);
        writes.push(sz(
            &format!(r"{caps}\FileAssociations"),
            &format!(".{}", fmt.ext),
            &format!("{app_key}.Image.{family}"),
        ));
    }
    writes.push(sz(
        r"Software\RegisteredApplications",
        app_key,
        &format!(r"Software\{app_key}\Capabilities"),
    ));
    owned_keys.push(format!(r"Software\{app_key}"));

    WindowsPlan {
        app_key: app_key.to_owned(),
        writes,
        owned_keys,
        notify: true,
        settings_uri: None,
    }
}

/// Plan that opens Default Apps. `UserChoice` is never written.
pub fn make_default_plan(app_key: &str, windows_11: bool) -> WindowsPlan {
    WindowsPlan {
        app_key: app_key.to_owned(),
        writes: Vec::new(),
        owned_keys: Vec::new(),
        notify: false,
        settings_uri: Some(settings_uri(app_key, windows_11)),
    }
}

fn open_command(exe: &str) -> String {
    format!("\"{exe}\" view \"%1\"")
}

fn sz(key: &str, name: &str, data: &str) -> RegWrite {
    RegWrite {
        key: key.to_owned(),
        name: name.to_owned(),
        data: RegData::Sz(data.to_owned()),
    }
}

fn none(key: &str, name: &str) -> RegWrite {
    RegWrite {
        key: key.to_owned(),
        name: name.to_owned(),
        data: RegData::None,
    }
}

fn families(formats: &[FormatAssoc]) -> Vec<(&'static str, &'static str)> {
    let mut out = Vec::new();
    for fmt in formats {
        let family = progid_family(fmt.ext);
        if !out.iter().any(|(existing, _)| *existing == family) {
            out.push((family, fmt.label));
        }
    }
    out
}

/// In-memory HKCU used by the unregister test on every OS.
#[derive(Debug, Default, Clone)]
pub struct MemoryRegistry {
    values: BTreeMap<String, BTreeMap<String, RegData>>,
    /// Set when a plan asks for `SHChangeNotify`.
    pub notified: bool,
}

impl MemoryRegistry {
    /// Empty hive.
    pub fn new() -> Self {
        Self::default()
    }

    /// Stores one value, creating the key if needed.
    pub fn set(&mut self, key: &str, name: &str, data: RegData) {
        self.values
            .entry(key.to_owned())
            .or_default()
            .insert(name.to_owned(), data);
    }

    /// Reads one value.
    pub fn get(&self, key: &str, name: &str) -> Option<&RegData> {
        self.values.get(key).and_then(|map| map.get(name))
    }

    /// Applies `plan` and records its manifest.
    pub fn apply(&mut self, plan: &WindowsPlan) -> Result<()> {
        for write in &plan.writes {
            self.set(&write.key, &write.name, write.data.clone());
        }
        if !plan.app_key.is_empty() && (!plan.writes.is_empty() || !plan.owned_keys.is_empty()) {
            let key = format!(r"Software\{}\AssocManifest", plan.app_key);
            self.set(&key, "", RegData::Sz(plan.manifest()));
        }
        if plan.notify {
            self.notified = true;
        }
        Ok(())
    }

    /// Deletes only the keys and values listed in the manifest for `app_key`.
    pub fn unregister(&mut self, app_key: &str) -> Result<()> {
        let key = format!(r"Software\{app_key}\AssocManifest");
        let Some(RegData::Sz(text)) = self.get(&key, "").cloned() else {
            return Ok(());
        };
        apply_manifest(self, &text);
        self.notified = true;
        Ok(())
    }
}

fn apply_manifest(reg: &mut MemoryRegistry, text: &str) {
    let mut trees = Vec::new();
    for line in text.lines() {
        let mut parts = line.splitn(5, '\t');
        match parts.next() {
            Some("V") => {
                let Some(key) = parts.next() else { continue };
                let Some(name) = parts.next() else { continue };
                if let Some(map) = reg.values.get_mut(key) {
                    map.remove(name);
                }
            }
            Some("K") => {
                if let Some(key) = parts.next() {
                    trees.push(key.to_owned());
                }
            }
            _ => {}
        }
    }
    for key in trees {
        reg.values
            .retain(|existing, _| existing != &key && !existing.starts_with(&format!("{key}\\")));
    }
}

/// ProgId our register plan writes for `ext`.
pub fn progid_for(app_key: &str, ext: &str) -> String {
    format!("{app_key}.Image.{}", progid_family(ext))
}

/// Reads `UserChoice\ProgId` for one extension from a memory hive.
pub fn memory_status(reg: &MemoryRegistry, app_key: &str, ext: &str) -> crate::AssocStatus {
    let key =
        format!(r"Software\Microsoft\Windows\CurrentVersion\Explorer\FileExts\.{ext}\UserChoice");
    let ours = progid_for(app_key, ext);
    match reg.get(&key, "ProgId") {
        Some(RegData::Sz(progid)) if progid == &ours => crate::AssocStatus::Default,
        Some(RegData::Sz(progid)) => crate::AssocStatus::OtherDefault(progid.clone()),
        _ => {
            let open_with = format!(r"Software\Classes\.{ext}\OpenWithProgids");
            if reg.get(&open_with, &ours).is_some() {
                crate::AssocStatus::Available
            } else {
                crate::AssocStatus::Unregistered
            }
        }
    }
}

/// HKCU executor. Compiled on Windows; the plan tests use [`MemoryRegistry`] everywhere.
#[cfg(windows)]
pub struct Hkcu;

#[cfg(windows)]
impl Hkcu {
    /// Applies `plan` under `HKEY_CURRENT_USER` and notifies the shell.
    pub fn apply(plan: &WindowsPlan) -> Result<()> {
        for write in &plan.writes {
            write_value(&write.key, &write.name, &write.data)?;
        }
        if !plan.app_key.is_empty() && (!plan.writes.is_empty() || !plan.owned_keys.is_empty()) {
            let key = format!(r"Software\{}\AssocManifest", plan.app_key);
            write_value(&key, "", &RegData::Sz(plan.manifest()))?;
        }
        if plan.notify {
            notify_assoc_changed();
        }
        Ok(())
    }

    /// Removes manifest entries for `app_key` and notifies the shell.
    pub fn unregister(app_key: &str) -> Result<()> {
        let key = format!(r"Software\{app_key}\AssocManifest");
        let Some(text) = read_sz(&key, "")? else {
            return Ok(());
        };
        for line in text.lines() {
            let mut parts = line.splitn(5, '\t');
            match parts.next() {
                Some("V") => {
                    let Some(reg_key) = parts.next() else {
                        continue;
                    };
                    let Some(name) = parts.next() else { continue };
                    delete_value(reg_key, name)?;
                }
                Some("K") => {
                    if let Some(reg_key) = parts.next() {
                        delete_tree(reg_key)?;
                    }
                }
                _ => {}
            }
        }
        notify_assoc_changed();
        Ok(())
    }

    /// Reads a `REG_SZ` value. Missing keys are `Ok(None)`.
    pub fn read_sz(key: &str, name: &str) -> Result<Option<String>> {
        read_sz(key, name)
    }

    /// Whether `name` exists under `key`, regardless of its type.
    pub fn has_value(key: &str, name: &str) -> Result<bool> {
        let Some(reg) = open_key(key, false)? else {
            return Ok(false);
        };
        match reg.get_raw_value(name) {
            Ok(_) => Ok(true),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
            Err(error) => Err(AssocError::Io(error)),
        }
    }

    /// Writes a `REG_SZ` value, creating intermediate keys.
    pub fn write_sz(key: &str, name: &str, data: &str) -> Result<()> {
        write_value(key, name, &RegData::Sz(data.to_owned()))
    }

    /// Deletes one value. A missing key or value is success.
    pub fn delete_value(key: &str, name: &str) -> Result<()> {
        delete_value(key, name)
    }

    /// Deletes a key tree. A missing key is success.
    pub fn delete_tree(key: &str) -> Result<()> {
        delete_tree(key)
    }

    /// Opens the Default Apps settings page.
    pub fn open_settings(uri: &str) -> Result<()> {
        let status = std::process::Command::new("cmd")
            .args(["/C", "start", "", uri])
            .status()?;
        if status.success() {
            Ok(())
        } else {
            Err(AssocError::Invalid(format!(
                "settings page did not open ({status})"
            )))
        }
    }
}

#[cfg(windows)]
fn hive() -> winreg::RegKey {
    winreg::RegKey::predef(winreg::enums::HKEY_CURRENT_USER)
}

#[cfg(windows)]
fn open_key(path: &str, create: bool) -> Result<Option<winreg::RegKey>> {
    use winreg::enums::KEY_ALL_ACCESS;
    let hkcu = hive();
    if path.is_empty() {
        return Ok(Some(hkcu));
    }
    if create {
        let mut current = hkcu;
        for part in path.split('\\') {
            if part.is_empty() {
                continue;
            }
            current = current.create_subkey(part)?.0;
        }
        return Ok(Some(current));
    }
    match hkcu.open_subkey_with_flags(path, KEY_ALL_ACCESS) {
        Ok(key) => Ok(Some(key)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(AssocError::Io(error)),
    }
}

#[cfg(windows)]
fn write_value(path: &str, name: &str, data: &RegData) -> Result<()> {
    let Some(key) = open_key(path, true)? else {
        return Err(AssocError::Invalid(format!("could not create {path}")));
    };
    match data {
        RegData::Sz(text) => key.set_value(name, text)?,
        RegData::None => {
            key.set_raw_value(
                name,
                &winreg::RegValue {
                    bytes: std::borrow::Cow::Owned(Vec::new()),
                    vtype: winreg::enums::RegType::REG_NONE,
                },
            )?;
        }
    }
    Ok(())
}

#[cfg(windows)]
fn read_sz(path: &str, name: &str) -> Result<Option<String>> {
    let Some(key) = open_key(path, false)? else {
        return Ok(None);
    };
    match key.get_value::<String, _>(name) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(AssocError::Io(error)),
    }
}

#[cfg(windows)]
fn delete_value(path: &str, name: &str) -> Result<()> {
    let Some(key) = open_key(path, false)? else {
        return Ok(());
    };
    match key.delete_value(name) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AssocError::Io(error)),
    }
}

#[cfg(windows)]
fn delete_tree(path: &str) -> Result<()> {
    match hive().delete_subkey_all(path) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(error) => Err(AssocError::Io(error)),
    }
}

#[cfg(windows)]
fn notify_assoc_changed() {
    // SAFETY: SHChangeNotify with SHCNE_ASSOCCHANGED and null item pointers is the
    // documented way to refresh file associations. Both pointers are unused for this event.
    #[allow(unsafe_code)]
    unsafe {
        #[link(name = "shell32")]
        unsafe extern "system" {
            fn SHChangeNotify(
                event: u32,
                flags: u32,
                item1: *const std::ffi::c_void,
                item2: *const std::ffi::c_void,
            );
        }
        SHChangeNotify(0x0800_0000, 0, std::ptr::null(), std::ptr::null());
    }
}
