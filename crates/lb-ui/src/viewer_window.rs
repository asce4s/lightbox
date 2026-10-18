//! Standalone viewer session (SPEC-VIEWER §V2.4).
//!
//! The borderless display-covering stage is M13.9. This window keeps the process
//! alive as the instance server and does not open the library until asked.

use crate::ids;
use crate::window::{LightboxWindow, PUMP_INTERVAL};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyWindowHandle, App, Context, Entity, FocusHandle, Focusable, IntoElement, Render, Result,
    TestSupportExt, Window, WindowOptions, div,
};
use lb_app::{InstanceServer, Launch, Lightbox};
use std::path::PathBuf;

/// Root of a viewer-only process.
pub struct ViewerWindow {
    app: Lightbox,
    instance: InstanceServer,
    paths: Vec<PathBuf>,
    perf: bool,
    focus: FocusHandle,
    library_open: bool,
}

impl ViewerWindow {
    /// Opens the viewer session for `paths`.
    pub fn open(
        app: Lightbox,
        paths: Vec<PathBuf>,
        perf: bool,
        instance: InstanceServer,
        cx: &mut App,
    ) -> Result<(AnyWindowHandle, Entity<Self>)> {
        gpui_kit::open_window(WindowOptions::default(), cx, move |window, cx| {
            cx.new(|cx| Self::new(app, paths, perf, instance, window, cx))
        })
    }

    fn new(
        app: Lightbox,
        paths: Vec<PathBuf>,
        perf: bool,
        instance: InstanceServer,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Self {
        if perf {
            tracing::info!("viewer perf capture requested");
        }
        let focus = cx.focus_handle();
        cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor().timer(PUMP_INTERVAL).await;
                if this.update(cx, |this, cx| this.pump(cx)).is_err() {
                    break;
                }
            }
        })
        .detach();
        Self {
            app,
            instance,
            paths,
            perf,
            focus,
            library_open: false,
        }
    }

    fn pump(&mut self, cx: &mut Context<Self>) {
        while let Some(launch) = self.instance.poll() {
            match launch {
                Launch::View { paths, perf } => {
                    self.paths = paths;
                    self.perf = perf;
                    cx.notify();
                }
                Launch::Library => self.show_library(cx),
                Launch::UnregisterFileTypes => {}
            }
        }
    }

    fn show_library(&mut self, cx: &mut Context<Self>) {
        if self.library_open {
            return;
        }
        self.library_open = true;
        let app = self.app.clone();
        std::thread::spawn(move || {
            if let Err(error) = lb_app::open_library_from_viewer(&app) {
                tracing::warn!(%error, "could not start the library");
            }
        });
        if let Err(error) = LightboxWindow::open_library(self.app.clone(), cx) {
            tracing::error!(%error, "could not open the library window");
        }
    }
}

impl Focusable for ViewerWindow {
    fn focus_handle(&self, _cx: &gpui_kit::App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for ViewerWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let _ = self.perf;
        let label = self
            .paths
            .first()
            .map(|path| path.display().to_string())
            .unwrap_or_default();
        div()
            .id(ids::VIEWER_WINDOW)
            .test_support()
            .track_focus(&self.focus)
            .size_full()
            .child(label)
    }
}
