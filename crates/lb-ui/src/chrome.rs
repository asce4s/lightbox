//! Title bar, filter chips, sidebar, photo tray and status bar.

use crate::ids;
use crate::window::{LightboxWindow, sidebar_id};
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::input::Input;
use gpui_kit::component::slider::Slider;
use gpui_kit::component::{ActiveTheme as _, Disableable as _};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, IntoElement, MouseButton, MouseDownEvent, ObjectFit, TestSupportExt as _,
    Window, div, img, px,
};
use lb_app::ThumbSize;
use lb_app::vm::{SidebarKey, SidebarSection};

impl LightboxWindow {
    pub(crate) fn render_title_bar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let can_export = !self.vm.target_ids().is_empty();
        div()
            .id(ids::TITLE_BAR)
            .test_support()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_2()
            .border_b_1()
            .border_color(theme.border)
            .child(
                Button::new(ids::SLIDESHOW_BUTTON)
                    .label("Slideshow")
                    .tooltip("Play the selection or the whole view")
                    .on_click(cx.listener(|this, _, window, cx| this.start_slideshow(window, cx))),
            )
            .child(
                Button::new(ids::EXPORT_BUTTON)
                    .label("Export")
                    .disabled(!can_export)
                    .on_click(cx.listener(|this, _, window, cx| this.open_export(window, cx))),
            )
            .child(div().flex_1())
            .child(
                Button::new(ids::THEME_TOGGLE)
                    .label(match self.theme {
                        lb_app::ThemeChoice::System => "Theme: System",
                        lb_app::ThemeChoice::Light => "Theme: Light",
                        lb_app::ThemeChoice::Dark => "Theme: Dark",
                    })
                    .tooltip("Switch between system, light and dark")
                    .on_click(cx.listener(|this, _, window, cx| this.cycle_theme(window, cx))),
            )
            .child(
                div()
                    .w(px(280.))
                    .child(Input::new(&self.search).id(ids::SEARCH_INPUT)),
            )
            .into_any_element()
    }

    pub(crate) fn render_chips(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let chips = self.vm.view().chips.clone();
        let chip = |id: &'static str, label: &'static str, on: bool, cx: &mut Context<Self>| {
            Button::new(id)
                .label(label)
                .compact()
                .toggled(on)
                .on_click(cx.listener(move |this, _, _, cx| {
                    let mut chips = this.vm.view().chips.clone();
                    match id {
                        ids::CHIP_STARRED => chips.starred = !chips.starred,
                        ids::CHIP_VIDEO => chips.video = !chips.video,
                        ids::CHIP_FACES => chips.faces = !chips.faces,
                        ids::CHIP_GEO => chips.geotagged = !chips.geotagged,
                        _ => chips.edited = !chips.edited,
                    }
                    this.vm.set_chips(chips);
                    cx.notify();
                }))
        };
        div()
            .id(ids::CHIP_BAR)
            .test_support()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_1()
            .border_b_1()
            .border_color(theme.border)
            .child(chip(ids::CHIP_STARRED, "★ Starred", chips.starred, cx))
            .child(chip(ids::CHIP_VIDEO, "Video", chips.video, cx))
            .child(chip(ids::CHIP_FACES, "Faces", chips.faces, cx))
            .child(chip(ids::CHIP_GEO, "Geotagged", chips.geotagged, cx))
            .child(chip(ids::CHIP_EDITED, "✎ Edited", chips.edited, cx))
            .into_any_element()
    }

    pub(crate) fn render_sidebar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let selected = self.sidebar.selected();
        let sections: Vec<SidebarSection> = self.sidebar.sections().to_vec();
        let mut col = div()
            .id(ids::SIDEBAR)
            .test_support()
            .size_full()
            .flex()
            .flex_col()
            .gap_1()
            .p_2()
            .overflow_y_scroll()
            .border_r_1()
            .border_color(theme.border);
        for section in sections {
            let mut head = div()
                .flex()
                .items_center()
                .justify_between()
                .mt_2()
                .px_2()
                .text_xs()
                .text_color(theme.muted_foreground)
                .child(section.title.to_uppercase());
            if section.title == "Albums" {
                head = head.child(
                    Button::new(ids::NEW_ALBUM_BUTTON)
                        .label("+ New")
                        .compact()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.new_album(cx);
                        })),
                );
            }
            col = col.child(head);
            for item in section.items {
                let key = item.key;
                let active = key == selected;
                let heading = matches!(key, SidebarKey::Year(_));
                let mut row = div()
                    .id(sidebar_id(key))
                    .test_support()
                    .flex()
                    .items_center()
                    .justify_between()
                    .rounded_md()
                    .px_2()
                    .py_1()
                    .ml(px(f32::from(item.depth) * 12.0))
                    .when(active, |r| r.bg(theme.list_active))
                    .when(!heading, |r| {
                        r.hover(|s| s.bg(theme.list_hover)).cursor_pointer()
                    })
                    .when(heading, |r| r.text_color(theme.muted_foreground).text_xs())
                    .child(div().truncate().child(item.label))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(item.count.to_string()),
                    );
                if !heading {
                    row = row.on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, window, cx| {
                            window.focus(&this.focus, cx);
                            this.select_sidebar(key, cx);
                        }),
                    );
                }
                col = col.child(row);
            }
        }
        col.into_any_element()
    }

    pub(crate) fn render_tray(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let tray = self.vm.tray();
        let has_target = !self.vm.target_ids().is_empty();
        let mut strip = div()
            .id(ids::TRAY)
            .test_support()
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .py_1()
            .h(px(64.))
            .border_t_1()
            .border_color(theme.border);
        let mut thumbs = div().flex().gap_1().flex_1().overflow_hidden();
        for id in tray {
            let child = match self.thumb_for_size(id, ThumbSize::S) {
                Some(path) => img(path)
                    .object_fit(ObjectFit::Cover)
                    .size_full()
                    .into_any_element(),
                None => div().size_full().into_any_element(),
            };
            thumbs = thumbs.child(
                div()
                    .id(ids::tray_cell(id))
                    .test_support()
                    .w(px(48.))
                    .h(px(48.))
                    .flex_none()
                    .rounded_sm()
                    .overflow_hidden()
                    .bg(theme.muted)
                    .child(child),
            );
        }
        strip = strip
            .child(
                Button::new(ids::TRAY_HOLD)
                    .label("Hold ✚")
                    .compact()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.vm.hold_selected();
                        cx.notify();
                    })),
            )
            .child(
                Button::new(ids::TRAY_CLEAR)
                    .label("Clear")
                    .compact()
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.vm.clear_tray();
                        cx.notify();
                    })),
            )
            .child(thumbs)
            .child(
                Button::new(ids::TRAY_STAR)
                    .label("★")
                    .compact()
                    .tooltip("Star (S)")
                    .disabled(!has_target)
                    .on_click(cx.listener(|this, _, _, cx| this.toggle_star_targets(cx))),
            )
            .child(
                Button::new(ids::TRAY_ROTATE_LEFT)
                    .label("⟲")
                    .compact()
                    .tooltip("Rotate left (Ctrl+Shift+R)")
                    .disabled(!has_target)
                    .on_click(cx.listener(|this, _, _, cx| this.rotate_targets(-1, cx))),
            )
            .child(
                Button::new(ids::TRAY_ROTATE_RIGHT)
                    .label("⟳")
                    .compact()
                    .tooltip("Rotate right (Ctrl+R)")
                    .disabled(!has_target)
                    .on_click(cx.listener(|this, _, _, cx| this.rotate_targets(1, cx))),
            )
            .child(
                Button::new(ids::TRAY_ADD_TAG)
                    .label("Tag…")
                    .compact()
                    .disabled(!has_target)
                    .on_click(cx.listener(|this, _, window, cx| this.open_tag_dialog(window, cx))),
            )
            .child(
                Button::new(ids::TRAY_ADD_ALBUM)
                    .label("Album…")
                    .compact()
                    .disabled(!has_target)
                    .on_click(cx.listener(|this, _, _, cx| this.open_album_picker(cx))),
            )
            .child(
                Button::new(ids::TRAY_EXPORT)
                    .label("Export…")
                    .compact()
                    .disabled(!has_target)
                    .on_click(cx.listener(|this, _, window, cx| this.open_export(window, cx))),
            );
        strip.into_any_element()
    }

    pub(crate) fn render_status_bar(
        &mut self,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let mut text = format!("{} photos", group_thousands(self.vm.result().total));
        let selected = self.vm.selection().ids.len();
        if selected > 0 {
            text.push_str(&format!(" · {selected} selected"));
        }
        if let Some((done, total)) = self.scan {
            text.push_str(&format!(" · Scanning {done}/{total}"));
        }
        if self.jobs > 0 {
            text.push_str(&format!(" · Jobs: {}", self.jobs));
        }
        div()
            .id(ids::STATUS_BAR)
            .test_support()
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .py_1()
            .border_t_1()
            .border_color(theme.border)
            .text_xs()
            .text_color(theme.muted_foreground)
            .child(
                div()
                    .id(ids::STATUS_TEXT)
                    .test_support()
                    .aria_label(text.clone())
                    .flex_1()
                    .child(text),
            )
            .child(
                div()
                    .id(ids::ZOOM_SLIDER)
                    .test_support()
                    .w(px(140.))
                    .child(Slider::new(&self.zoom)),
            )
            .into_any_element()
    }

    pub(crate) fn render_toast(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let message = self.toast.clone()?;
        let theme = cx.theme().clone();
        Some(
            div()
                .id(ids::TOAST)
                .test_support()
                .aria_label(message.clone())
                .absolute()
                .bottom(px(80.))
                .right(px(16.))
                .max_w(px(420.))
                .px_3()
                .py_2()
                .rounded_md()
                .bg(theme.popover)
                .text_color(theme.popover_foreground)
                .border_1()
                .border_color(theme.border)
                .flex()
                .gap_3()
                .child(div().flex_1().child(message))
                .child(
                    Button::new("toast-dismiss")
                        .label("✕")
                        .compact()
                        .ghost()
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.toast = None;
                            cx.notify();
                        })),
                )
                .into_any_element(),
        )
    }
}

fn group_thousands(n: usize) -> String {
    let digits = n.to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    out
}
