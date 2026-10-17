//! GPUI Kit views over the `lb-app` view-models.
#![warn(missing_docs)]

pub mod ids;

mod band;
mod chrome;
mod editor;
mod hello;
mod overlays;
mod render;
mod slideshow;
mod stage;
mod timeline;
mod viewer;
mod window;

pub use editor::{EDITOR_BAR_HEIGHT, EDITOR_PANEL_WIDTH, Tool};
pub use hello::{HELLO_TEXT, HelloWindow, open_hello_window};
pub use viewer::{FILMSTRIP_HEIGHT, VIEWER_BAR_HEIGHT};
pub use window::{LightboxWindow, Overlay, Route, thumb_size_for};

/// Starts the GUI event loop on `app` and blocks until the last window closes.
pub fn run(app: lb_app::Lightbox) {
    gpui_kit::application().run(move |cx| {
        gpui_kit::init(cx);
        if let Err(error) = LightboxWindow::open(app, cx) {
            tracing::error!(%error, "could not open the main window");
            cx.quit();
        }
    });
}
