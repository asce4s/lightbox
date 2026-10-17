//! The slideshow screen (SPEC §6.12): black stage, fitted photo, auto-hiding controls.

use crate::ids;
use crate::window::LightboxWindow;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, Entity, IntoElement, MouseMoveEvent, ObjectFit, TestSupportExt as _,
    Window, div, hsla, img, px,
};
use lb_app::ThumbSize;
use lb_app::vm::{DURATION_RANGE, Modifiers, SlideshowOptions, SlideshowState};

pub(crate) fn duration_slider(secs: u32, cx: &mut Context<LightboxWindow>) -> Entity<SliderState> {
    cx.new(|_| {
        SliderState::new()
            .min(DURATION_RANGE.0 as f32)
            .max(DURATION_RANGE.1 as f32)
            .step(1.0)
            .default_value(secs as f32)
    })
}

impl LightboxWindow {
    /// The running slideshow, if any.
    pub fn slideshow(&self) -> Option<&SlideshowState> {
        self.slideshow.as_ref()
    }

    /// Starts a full-window slideshow of the selection (two or more photos) or of the whole
    /// grid, beginning at the focused photo.
    pub fn start_slideshow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.vm.selected_ids();
        let ids = if selected.len() > 1 {
            selected
        } else {
            self.vm.order().to_vec()
        };
        if ids.is_empty() {
            self.notify_message("There are no photos to show.", cx);
            return;
        }
        let settings = self.app.settings().load().unwrap_or_default();
        let seed = self.app.clock().now().and_utc().timestamp().unsigned_abs();
        let opts = SlideshowOptions::from_settings(&settings.slideshow, seed);
        let mut state = match SlideshowState::new(&self.app, &ids, opts) {
            Ok(s) => s,
            Err(e) => {
                let message = e.user_message();
                self.notify_message(message, cx);
                return;
            }
        };
        if state.is_empty() {
            self.notify_message("None of those photos can be shown.", cx);
            return;
        }
        if let Some(focused) = self.vm.focused() {
            state.start_at(focused);
        }
        let secs = state.options().duration_secs;
        self.slideshow_slider.update(cx, |s, cx| {
            s.set_value(secs as f32, window, cx);
        });
        self.slideshow = Some(state);
        if !window.is_fullscreen() {
            window.toggle_fullscreen();
            self.slideshow_fullscreen = true;
        }
        self.preload_slideshow();
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Esc: back to the library with the photo that was showing focused.
    pub fn stop_slideshow(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(state) = self.slideshow.take() else {
            return;
        };
        if self.slideshow_fullscreen {
            self.slideshow_fullscreen = false;
            if window.is_fullscreen() {
                window.toggle_fullscreen();
            }
        }
        if let Some(id) = state.current() {
            self.vm.click(id, Modifiers::NONE);
            self.reveal_focused();
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn tick_slideshow(&mut self) -> bool {
        let Some(s) = &mut self.slideshow else {
            return false;
        };
        let advanced = s.tick();
        let hidden = !s.controls_visible();
        if advanced {
            self.preload_slideshow();
        }
        advanced || (hidden && self.slideshow_controls_shown)
    }

    fn preload_slideshow(&mut self) {
        let Some(s) = &self.slideshow else { return };
        let ids: Vec<_> = s.current().into_iter().chain(s.upcoming()).collect();
        for id in ids {
            self.thumb_for_size(id, ThumbSize::L);
        }
    }

    pub(crate) fn slideshow_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        let Some(s) = &mut self.slideshow else { return };
        s.touch();
        match key {
            "escape" => {
                self.stop_slideshow(window, cx);
                return;
            }
            "space" => s.toggle_play(),
            "left" => {
                s.go_prev();
            }
            "right" => {
                s.go_next();
            }
            _ => {}
        }
        self.preload_slideshow();
        cx.notify();
    }

    fn with_slideshow(&mut self, cx: &mut Context<Self>, f: impl FnOnce(&mut SlideshowState)) {
        if let Some(s) = &mut self.slideshow {
            s.touch();
            f(s);
        }
        self.preload_slideshow();
        cx.notify();
    }

    pub(crate) fn on_duration_change(&mut self, secs: u32, cx: &mut Context<Self>) {
        self.with_slideshow(cx, |s| s.set_duration(secs));
    }

    pub(crate) fn render_slideshow(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(s) = &self.slideshow else {
            return div().into_any_element();
        };
        let current = s.current();
        let caption = s.caption();
        let controls = s.controls_visible();
        let (playing, finished) = (s.is_playing(), s.is_finished());
        let (shuffle, looping, captions) = (
            s.options().shuffle,
            s.options().looping,
            s.options().show_captions,
        );
        let position = format!("{} / {}", s.index() + 1, s.len());
        self.slideshow_controls_shown = controls;
        let path = current.and_then(|id| self.thumb_for_size(id, ThumbSize::L));
        let theme = cx.theme().clone();
        let white = hsla(0.0, 0.0, 1.0, 0.9);
        let mut root = div()
            .id(ids::SLIDESHOW)
            .test_support()
            .relative()
            .size_full()
            .bg(hsla(0.0, 0.0, 0.0, 1.0))
            .text_color(white)
            .on_mouse_move(cx.listener(|this, _: &MouseMoveEvent, _, cx| {
                this.with_slideshow(cx, |s| s.touch());
            }));
        if let Some(path) = path {
            root = root.child(
                div().absolute().size_full().child(
                    img(path)
                        .object_fit(ObjectFit::Contain)
                        .size_full()
                        .with_fallback(|| div().size_full().into_any_element()),
                ),
            );
        }
        if let Some(text) = caption {
            root = root.child(
                div()
                    .absolute()
                    .bottom(px(64.))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .id(ids::SLIDESHOW_CAPTION)
                            .test_support()
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .bg(hsla(0.0, 0.0, 0.0, 0.6))
                            .child(text),
                    ),
            );
        }
        if finished {
            root = root.child(
                div()
                    .absolute()
                    .top_4()
                    .right_4()
                    .text_color(theme.muted_foreground)
                    .child("End of slideshow - Esc to exit"),
            );
        }
        if controls {
            let toggle = |id: &'static str,
                          label: &'static str,
                          on: bool,
                          cx: &mut Context<Self>,
                          f: fn(&mut SlideshowState)| {
                Button::new(id)
                    .label(label)
                    .toggled(on)
                    .on_click(cx.listener(move |this, _, _, cx| this.with_slideshow(cx, f)))
            };
            root = root.child(
                div()
                    .id(ids::SLIDESHOW_CONTROLS)
                    .test_support()
                    .absolute()
                    .bottom_0()
                    .left_0()
                    .right_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .gap_2()
                    .p_3()
                    .bg(hsla(0.0, 0.0, 0.0, 0.7))
                    .child(
                        Button::new(ids::SLIDESHOW_PREV)
                            .label("Prev")
                            .tooltip("Previous (Left)")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.with_slideshow(cx, |s| {
                                    s.go_prev();
                                });
                            })),
                    )
                    .child(
                        Button::new(ids::SLIDESHOW_PLAY)
                            .label(if playing { "Pause" } else { "Play" })
                            .tooltip("Play or pause (Space)")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.with_slideshow(cx, SlideshowState::toggle_play);
                            })),
                    )
                    .child(
                        Button::new(ids::SLIDESHOW_NEXT)
                            .label("Next")
                            .tooltip("Next (Right)")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.with_slideshow(cx, |s| {
                                    s.go_next();
                                });
                            })),
                    )
                    .child(div().child(position))
                    .child(
                        div()
                            .id(ids::SLIDESHOW_DURATION)
                            .test_support()
                            .w(px(140.))
                            .child(Slider::new(&self.slideshow_slider)),
                    )
                    .child(toggle(
                        ids::SLIDESHOW_SHUFFLE,
                        "Shuffle",
                        shuffle,
                        cx,
                        |s| {
                            s.set_shuffle(!s.options().shuffle);
                        },
                    ))
                    .child(toggle(ids::SLIDESHOW_LOOP, "Loop", looping, cx, |s| {
                        s.set_loop(!s.options().looping);
                    }))
                    .child(toggle(
                        ids::SLIDESHOW_CAPTIONS,
                        "Captions",
                        captions,
                        cx,
                        |s| s.set_show_captions(!s.options().show_captions),
                    ))
                    .child(
                        Button::new(ids::SLIDESHOW_EXIT)
                            .label("Exit")
                            .tooltip("Exit (Esc)")
                            .on_click(cx.listener(|this, _, window, cx| {
                                this.stop_slideshow(window, cx);
                            })),
                    ),
            );
        }
        root.into_any_element()
    }

    pub(crate) fn slideshow_subscription(
        slider: &Entity<SliderState>,
        cx: &mut Context<Self>,
    ) -> gpui_kit::Subscription {
        cx.subscribe(slider, |this: &mut Self, _, event, cx| {
            if let SliderEvent::Change(value) = event {
                this.on_duration_change(value.start().round() as u32, cx);
            }
        })
    }
}
