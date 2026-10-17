//! `cargo xtask package`: release archives per operating system (SPEC §13).
//!
//! Linux gets a `.tar.gz` and an AppDir (turned into an AppImage when `appimagetool` is on
//! `PATH`), macOS an unsigned `.app` bundle in a `.tar.gz` (plus a `.dmg` when `hdiutil` exists),
//! Windows a `.zip`.

use crate::icon;
use anyhow::{Context, Result, bail};
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// Bundle identifier of the macOS app.
pub const BUNDLE_ID: &str = "app.lightbox.Lightbox";

/// Binaries shipped in every package: the GUI and the command line.
pub const BINARIES: [&str; 2] = ["lightbox", "lightbox-cli"];

/// Files copied next to the binaries when they exist in the repository.
pub const DOCS: [&str; 2] = ["README.md", "LICENSE"];

/// Version, OS and CPU of a package.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Target {
    /// `linux`, `macos` or `windows`.
    pub os: String,
    /// CPU architecture as Rust names it (`x86_64`, `aarch64`).
    pub arch: String,
    /// Package version.
    pub version: String,
}

impl Target {
    /// The machine running `xtask`.
    pub fn host() -> Self {
        Self {
            os: std::env::consts::OS.to_owned(),
            arch: std::env::consts::ARCH.to_owned(),
            version: env!("CARGO_PKG_VERSION").to_owned(),
        }
    }

    /// `lightbox-<version>-<os>-<arch>`, the stem of every artifact.
    pub fn stem(&self) -> String {
        format!("lightbox-{}-{}-{}", self.version, self.os, self.arch)
    }

    fn exe(&self, name: &str) -> String {
        if self.os == "windows" {
            format!("{name}.exe")
        } else {
            name.to_owned()
        }
    }

    /// File name of the archive for this target.
    pub fn archive_name(&self) -> String {
        let ext = if self.os == "windows" {
            "zip"
        } else {
            "tar.gz"
        };
        format!("{}.{ext}", self.stem())
    }
}

/// `Contents/Info.plist` of the macOS bundle.
pub fn info_plist(version: &str) -> String {
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleName</key><string>Lightbox</string>
  <key>CFBundleDisplayName</key><string>Lightbox</string>
  <key>CFBundleIdentifier</key><string>{BUNDLE_ID}</string>
  <key>CFBundleExecutable</key><string>lightbox</string>
  <key>CFBundleIconFile</key><string>icon.png</string>
  <key>CFBundlePackageType</key><string>APPL</string>
  <key>CFBundleShortVersionString</key><string>{version}</string>
  <key>CFBundleVersion</key><string>{version}</string>
  <key>LSMinimumSystemVersion</key><string>13.0</string>
  <key>NSHighResolutionCapable</key><true/>
  <key>LSApplicationCategoryType</key><string>public.app-category.photography</string>
</dict>
</plist>
"#
    )
}

/// The freedesktop entry used by the Linux archive and the AppDir.
pub fn desktop_entry() -> String {
    "[Desktop Entry]\nType=Application\nName=Lightbox\nComment=Photo manager\nExec=lightbox\nIcon=lightbox\nCategories=Graphics;Photography;Viewer;\nTerminal=false\n".to_owned()
}

fn copy_exec(from: &Path, to: &Path) -> Result<()> {
    fs::copy(from, to).with_context(|| format!("copying {}", from.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(to, fs::Permissions::from_mode(0o755))?;
    }
    Ok(())
}

fn copy_docs(repo: &Path, to: &Path) -> Result<()> {
    for doc in DOCS {
        let from = repo.join(doc);
        if from.is_file() {
            fs::copy(&from, to.join(doc))?;
        }
    }
    Ok(())
}

fn write_icon(path: &Path, size: u32) -> Result<()> {
    icon::render(size)
        .save(path)
        .with_context(|| format!("writing {}", path.display()))
}

/// Lays out the contents of the archive for `target` under `out/<stem>/` and returns that path.
pub fn stage_archive(target: &Target, repo: &Path, bin_dir: &Path, out: &Path) -> Result<PathBuf> {
    let dir = out.join(target.stem());
    fs::create_dir_all(&dir)?;
    for bin in BINARIES {
        let name = target.exe(bin);
        copy_exec(&bin_dir.join(&name), &dir.join(&name))?;
    }
    copy_docs(repo, &dir)?;
    write_icon(&dir.join("lightbox.png"), 512)?;
    if target.os == "linux" {
        fs::write(dir.join("lightbox.desktop"), desktop_entry())?;
    }
    Ok(dir)
}

/// Builds `out/Lightbox.app` with the GUI binary, the CLI next to it and the icon.
pub fn stage_app_bundle(target: &Target, bin_dir: &Path, out: &Path) -> Result<PathBuf> {
    let app = out.join("Lightbox.app");
    let macos = app.join("Contents/MacOS");
    let resources = app.join("Contents/Resources");
    fs::create_dir_all(&macos)?;
    fs::create_dir_all(&resources)?;
    for bin in BINARIES {
        copy_exec(&bin_dir.join(bin), &macos.join(bin))?;
    }
    fs::write(app.join("Contents/Info.plist"), info_plist(&target.version))?;
    write_icon(&resources.join("icon.png"), 512)?;
    Ok(app)
}

/// Builds `out/Lightbox.AppDir`, ready for `appimagetool`.
pub fn stage_appdir(bin_dir: &Path, out: &Path) -> Result<PathBuf> {
    let dir = out.join("Lightbox.AppDir");
    let bin = dir.join("usr/bin");
    fs::create_dir_all(&bin)?;
    for name in BINARIES {
        copy_exec(&bin_dir.join(name), &bin.join(name))?;
    }
    fs::write(dir.join("lightbox.desktop"), desktop_entry())?;
    write_icon(&dir.join("lightbox.png"), 512)?;
    let apprun = dir.join("AppRun");
    fs::write(
        &apprun,
        "#!/bin/sh\nHERE=\"$(dirname \"$(readlink -f \"$0\")\")\"\nexec \"$HERE/usr/bin/lightbox\" \"$@\"\n",
    )?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&apprun, fs::Permissions::from_mode(0o755))?;
    }
    Ok(dir)
}

