//! GPUI Kit views over the `lb-app` view-models.
#![warn(missing_docs)]

pub mod ids;

mod hello;

pub use hello::{HELLO_TEXT, HelloWindow, open_hello_window};

/// Starts the GUI event loop and blocks until the last window closes.
pub fn run() {
    gpui_kit::application().run(|cx| {
        gpui_kit::init(cx);
        if let Err(error) = open_hello_window(cx) {
            tracing::error!(%error, "could not open the main window");
            cx.quit();
        }
    });
}
