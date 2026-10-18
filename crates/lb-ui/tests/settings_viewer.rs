//! M13.11: viewer settings and the first-run default-app banner (SPEC-VIEWER §V10.6, §V11).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::{UiLib, open, settle, ui_library_with};
use gpui_kit::test::TestWindowExt as _;
use gpui_kit::{AnyWindowHandle, AppContext as _, Entity, TestAppContext};
use lb_app::{
    AppInstall, AssocBackend, AssocResult, AssocStatus, BackdropMode, FormatAssoc, LightboxOptions,
    MakeDefaultOutcome, MakeDefaultResult, SavedDefaults,
};
use lb_ui::{LightboxWindow, ids};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

type View = Entity<LightboxWindow>;

struct FakeAssoc {
    made: Mutex<Vec<Vec<String>>>,
    restores: AtomicUsize,
}

impl FakeAssoc {
    fn new() -> Self {
        Self {
            made: Mutex::new(Vec::new()),
            restores: AtomicUsize::new(0),
        }
    }

    fn made(&self) -> Vec<Vec<String>> {
        self.made.lock().expect("assoc").clone()
    }

    fn restores(&self) -> usize {
        self.restores.load(Ordering::SeqCst)
    }
}

impl AssocBackend for FakeAssoc {
    fn status(&self, formats: &[FormatAssoc]) -> AssocResult<Vec<(FormatAssoc, AssocStatus)>> {
        Ok(formats
            .iter()
            .copied()
            .map(|fmt| {
                let status = match fmt.ext {
                    "jpg" => AssocStatus::Available,
                    "png" => AssocStatus::OtherDefault("Preview".to_owned()),
                    _ => AssocStatus::Unregistered,
                };
                (fmt, status)
            })
            .collect())
    }

    fn register_available(&self, _app: &AppInstall, _formats: &[FormatAssoc]) -> AssocResult<()> {
        Ok(())
    }

    fn make_default(
        &self,
        _app: &AppInstall,
        formats: &[FormatAssoc],
    ) -> AssocResult<MakeDefaultResult> {
        self.made
            .lock()
            .expect("assoc")
            .push(formats.iter().map(|fmt| fmt.ext.to_owned()).collect());
        Ok(MakeDefaultResult {
            outcome: MakeDefaultOutcome::OpenedSystemSettings,
            saved: Some(SavedDefaults {
                payload: "saved".to_owned(),
            }),
        })
    }

    fn restore_previous(&self, _saved: &SavedDefaults) -> AssocResult<()> {
        self.restores.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }

    fn unregister(&self, _app: &AppInstall) -> AssocResult<()> {
        Ok(())
    }
}

fn library(force_prompt: bool, assoc: Arc<FakeAssoc>) -> UiLib {
    ui_library_with(
        &[("june", &["IMG_20210601_100000.jpg"])],
        LightboxOptions {
            assoc: Some(assoc),
            force_default_prompt: force_prompt,
            ..LightboxOptions::default()
        },
    )
}

fn click(
    cx: &mut TestAppContext,
    handle: AnyWindowHandle,
    view: &View,
    id: impl Into<gpui_kit::ElementId>,
) {
    let id = id.into();
    cx.update_window(handle, |_, window, cx| window.click(id, cx))
        .unwrap();
    settle(cx, handle, view);
}

#[gpui_kit::test]
fn ui_settings_viewer_section_and_make_default(cx: &mut TestAppContext) {
    let assoc = Arc::new(FakeAssoc::new());
    let lib = library(false, Arc::clone(&assoc));
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);

    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find(ids::DEFAULT_VIEWER_BANNER).is_none());
    })
    .unwrap();
    click(cx, handle, &view, ids::SETTINGS_BUTTON);

    cx.update_window(handle, |_, window, _| {
        assert!(window.find(ids::SETTINGS_VIEWER).visible());
        let jpg = window.find(ids::assoc_type("jpg"));
        assert_eq!(jpg.checked(), Some(true));
        assert!(
            jpg.label().unwrap().contains("Available"),
            "{}",
            jpg.label().unwrap()
        );
        let png = window.find(ids::assoc_type("png"));
        assert!(
            png.label().unwrap().contains("Default: Preview"),
            "{}",
            png.label().unwrap()
        );
    })
    .unwrap();

    click(cx, handle, &view, ids::SETTINGS_RESTORE_DEFAULTS);
    assert_eq!(
        assoc.restores(),
        0,
        "restore stays idle until defaults were saved"
    );

    click(cx, handle, &view, ids::assoc_type("jpg"));
    cx.update_window(handle, |_, window, _| {
        assert_eq!(window.find(ids::assoc_type("jpg")).checked(), Some(false));
    })
    .unwrap();
    click(cx, handle, &view, ids::SETTINGS_BACKDROP);
    click(cx, handle, &view, ids::SETTINGS_ENLARGE);
    click(cx, handle, &view, ids::SETTINGS_MAKE_DEFAULT);

    let made = assoc.made();
    assert_eq!(made.len(), 1);
    assert!(!made[0].iter().any(|ext| ext == "jpg"));
    assert!(made[0].iter().any(|ext| ext == "png"));
    cx.update_window(handle, |_, window, _| {
        let note = window.find(ids::SETTINGS_NOTE);
        let note = note.label().unwrap();
        assert!(note.contains("Choose Lightbox Photo Viewer"), "{note}");
    })
    .unwrap();

    let settings = lib.app.settings().load().unwrap();
    assert_eq!(settings.viewer.backdrop, BackdropMode::DesktopBlur);
    assert!(settings.viewer.enlarge_small);
    assert!(!settings.viewer.file_types.iter().any(|ext| ext == "jpg"));
    assert_eq!(
        settings
            .viewer
            .saved_defaults
            .as_ref()
            .map(|saved| saved.payload.as_str()),
        Some("saved")
    );

    click(cx, handle, &view, ids::SETTINGS_RESTORE_DEFAULTS);
    assert_eq!(assoc.restores(), 1);
    let settings = lib.app.settings().load().unwrap();
    assert!(settings.viewer.saved_defaults.is_none());
    assert_eq!(settings.viewer.backdrop, BackdropMode::DesktopBlur);
    assert!(!settings.viewer.file_types.iter().any(|ext| ext == "jpg"));
}

#[gpui_kit::test]
fn ui_first_run_banner_dismiss_persists(cx: &mut TestAppContext) {
    let assoc = Arc::new(FakeAssoc::new());
    let lib = library(true, assoc);
    let (handle, view) = open(cx, &lib);
    settle(cx, handle, &view);

    cx.update_window(handle, |_, window, _| {
        let banner = window.find(ids::DEFAULT_VIEWER_BANNER);
        assert!(banner.visible());
        assert_eq!(
            banner.label(),
            Some("Open photos from your file manager with Lightbox?")
        );
        assert!(window.try_find(ids::BANNER_MAKE_DEFAULT).is_some());
        assert!(window.try_find(ids::BANNER_DISMISS).is_some());
    })
    .unwrap();

    click(cx, handle, &view, ids::BANNER_DISMISS);
    cx.update_window(handle, |_, window, _| {
        assert!(window.try_find(ids::DEFAULT_VIEWER_BANNER).is_none());
    })
    .unwrap();
    assert!(
        lib.app
            .settings()
            .load()
            .unwrap()
            .viewer
            .default_prompt_dismissed
    );

    let (again, again_view) = open(cx, &lib);
    settle(cx, again, &again_view);
    cx.update_window(again, |_, window, _| {
        assert!(window.try_find(ids::DEFAULT_VIEWER_BANNER).is_none());
    })
    .unwrap();
}
