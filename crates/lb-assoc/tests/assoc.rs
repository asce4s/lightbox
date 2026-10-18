//! Association plans and sandboxed executors (SPEC-VIEWER §V10, §V14).

use lb_assoc::{
    AppInstall, AssocBackend, AssocStatus, BUNDLE_ID, DESKTOP_ID, LinuxAssoc, LinuxDirs,
    MakeDefaultOutcome, MemoryRegistry, RegData, base_formats, desktop_entry, edit_mimeapps,
    formats, info_plist, make_default_plan, quote_exec, register_plan, restore_mimeapps,
};
use std::fs;
use std::path::PathBuf;

fn jpeg_only() -> Vec<lb_assoc::FormatAssoc> {
    base_formats()
        .into_iter()
        .filter(|fmt| fmt.ext == "jpg")
        .collect()
}

fn app(exe: &str, icon: Option<PathBuf>) -> AppInstall {
    AppInstall {
        exe: PathBuf::from(exe),
        bundle_id: BUNDLE_ID.to_owned(),
        display_name: "Lightbox Photo Viewer".to_owned(),
        icon,
    }
}

fn status_of<'a>(rows: &'a [(lb_assoc::FormatAssoc, AssocStatus)], ext: &str) -> &'a AssocStatus {
    &rows
        .iter()
        .find(|(fmt, _)| fmt.ext == ext)
        .unwrap_or_else(|| panic!("missing {ext}"))
        .1
}

#[test]
fn assoc_formats_follow_decoder_capabilities() {
    let base = formats(false, false);
    let exts: Vec<_> = base.iter().map(|fmt| fmt.ext).collect();
    assert_eq!(
        exts,
        [
            "jpg", "jpeg", "jpe", "png", "gif", "webp", "bmp", "tif", "tiff"
        ]
    );
    assert!(base.iter().all(|fmt| fmt.mime.starts_with("image/")));
    let jpeg: Vec<_> = base
        .iter()
        .filter(|fmt| fmt.ext == "jpg" || fmt.ext == "jpe")
        .collect();
    assert!(
        jpeg.iter()
            .all(|fmt| fmt.mime == "image/jpeg" && fmt.uti == "public.jpeg")
    );
    assert!(
        base.iter()
            .all(|fmt| !fmt.ext.chars().any(|ch| ch.is_ascii_uppercase()))
    );

    let with_heif = formats(true, false);
    assert!(
        with_heif
            .iter()
            .any(|fmt| fmt.ext == "heic" && fmt.mime == "image/heic" && fmt.uti == "public.heic")
    );
    assert!(
        with_heif
            .iter()
            .any(|fmt| fmt.ext == "heif" && fmt.mime == "image/heif" && fmt.uti == "public.heif")
    );
    assert!(
        with_heif
            .iter()
            .any(|fmt| fmt.ext == "avif" && fmt.mime == "image/avif" && fmt.uti == "public.avif")
    );
    assert!(!with_heif.iter().any(|fmt| fmt.ext == "dng"));

    let with_raw = formats(false, true);
    let raw = [
        ("dng", "image/x-adobe-dng", "com.adobe.raw-image"),
        ("cr2", "image/x-canon-cr2", "com.canon.cr2-raw-image"),
        ("cr3", "image/x-canon-cr3", "com.canon.cr3-raw-image"),
        ("nef", "image/x-nikon-nef", "com.nikon.raw-image"),
        ("arw", "image/x-sony-arw", "com.sony.arw-raw-image"),
        ("orf", "image/x-olympus-orf", "com.olympus.or-raw-image"),
        (
            "rw2",
            "image/x-panasonic-rw2",
            "com.panasonic.rw2-raw-image",
        ),
        ("raf", "image/x-fuji-raf", "com.fuji.raw-image"),
    ];
    for (ext, mime, uti) in raw {
        let fmt = with_raw
            .iter()
            .find(|fmt| fmt.ext == ext)
            .unwrap_or_else(|| panic!("{ext}"));
        assert_eq!((fmt.mime, fmt.uti), (mime, uti));
    }
    assert!(!with_raw.iter().any(|fmt| fmt.ext == "heic"));

    let both = formats(true, true);
    assert_eq!(both.len(), base.len() + 3 + raw.len());
    for video in ["mp4", "mov", "webm", "mkv"] {
        assert!(!both.iter().any(|fmt| fmt.ext == video));
    }
}

