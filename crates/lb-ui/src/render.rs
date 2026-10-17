//! `Render` for the library window: layout, grid and keyboard handling.

use crate::ids;
use crate::window::{INFO_WIDTH, LightboxWindow, Overlay, SIDEBAR_WIDTH};
use gpui_kit::component::{ActiveTheme as _, h_resizable, resizable_panel, v_virtual_list};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, Focusable as _, FontWeight, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, ObjectFit, Render, TestSupportExt as _, Window, div, img, px, size,
};
use lb_app::vm::{GAP, GridRow, HEADER_HEIGHT, Modifiers, NavKey};
use lb_core::Photo;
use std::rc::Rc;

/// Pixels reserved on both sides of the grid for padding.
const GRID_PADDING: f32 = GAP;

impl Render for LightboxWindow {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme().clone();
        let info = self.info_open;
        let sidebar = self.render_sidebar(cx);
        let grid = self.render_grid(window, cx);
        let info_panel = self.render_info_panel(cx);
        let main = h_resizable("main-panels")
            .with_state(&self.panels)
            .child(
                resizable_panel()
                    .size(px(SIDEBAR_WIDTH))
                    .size_range(px(160.)..px(420.))
                    .child(sidebar),
            )
            .child(resizable_panel().child(grid))
            .child(
                resizable_panel()
                    .visible(info)
                    .size(px(INFO_WIDTH))
                    .size_range(px(200.)..px(480.))
                    .child(info_panel),
            );
        let overlay = self.render_overlay(window, cx);
        div()
            .id(ids::ROOT)
            .test_support()
            .track_focus(&self.focus)
            .key_context("Lightbox")
            .on_key_down(cx.listener(Self::on_key_down))
            .relative()
            .size_full()
            .flex()
            .flex_col()
            .bg(theme.background)
            .text_color(theme.foreground)
            .text_sm()
            .child(self.render_title_bar(cx))
            .child(self.render_chips(cx))
            .child(div().flex_1().min_h_0().child(main))
            .child(self.render_tray(cx))
            .child(self.render_status_bar(window, cx))
            .children(overlay)
            .children(self.render_toast(cx))
    }
}

