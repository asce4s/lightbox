//! Minimal window proving the GPUI Kit toolchain builds, renders and can be tested headlessly.

use crate::ids;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyWindowHandle, App, Context, Entity, IntoElement, Render, Result, TestSupportExt, Window,
    WindowOptions, div, px, rgb,
};

/// Text rendered by [`HelloWindow`].
pub const HELLO_TEXT: &str = "Hello, Lightbox";

/// Root view of the hello window.
#[derive(Debug, Default)]
pub struct HelloWindow;

impl Render for HelloWindow {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .size_full()
            .flex()
            .items_center()
            .justify_center()
            .bg(rgb(0x1e1e2e))
            .text_color(rgb(0xcdd6f4))
            .child(
                div()
                    .id(ids::HELLO_LABEL)
                    .test_support()
                    .aria_label(HELLO_TEXT)
                    .h(px(32.))
                    .child(HELLO_TEXT),
            )
    }
}

/// Opens the hello window on `cx`. [`gpui_kit::init`] must have run.
pub fn open_hello_window(cx: &mut App) -> Result<(AnyWindowHandle, Entity<HelloWindow>)> {
    gpui_kit::open_window(WindowOptions::default(), cx, |_, cx| {
        cx.new(|_| HelloWindow)
    })
}