#[test]
fn windows_assoc_plan_snapshot() {
    let plan = register_plan(
        r"C:\Program Files\Lightbox\lightbox.exe",
        "lightbox.exe",
        "Lightbox",
        &base_formats(),
    );
    let mut text = plan.render();
    text.push_str("make_default:\n");
    text.push_str(&make_default_plan("Lightbox", true).render());
    assert!(text.contains("ms-settings:defaultapps?registeredAppUser=Lightbox"));
    assert!(text.contains(r#""C:\Program Files\Lightbox\lightbox.exe" view "%1""#));
    assert!(text.contains("notify: true"));
    insta::assert_snapshot!(text);
}

#[test]
fn windows_unregister_removes_only_manifest_entries() {
    let mut reg = MemoryRegistry::new();
    reg.set(
        r"Software\RegisteredApplications",
        "Notepad",
        RegData::Sz(r"Software\Notepad".to_owned()),
    );
    reg.set(
        r"Software\Classes\.jpg\OpenWithProgids",
        "Other.ProgId",
        RegData::None,
    );
    reg.set(
        r"Software\Classes\.jpg",
        "",
        RegData::Sz("Other.ProgId".to_owned()),
    );
    reg.set(r"Software\Keep", "Marker", RegData::Sz("stay".to_owned()));

    let formats: Vec<_> = base_formats()
        .into_iter()
        .filter(|fmt| fmt.ext == "jpg" || fmt.ext == "png")
        .collect();
    let plan = register_plan(
        r"C:\Program Files\Lightbox\lightbox.exe",
        "lightbox.exe",
        "Lightbox",
        &formats,
    );
    reg.apply(&plan).unwrap();
    assert!(
        reg.get(r"Software\Classes\Lightbox.Image.jpeg", "")
            .is_some()
    );
    assert!(
        reg.get(
            r"Software\Classes\.jpg\OpenWithProgids",
            "Lightbox.Image.jpeg"
        )
        .is_some()
    );
    reg.notified = false;
    reg.unregister("Lightbox").unwrap();
    assert!(reg.notified);

    assert!(
        reg.get(r"Software\Classes\Lightbox.Image.jpeg", "")
            .is_none()
    );
    assert!(
        reg.get(r"Software\Classes\Lightbox.Image.png", "FriendlyTypeName")
            .is_none()
    );
    assert!(
        reg.get(
            r"Software\Classes\Applications\lightbox.exe",
            "FriendlyAppName"
        )
        .is_none()
    );
    assert!(
        reg.get(r"Software\Lightbox\Capabilities", "ApplicationName")
            .is_none()
    );
    assert!(reg.get(r"Software\Lightbox\AssocManifest", "").is_none());
    assert!(
        reg.get(r"Software\RegisteredApplications", "Lightbox")
            .is_none()
    );
    assert!(
        reg.get(
            r"Software\Classes\.jpg\OpenWithProgids",
            "Lightbox.Image.jpeg"
        )
        .is_none()
    );

    assert_eq!(
        reg.get(r"Software\RegisteredApplications", "Notepad"),
        Some(&RegData::Sz(r"Software\Notepad".to_owned()))
    );
    assert_eq!(
        reg.get(r"Software\Classes\.jpg\OpenWithProgids", "Other.ProgId"),
        Some(&RegData::None)
    );
    assert_eq!(
        reg.get(r"Software\Classes\.jpg", ""),
        Some(&RegData::Sz("Other.ProgId".to_owned()))
    );
    assert_eq!(
        reg.get(r"Software\Keep", "Marker"),
        Some(&RegData::Sz("stay".to_owned()))
    );
}

#[cfg(windows)]
#[test]
fn windows_registry_roundtrip_hkcu() {
    let app_key = "LightboxTest";
    let formats = jpeg_only();
    let plan = register_plan(
        r"C:\LightboxTest\lightbox-test.exe",
        "lightbox-test.exe",
        app_key,
        &formats,
    );
    struct Guard;
    impl Drop for Guard {
        fn drop(&mut self) {
            let _ = lb_assoc::Hkcu::delete_tree(r"Software\LightboxTest");
            let _ = lb_assoc::Hkcu::delete_tree(r"Software\Classes\LightboxTest.Image.jpeg");
            let _ = lb_assoc::Hkcu::delete_tree(r"Software\Classes\Applications\lightbox-test.exe");
            let _ =
                lb_assoc::Hkcu::delete_value(r"Software\RegisteredApplications", "LightboxTest");
            let _ = lb_assoc::Hkcu::delete_value(
                r"Software\Classes\.jpg\OpenWithProgids",
                "LightboxTest.Image.jpeg",
            );
            let _ = lb_assoc::Hkcu::delete_value(
                r"Software\Classes\.jpg\OpenWithProgids",
                "Other.ProgId",
            );
            let _ = lb_assoc::Hkcu::delete_tree(r"Software\LightboxTestKeep");
        }
    }
    let _guard = Guard;
    lb_assoc::Hkcu::write_sz(r"Software\LightboxTestKeep", "Marker", "stay").unwrap();
    lb_assoc::Hkcu::write_sz(
        r"Software\Classes\.jpg\OpenWithProgids",
        "Other.ProgId",
        "keep",
    )
    .unwrap();
    lb_assoc::Hkcu::apply(&plan).unwrap();
    assert_eq!(
        lb_assoc::Hkcu::read_sz(r"Software\Classes\LightboxTest.Image.jpeg", "")
            .unwrap()
            .as_deref(),
        Some("JPEG image (Lightbox)")
    );
    lb_assoc::Hkcu::unregister(app_key).unwrap();
    assert_eq!(
        lb_assoc::Hkcu::read_sz(r"Software\Classes\LightboxTest.Image.jpeg", "").unwrap(),
        None
    );
    assert_eq!(
        lb_assoc::Hkcu::read_sz(r"Software\LightboxTestKeep", "Marker")
            .unwrap()
            .as_deref(),
        Some("stay")
    );
    assert_eq!(
        lb_assoc::Hkcu::read_sz(r"Software\Classes\.jpg\OpenWithProgids", "Other.ProgId")
            .unwrap()
            .as_deref(),
        Some("keep")
    );
}

#[test]
fn macos_info_plist_document_types_snapshot() {
    let plist = info_plist("1.2.3", &base_formats());
    assert!(plist.contains("<string>public.jpeg</string>"));
    assert!(plist.contains("<string>Alternate</string>"));
    assert!(plist.contains("<string>Viewer</string>"));
    assert!(!plist.contains("public.heic"));
    assert!(!plist.contains("com.adobe.raw-image"));
    let full = info_plist("1.2.3", &formats(true, true));
    assert!(full.contains("public.heic"));
    assert!(full.contains("public.heif"));
    assert!(full.contains("public.avif"));
    assert!(full.contains("com.fuji.raw-image"));
    insta::assert_snapshot!(plist);
}

#[cfg(target_os = "macos")]
#[test]
fn macos_info_plist_lints() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("Info.plist");
    fs::write(&path, info_plist("1.2.3", &formats(true, true))).unwrap();
    let status = std::process::Command::new("plutil")
        .arg("-lint")
        .arg(&path)
        .status()
        .unwrap();
    assert!(status.success());
}

#[test]
fn linux_desktop_entry_snapshot() {
    let text = desktop_entry("/usr/bin/lightbox", &base_formats());
    assert!(text.contains("Name=Lightbox Photo Viewer"));
    assert!(text.contains("Exec=/usr/bin/lightbox view %F"));
    assert!(!text.contains("image/heic"));
    assert!(!text.contains("image/x-adobe-dng"));
    insta::assert_snapshot!(text);
}

#[test]
fn linux_exec_quoting_table() {
    let cases = [
        ("/usr/bin/lightbox", "/usr/bin/lightbox"),
        ("/opt/My Apps/lightbox", "\"/opt/My Apps/lightbox\""),
        ("/opt/a$b/lightbox", "\"/opt/a\\$b/lightbox\""),
        ("/tmp/a%b/lightbox", "/tmp/a%%b/lightbox"),
        ("/tmp/a`b/lightbox", "\"/tmp/a\\`b/lightbox\""),
        ("/tmp/a\\b/lightbox", "\"/tmp/a\\\\b/lightbox\""),
        ("/tmp/a\"b/lightbox", "\"/tmp/a\\\"b/lightbox\""),
    ];
    for (input, expected) in cases {
        assert_eq!(quote_exec(input), expected, "{input}");
    }
}

#[test]
fn mimeapps_list_edit_preserves_other_content() {
    let original = "\
# keep me

[Default Applications]
image/jpeg=eog.desktop;
image/png=gwenview.desktop;

[Added Associations]
image/jpeg=extra.desktop;

[Other]
key=value
";
    let edit = edit_mimeapps(original, DESKTOP_ID, &["image/jpeg", "image/gif"]);
    assert!(!edit.created_section);
    assert_eq!(edit.previous[0].previous.as_deref(), Some("eog.desktop;"));
    assert_eq!(edit.previous[1].previous, None);
    assert_eq!(
        edit.text,
        "\
# keep me

[Default Applications]
image/jpeg=app.lightbox.Viewer.desktop;
image/png=gwenview.desktop;
image/gif=app.lightbox.Viewer.desktop;

[Added Associations]
image/jpeg=extra.desktop;

[Other]
key=value
"
    );
}

#[test]
fn mimeapps_desktop_specific_list_updated() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    fs::create_dir_all(&config).unwrap();
    fs::write(
        config.join("mimeapps.list"),
        "[Default Applications]\nimage/jpeg=eog.desktop;\n",
    )
    .unwrap();
    fs::write(
        config.join("gnome-mimeapps.list"),
        "[Default Applications]\nimage/jpeg=shotwell.desktop;\n",
    )
    .unwrap();
    let assoc = LinuxAssoc {
        dirs: LinuxDirs {
            data_home: root.path().join("data"),
            config_home: config.clone(),
            config_dirs: vec![root.path().join("xdg")],
            desktops: vec!["gnome".to_owned(), "ubuntu".to_owned()],
        },
    };
    let result = assoc
        .make_default(&app("/usr/bin/lightbox", None), &jpeg_only())
        .unwrap();
    assert_eq!(result.outcome, MakeDefaultOutcome::Done);
    let mime = fs::read_to_string(config.join("mimeapps.list")).unwrap();
    let gnome = fs::read_to_string(config.join("gnome-mimeapps.list")).unwrap();
    assert!(mime.contains("image/jpeg=app.lightbox.Viewer.desktop;"));
    assert!(gnome.contains("image/jpeg=app.lightbox.Viewer.desktop;"));
    assert!(!mime.contains("eog.desktop"));
    assert!(!gnome.contains("shotwell.desktop"));
    assert!(!config.join("ubuntu-mimeapps.list").exists());
}