/// Packs `dir` (a folder inside `out`) into `out/<archive>` with the system `tar`
/// (`.zip` through bsdtar on Windows).
pub fn make_archive(out: &Path, dir_name: &str, archive: &str) -> Result<PathBuf> {
    let path = out.join(archive);
    let mut cmd = Command::new("tar");
    if archive.ends_with(".zip") {
        cmd.args(["-a", "-c", "-f"]);
    } else {
        cmd.args(["-czf"]);
    }
    let status = cmd
        .arg(&path)
        .arg("-C")
        .arg(out)
        .arg(dir_name)
        .status()
        .context("could not run tar")?;
    if !status.success() {
        bail!("tar failed ({status})");
    }
    Ok(path)
}

fn on_path(tool: &str) -> bool {
    std::env::var_os("PATH")
        .is_some_and(|paths| std::env::split_paths(&paths).any(|dir| dir.join(tool).is_file()))
}

fn run_cargo_build(repo: &Path) -> Result<PathBuf> {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let status = Command::new(cargo)
        .current_dir(repo)
        .args([
            "build",
            "--release",
            "--locked",
            "-p",
            "lightbox",
            "-p",
            "lb-cli",
        ])
        .status()?;
    if !status.success() {
        bail!("release build failed ({status})");
    }
    Ok(repo.join("target/release"))
}

