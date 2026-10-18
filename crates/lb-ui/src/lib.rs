//! GPUI Kit views over the `lb-app` view-models.
#![warn(missing_docs)]

pub mod ids;

mod band;
mod chrome;
mod editor;
mod hello;
mod overlays;
mod render;
mod settings_viewer;
mod slideshow;
mod stage;
mod timeline;
mod viewer;
mod window;

pub use editor::{EDITOR_BAR_HEIGHT, EDITOR_PANEL_WIDTH, Tool};
pub use hello::{HELLO_TEXT, HelloWindow, open_hello_window};
pub use viewer::{FILMSTRIP_HEIGHT, VIEWER_BAR_HEIGHT};
pub use window::{LightboxWindow, Overlay, Route, thumb_size_for};

use gpui_kit::{AnyWindowHandle, App, AppContext as _};
use lb_app::{InstanceServer, Launch, Lightbox};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

/// Starts the GUI event loop on `app` and blocks until the last window closes.
///
/// `launch` chooses the library window or the viewer-only session. `instance` is
/// drained by that window's pump. On macOS, a launch with no paths waits up to
/// 250 ms for `on_open_urls` before the library window opens.
pub fn run(app: Lightbox, launch: Launch, instance: InstanceServer) {
    let urls = Arc::new(Mutex::new(Vec::<String>::new()));
    let reopen = Arc::new(AtomicBool::new(false));
    let library = Arc::new(Mutex::new(None::<AnyWindowHandle>));
    let application = gpui_kit::application();
    {
        let urls = Arc::clone(&urls);
        application.on_open_urls(move |list| match urls.lock() {
            Ok(mut guard) => guard.extend(list),
            Err(poisoned) => poisoned.into_inner().extend(list),
        });
    }
    {
        let app = app.clone();
        let library = Arc::clone(&library);
        let reopen = Arc::clone(&reopen);
        application.on_reopen(move |cx| {
            reopen.store(true, Ordering::SeqCst);
            show_library(&app, &library, cx);
        });
    }
    application.run(move |cx| {
        gpui_kit::init(cx);
        let launch = finish_macos(launch, &urls, &reopen);
        match launch {
            Launch::Library => match LightboxWindow::open(app, instance, cx) {
                Ok((handle, _)) => store_library(&library, handle),
                Err(error) => {
                    tracing::error!(%error, "could not open the main window");
                    cx.quit();
                }
            },
            Launch::View { paths, perf } => {
                if perf {
                    tracing::info!("viewer perf capture requested");
                }
                if let Err(error) = LightboxWindow::open_standalone(app, paths, instance, cx) {
                    tracing::error!(%error, "could not open the viewer window");
                    cx.quit();
                }
            }
            Launch::UnregisterFileTypes => cx.quit(),
        }
    });
}

fn finish_macos(launch: Launch, urls: &Mutex<Vec<String>>, reopen: &AtomicBool) -> Launch {
    #[cfg(target_os = "macos")]
    {
        let start = std::time::Instant::now();
        loop {
            let list = match urls.lock() {
                Ok(guard) => guard.clone(),
                Err(poisoned) => poisoned.into_inner().clone(),
            };
            match lb_app::macos_route(
                &launch,
                &list,
                start.elapsed(),
                reopen.load(Ordering::SeqCst),
            ) {
                lb_app::MacosDecision::Ready(next) => return next,
                lb_app::MacosDecision::Wait => {
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (urls, reopen);
        launch
    }
}

fn show_library(app: &Lightbox, slot: &Mutex<Option<AnyWindowHandle>>, cx: &mut App) {
    let existing = match slot.lock() {
        Ok(guard) => *guard,
        Err(poisoned) => *poisoned.into_inner(),
    };
    if let Some(handle) = existing {
        let activated = cx.update_window(handle, |_view, window, _cx| {
            window.activate_window();
        });
        if activated.is_ok() {
            return;
        }
    }
    let scan = app.clone();
    std::thread::spawn(move || {
        if let Err(error) = lb_app::open_library_from_viewer(&scan) {
            tracing::warn!(%error, "could not start the library");
        }
    });
    match LightboxWindow::open_library(app.clone(), cx) {
        Ok((handle, _)) => store_library(slot, handle),
        Err(error) => tracing::error!(%error, "could not open the library window"),
    }
}

fn store_library(slot: &Mutex<Option<AnyWindowHandle>>, handle: AnyWindowHandle) {
    match slot.lock() {
        Ok(mut guard) => *guard = Some(handle),
        Err(poisoned) => *poisoned.into_inner() = Some(handle),
    }
}