#[test]
fn mimeapps_restore_previous_default() {
    let original = "\
# keep me
[Default Applications]
image/jpeg=eog.desktop;
image/png=eog.desktop;

[Added Associations]
image/jpeg=extra.desktop;
";
    let edit = edit_mimeapps(original, DESKTOP_ID, &["image/jpeg", "image/gif"]);
    assert_eq!(edit.previous[0].previous.as_deref(), Some("eog.desktop;"));
    assert_eq!(edit.previous[1].previous, None);
    assert!(edit.text.contains("image/gif=app.lightbox.Viewer.desktop;"));
    assert!(edit.text.contains("image/png=eog.desktop;"));
    assert!(edit.text.contains("[Added Associations]"));
    let restored = restore_mimeapps(&edit.text, edit.created_section, &edit.previous);
    assert_eq!(restored, original);
}

#[test]
fn linux_status_resolution_order() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    let dir1 = root.path().join("xdg1");
    let dir2 = root.path().join("xdg2");
    for dir in [&config, &dir1, &dir2] {
        fs::create_dir_all(dir).unwrap();
    }
    fs::write(
        config.join("gnome-mimeapps.list"),
        "[Default Applications]\nimage/png=other.desktop;\n",
    )
    .unwrap();
    fs::write(
        config.join("mimeapps.list"),
        "[Default Applications]\nimage/png=app.lightbox.Viewer.desktop;\nimage/jpeg=app.lightbox.Viewer.desktop;\n",
    )
    .unwrap();
    fs::write(
        dir1.join("gnome-mimeapps.list"),
        "[Default Applications]\nimage/tiff=shotwell.desktop;\n",
    )
    .unwrap();
    fs::write(
        dir1.join("mimeapps.list"),
        "[Default Applications]\nimage/webp=other.desktop;\n",
    )
    .unwrap();
    fs::write(
        dir2.join("mimeapps.list"),
        "[Default Applications]\nimage/gif=app.lightbox.Viewer.desktop;\nimage/webp=app.lightbox.Viewer.desktop;\n",
    )
    .unwrap();
    let assoc = LinuxAssoc {
        dirs: LinuxDirs {
            data_home: root.path().join("data"),
            config_home: config,
            config_dirs: vec![dir1, dir2],
            desktops: vec!["gnome".to_owned()],
        },
    };
    let rows = assoc.status(&base_formats()).unwrap();
    assert_eq!(
        status_of(&rows, "png"),
        &AssocStatus::OtherDefault("other.desktop".to_owned())
    );
    assert_eq!(status_of(&rows, "jpg"), &AssocStatus::Default);
    assert_eq!(status_of(&rows, "jpeg"), &AssocStatus::Default);
    assert_eq!(
        status_of(&rows, "tif"),
        &AssocStatus::OtherDefault("shotwell.desktop".to_owned())
    );
    assert_eq!(status_of(&rows, "gif"), &AssocStatus::Default);
    assert_eq!(
        status_of(&rows, "webp"),
        &AssocStatus::OtherDefault("other.desktop".to_owned())
    );
    assert_eq!(status_of(&rows, "bmp"), &AssocStatus::Unregistered);

    assoc
        .register_available(&app("/usr/bin/lightbox", None), &base_formats())
        .unwrap();
    let rows = assoc.status(&base_formats()).unwrap();
    assert_eq!(status_of(&rows, "bmp"), &AssocStatus::Available);
    assert_eq!(
        status_of(&rows, "png"),
        &AssocStatus::OtherDefault("other.desktop".to_owned())
    );
}