/// Builds the release binaries and packages them for the machine running `xtask`.
pub fn run() -> Result<()> {
    let repo = std::env::current_dir()?;
    let target = Target::host();
    let bin_dir = run_cargo_build(&repo)?;
    let out = repo.join("dist");
    if out.exists() {
        fs::remove_dir_all(&out)?;
    }
    fs::create_dir_all(&out)?;
    let mut artifacts = Vec::new();

    let staged = stage_archive(&target, &repo, &bin_dir, &out)?;
    let dir_name = staged
        .file_name()
        .and_then(|n| n.to_str())
        .context("bad staging directory")?
        .to_owned();
    artifacts.push(make_archive(&out, &dir_name, &target.archive_name())?);

    match target.os.as_str() {
        "linux" => {
            let appdir = stage_appdir(&bin_dir, &out)?;
            if on_path("appimagetool") {
                let image = out.join(format!("{}.AppImage", target.stem()));
                let status = Command::new("appimagetool")
                    .env("ARCH", &target.arch)
                    .arg(&appdir)
                    .arg(&image)
                    .status()?;
                if status.success() {
                    artifacts.push(image);
                } else {
                    eprintln!("appimagetool failed ({status}); AppImage skipped");
                }
            } else {
                println!(
                    "appimagetool not found; AppDir left in {}",
                    appdir.display()
                );
            }
        }
        "macos" => {
            let app = stage_app_bundle(&target, &bin_dir, &out)?;
            artifacts.push(make_archive(
                &out,
                "Lightbox.app",
                &format!("{}-app.tar.gz", target.stem()),
            )?);
            if on_path("hdiutil") {
                let dmg = out.join(format!("{}.dmg", target.stem()));
                let status = Command::new("hdiutil")
                    .args(["create", "-volname", "Lightbox", "-srcfolder"])
                    .arg(&app)
                    .args(["-ov", "-format", "UDZO"])
                    .arg(&dmg)
                    .status()?;
                if status.success() {
                    artifacts.push(dmg);
                } else {
                    eprintln!("hdiutil failed ({status}); dmg skipped");
                }
            }
        }
        _ => {}
    }
    for artifact in artifacts {
        println!("packaged {}", artifact.display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_binaries(os: &str) -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let t = Target {
            os: os.into(),
            arch: "x86_64".into(),
            version: "1.2.3".into(),
        };
        for bin in BINARIES {
            fs::write(dir.path().join(t.exe(bin)), b"#!/bin/sh\n").unwrap();
        }
        dir
    }

    fn target(os: &str) -> Target {
        Target {
            os: os.into(),
            arch: "aarch64".into(),
            version: "1.2.3".into(),
        }
    }

    #[test]
    fn artifact_names_follow_os_conventions() {
        assert_eq!(
            target("linux").archive_name(),
            "lightbox-1.2.3-linux-aarch64.tar.gz"
        );
        assert_eq!(
            target("windows").archive_name(),
            "lightbox-1.2.3-windows-aarch64.zip"
        );
        assert_eq!(target("macos").stem(), "lightbox-1.2.3-macos-aarch64");
        assert_eq!(target("windows").exe("lightbox"), "lightbox.exe");
    }

    #[test]
    fn plist_and_desktop_entry_name_the_app() {
        let plist = info_plist("1.2.3");
        assert!(plist.contains("<string>app.lightbox.Lightbox</string>"));
        assert!(plist.contains("<key>CFBundleShortVersionString</key><string>1.2.3</string>"));
        assert!(plist.contains("<key>CFBundleExecutable</key><string>lightbox</string>"));
        let desktop = desktop_entry();
        assert!(desktop.contains("Exec=lightbox") && desktop.contains("Icon=lightbox"));
    }

    #[test]
    fn linux_package_contains_binaries_icon_and_desktop_file() {
        let bins = fake_binaries("linux");
        let repo = tempfile::tempdir().unwrap();
        fs::write(repo.path().join("LICENSE"), "MIT").unwrap();
        let out = tempfile::tempdir().unwrap();
        let dir = stage_archive(&target("linux"), repo.path(), bins.path(), out.path()).unwrap();
        for name in [
            "lightbox",
            "lightbox-cli",
            "LICENSE",
            "lightbox.png",
            "lightbox.desktop",
        ] {
            assert!(dir.join(name).is_file(), "{name} missing");
        }
        assert!(!dir.join("README.md").exists(), "absent docs are skipped");
        let icon = image::open(dir.join("lightbox.png")).unwrap();
        assert_eq!((icon.width(), icon.height()), (512, 512));
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = fs::metadata(dir.join("lightbox"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o111, 0o111);
        }
    }

    #[test]
    fn windows_package_uses_exe_names() {
        let bins = fake_binaries("windows");
        let out = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let dir = stage_archive(&target("windows"), repo.path(), bins.path(), out.path()).unwrap();
        assert!(dir.join("lightbox.exe").is_file() && dir.join("lightbox-cli.exe").is_file());
        assert!(!dir.join("lightbox.desktop").exists());
    }

    #[test]
    fn macos_bundle_has_the_required_layout() {
        let bins = fake_binaries("macos");
        let out = tempfile::tempdir().unwrap();
        let app = stage_app_bundle(&target("macos"), bins.path(), out.path()).unwrap();
        for rel in [
            "Contents/Info.plist",
            "Contents/MacOS/lightbox",
            "Contents/MacOS/lightbox-cli",
            "Contents/Resources/icon.png",
        ] {
            assert!(app.join(rel).is_file(), "{rel} missing");
        }
    }

    #[test]
    fn appdir_has_apprun_desktop_file_and_icon() {
        let bins = fake_binaries("linux");
        let out = tempfile::tempdir().unwrap();
        let dir = stage_appdir(bins.path(), out.path()).unwrap();
        for rel in [
            "AppRun",
            "lightbox.desktop",
            "lightbox.png",
            "usr/bin/lightbox",
            "usr/bin/lightbox-cli",
        ] {
            assert!(dir.join(rel).is_file(), "{rel} missing");
        }
        assert!(
            fs::read_to_string(dir.join("AppRun"))
                .unwrap()
                .contains("usr/bin/lightbox")
        );
    }

    #[test]
    fn archive_contains_the_staged_folder() {
        if !on_path("tar") && !on_path("tar.exe") {
            return;
        }
        let bins = fake_binaries("linux");
        let out = tempfile::tempdir().unwrap();
        let repo = tempfile::tempdir().unwrap();
        let t = target("linux");
        let dir = stage_archive(&t, repo.path(), bins.path(), out.path()).unwrap();
        let name = dir.file_name().unwrap().to_str().unwrap().to_owned();
        let archive = make_archive(out.path(), &name, &t.archive_name()).unwrap();
        assert!(fs::metadata(&archive).unwrap().len() > 0);
        let listing = Command::new("tar")
            .arg("-tzf")
            .arg(&archive)
            .output()
            .unwrap();
        let listing = String::from_utf8_lossy(&listing.stdout);
        assert!(
            listing.contains(&format!("{name}/lightbox-cli")),
            "{listing}"
        );
    }
}