impl LightboxWindow {
    fn render_grid(&mut self, window: &mut Window, cx: &mut Context<Self>) -> AnyElement {
        let width = self.grid_width(window, cx) - 2.0 * GRID_PADDING;
        self.vm.set_viewport_width(width);
        self.track_scroll_anchor();
        let theme = cx.theme().clone();
        if self.vm.layout().rows.is_empty() {
            let text = if self.vm.view().search.trim().is_empty() && self.vm.view().chips.is_empty()
            {
                "No photos here yet"
            } else {
                "No photos match"
            };
            return div()
                .id(ids::GRID)
                .test_support()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme.muted_foreground)
                .child(div().id(ids::GRID_EMPTY).test_support().child(text))
                .into_any_element();
        }
        let layout = self.vm.layout();
        let sizes: Rc<Vec<_>> = Rc::new(
            (0..layout.rows.len())
                .map(|i| size(px(width), px(layout.row_height(i))))
                .collect(),
        );
        let list = v_virtual_list(cx.entity(), "grid-rows", sizes, |this, range, _, cx| {
            range.map(|i| this.render_row(i, cx)).collect::<Vec<_>>()
        })
        .track_scroll(&self.scroll);
        div()
            .id(ids::GRID)
            .test_support()
            .size_full()
            .px(px(GRID_PADDING))
            .child(list)
            .into_any_element()
    }

    fn render_row(&mut self, index: usize, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(row) = self.vm.layout().rows.get(index).copied() else {
            return div().into_any_element();
        };
        match row {
            GridRow::Header { group } => {
                let Some(g) = self.vm.result().groups.get(group) else {
                    return div().into_any_element();
                };
                let (title, subtitle) = (g.title.clone(), g.subtitle.clone());
                div()
                    .id(ids::group_header(group))
                    .test_support()
                    .h(px(HEADER_HEIGHT))
                    .flex()
                    .items_end()
                    .gap_3()
                    .pb_1()
                    .border_b_1()
                    .border_color(theme.border)
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(title))
                    .child(div().text_color(theme.muted_foreground).child(subtitle))
                    .into_any_element()
            }
            GridRow::Cells { group, start, len } => {
                let ids: Vec<_> = self
                    .vm
                    .result()
                    .groups
                    .get(group)
                    .map(|g| g.ids[start..(start + len).min(g.ids.len())].to_vec())
                    .unwrap_or_default();
                let photos = self.vm.photos(&ids).unwrap_or_default();
                let cell = self.vm.layout().cell;
                let mut row = div().flex().gap(px(GAP)).h(px(cell + GAP)).items_start();
                for id in ids {
                    if let Some(photo) = photos.iter().find(|p| p.id == id) {
                        row = row.child(self.render_cell(photo, cell, cx));
                    }
                }
                row.into_any_element()
            }
        }
    }

    fn render_cell(&mut self, photo: &Photo, cell: f32, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let id = photo.id;
        let selected = self.vm.selection().ids.contains(&id);
        let focused = self.vm.focused() == Some(id);
        let held = self.vm.tray().contains(&id);
        let thumb = self.thumb_for(id);
        let mut el = div()
            .id(ids::grid_cell(id))
            .test_support()
            .relative()
            .w(px(cell))
            .h(px(cell))
            .flex_none()
            .rounded_md()
            .overflow_hidden()
            .bg(theme.muted)
            .border_2()
            .border_color(if selected {
                theme.primary
            } else if focused {
                theme.ring
            } else {
                theme.background
            })
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                    let mods = Modifiers {
                        shift: ev.modifiers.shift,
                        ctrl: ev.modifiers.control || ev.modifiers.platform,
                    };
                    this.click_cell(id, mods, cx);
                    if ev.click_count >= 2 {
                        this.open_photo(id, cx);
                    }
                }),
            )
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(move |this, ev: &MouseDownEvent, window, cx| {
                    window.focus(&this.focus, cx);
                    this.open_menu(id, ev.position, cx);
                }),
            );
        el = match thumb {
            Some(path) => el.child(
                img(path)
                    .object_fit(ObjectFit::Contain)
                    .size_full()
                    .with_fallback(|| div().size_full().into_any_element()),
            ),
            None => el.child(
                div()
                    .size_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .text_color(theme.muted_foreground)
                    .child(if photo.status == lb_core::PhotoStatus::Ok {
                        ""
                    } else {
                        "unreadable"
                    }),
            ),
        };
        let mut badges = div().absolute().top_1().left_1().flex().gap_1().text_xs();
        if photo.starred {
            badges = badges.child(div().text_color(theme.warning).child("★"));
        }
        if photo.edit_version > 0 {
            badges = badges.child(div().text_color(theme.foreground).child("✎"));
        }
        if held {
            badges = badges.child(div().text_color(theme.primary).child("◼"));
        }
        el = el.child(badges);
        if photo.rating > 0 {
            el = el.child(
                div()
                    .absolute()
                    .bottom_1()
                    .left_1()
                    .text_xs()
                    .text_color(theme.foreground)
                    .child("●".repeat(usize::from(photo.rating))),
            );
        }
        el.into_any_element()
    }

    fn render_info_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let lines: Vec<String> = self
            .vm
            .focused()
            .and_then(|id| self.vm.photos(&[id]).ok())
            .and_then(|p| p.into_iter().next())
            .map(|p| {
                let mut v = vec![
                    p.path
                        .file_name()
                        .map(|n| n.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                ];
                if let (Some(w), Some(h)) = (p.width, p.height) {
                    v.push(format!("{w} x {h}"));
                }
                if let Some(t) = p.taken_at {
                    v.push(t.format("%Y-%m-%d %H:%M").to_string());
                }
                if let Some(c) = &p.camera {
                    v.push(c.clone());
                }
                v
            })
            .unwrap_or_default();
        div()
            .id(ids::INFO_PANEL)
            .test_support()
            .size_full()
            .p_3()
            .flex()
            .flex_col()
            .gap_1()
            .border_l_1()
            .border_color(theme.border)
            .children(lines.into_iter().map(|l| div().child(l)))
            .into_any_element()
    }

    fn on_key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        let key = ev.keystroke.key.as_str();
        let m = ev.keystroke.modifiers;
        let cmd = m.control || m.platform;
        if !matches!(self.overlay, Overlay::None) {
            if key == "escape" {
                self.close_overlay(window, cx);
            }
            return;
        }
        if cmd && key == "f" {
            window.focus(&self.search.focus_handle(cx), cx);
            return;
        }
        if self.search_focused(window, cx) {
            if key == "escape" {
                window.focus(&self.focus, cx);
            }
            return;
        }
        match key {
            "left" => self.move_focus(NavKey::Left, m.shift, cx),
            "right" => self.move_focus(NavKey::Right, m.shift, cx),
            "up" => self.move_focus(NavKey::Up, m.shift, cx),
            "down" => self.move_focus(NavKey::Down, m.shift, cx),
            "home" => self.move_focus(NavKey::Home, m.shift, cx),
            "end" => self.move_focus(NavKey::End, m.shift, cx),
            "enter" => {
                if let Some(id) = self.vm.activate() {
                    self.open_photo(id, cx);
                }
            }
            "space" => {
                self.vm.toggle_hold_focused();
                cx.notify();
            }
            "escape" => {
                self.vm.clear_selection();
                cx.notify();
            }
            "delete" | "backspace" => self.ask_delete(cx),
            "a" if cmd => {
                self.vm.select_all();
                cx.notify();
            }
            "z" if cmd && !m.shift => self.undo_organize(cx),
            "r" if cmd => self.rotate_targets(if m.shift { -1 } else { 1 }, cx),
            "s" if !cmd => self.toggle_star_targets(cx),
            digit if !cmd && digit.len() == 1 => {
                if let Some(d) = digit.chars().next().and_then(|c| c.to_digit(10))
                    && d <= 5
                {
                    self.rate_targets(d as u8, cx);
                }
            }
            _ => {}
        }
    }
}