#[test]
fn linux_register_in_temp_xdg_home() {
    let root = tempfile::tempdir().unwrap();
    let icon = root.path().join("icon.png");
    fs::write(&icon, b"png-bytes").unwrap();
    let assoc = LinuxAssoc {
        dirs: LinuxDirs {
            data_home: root.path().join("data"),
            config_home: root.path().join("config"),
            config_dirs: vec![root.path().join("xdg")],
            desktops: vec!["gnome".to_owned()],
        },
    };
    let install = app("/usr/bin/lightbox", Some(icon));
    assoc.register_available(&install, &base_formats()).unwrap();
    let desktop = root.path().join("data/applications").join(DESKTOP_ID);
    assert_eq!(
        fs::read_to_string(&desktop).unwrap(),
        desktop_entry("/usr/bin/lightbox", &base_formats())
    );
    let icon_out = root
        .path()
        .join("data/icons/hicolor/512x512/apps/app.lightbox.Lightbox.png");
    assert_eq!(fs::read(&icon_out).unwrap(), b"png-bytes");
    assert!(!root.path().join("config/mimeapps.list").exists());

    fs::create_dir_all(root.path().join("config")).unwrap();
    fs::write(
        root.path().join("config/mimeapps.list"),
        "# stay\n[Default Applications]\nimage/png=eog.desktop;\nimage/gif=app.lightbox.Viewer.desktop;gwenview.desktop;\n",
    )
    .unwrap();
    assoc.make_default(&install, &jpeg_only()).unwrap();
    assoc.unregister().unwrap();
    assert!(!desktop.exists());
    assert!(!icon_out.exists());
    let mime = fs::read_to_string(root.path().join("config/mimeapps.list")).unwrap();
    assert!(mime.contains("image/png=eog.desktop;"), "{mime}");
    assert!(mime.contains("image/gif=gwenview.desktop;"), "{mime}");
    assert!(mime.contains("# stay"), "{mime}");
    assert!(!mime.contains(DESKTOP_ID), "{mime}");
}

#[test]
fn default_viewer_requires_explicit_action() {
    let root = tempfile::tempdir().unwrap();
    let config = root.path().join("config");
    fs::create_dir_all(&config).unwrap();
    let seeded = "[Default Applications]\nimage/jpeg=eog.desktop;\n";
    fs::write(config.join("mimeapps.list"), seeded).unwrap();
    let assoc = LinuxAssoc {
        dirs: LinuxDirs {
            data_home: root.path().join("data"),
            config_home: config.clone(),
            config_dirs: vec![root.path().join("xdg")],
            desktops: Vec::new(),
        },
    };
    let install = app("/usr/bin/lightbox", None);
    AssocBackend::register_available(&assoc, &install, &base_formats()).unwrap();
    assert_eq!(
        fs::read_to_string(config.join("mimeapps.list")).unwrap(),
        seeded
    );
    let rows = assoc.status(&base_formats()).unwrap();
    assert!(
        rows.iter()
            .all(|(_, status)| !matches!(status, AssocStatus::Default))
    );
    assert_eq!(status_of(&rows, "png"), &AssocStatus::Available);
    assert_eq!(
        status_of(&rows, "jpg"),
        &AssocStatus::OtherDefault("eog.desktop".to_owned())
    );

    let made = AssocBackend::make_default(&assoc, &install, &base_formats()).unwrap();
    assert_eq!(made.outcome, MakeDefaultOutcome::Done);
    let rows = assoc.status(&base_formats()).unwrap();
    assert_eq!(status_of(&rows, "jpg"), &AssocStatus::Default);
    assert_eq!(status_of(&rows, "png"), &AssocStatus::Default);
}
