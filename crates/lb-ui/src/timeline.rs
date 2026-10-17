//! Timeline scrubber under the grid (SPEC §9.3, F14).

use crate::ids;
use crate::window::LightboxWindow;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::Button;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent,
    ScrollStrategy, TestSupportExt as _, div, point, px,
};
use lb_app::FilterChips;
use lb_app::vm::GridRow;

/// Height of the scrubber strip.
pub const TIMELINE_HEIGHT: f32 = 44.0;
const BAR_MAX: f32 = 22.0;

/// A press on the scrubber that has not been released yet.
#[derive(Debug, Clone, Copy)]
pub(crate) struct TimelineDrag {
    start: usize,
    shift: bool,
}

impl LightboxWindow {
    pub(crate) fn render_timeline(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let buckets = self.vm.timeline().to_vec();
        let max = buckets.iter().map(|b| b.count).max().unwrap_or(1).max(1) as f32;
        let filtered = self.vm.view().chips.date.is_some();
        let mut strip = div().flex().flex_1().h_full().items_end().gap(px(1.));
        for (i, b) in buckets.iter().enumerate() {
            let height = 4.0 + (BAR_MAX - 4.0) * (b.count as f32 / max);
            let mut seg = div()
                .id(ids::timeline_month(i))
                .test_support()
                .flex_1()
                .h_full()
                .flex()
                .flex_col()
                .justify_end()
                .items_center()
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                        this.timeline_press(i, ev.modifiers.shift, cx);
                    }),
                )
                .on_mouse_move(cx.listener(move |this, ev: &MouseMoveEvent, _, cx| {
                    if ev.pressed_button == Some(MouseButton::Left) {
                        this.timeline_hover(i, cx);
                    }
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseUpEvent, _, cx| this.timeline_release(i, cx)),
                )
                .child(div().w_full().h(px(height)).bg(theme.primary.opacity(0.7)));
            let new_year = i == 0 || buckets[i - 1].year != b.year;
            if new_year || buckets.len() <= 12 {
                let text = if new_year {
                    b.year.to_string()
                } else {
                    month_name(b.month).chars().take(1).collect()
                };
                seg = seg.child(
                    div()
                        .text_xs()
                        .text_color(theme.muted_foreground)
                        .child(text),
                );
            }
            strip = strip.child(seg);
        }
        let mut bar = div()
            .id(ids::TIMELINE)
            .test_support()
            .flex()
            .items_end()
            .gap_2()
            .h(px(TIMELINE_HEIGHT))
            .px_3()
            .py_1()
            .border_t_1()
            .border_color(theme.border)
            .child(strip);
        if filtered {
            bar = bar.child(
                Button::new(ids::TIMELINE_CLEAR)
                    .label("Clear dates")
                    .compact()
                    .on_click(cx.listener(|this, _, _, cx| this.clear_timeline_filter(cx))),
            );
        }
        bar.into_any_element()
    }

    fn timeline_press(&mut self, index: usize, shift: bool, cx: &mut Context<Self>) {
        self.timeline_drag = Some(TimelineDrag {
            start: index,
            shift,
        });
        if !shift {
            self.scroll_to_month(index);
        }
        cx.notify();
    }

    fn timeline_hover(&mut self, index: usize, cx: &mut Context<Self>) {
        if let Some(drag) = self.timeline_drag
            && !drag.shift
        {
            self.scroll_to_month(index);
            cx.notify();
        }
    }

    fn timeline_release(&mut self, index: usize, cx: &mut Context<Self>) {
        let Some(drag) = self.timeline_drag.take() else {
            return;
        };
        if drag.shift {
            self.vm.timeline_filter(drag.start, index);
            self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        }
        cx.notify();
    }

    /// Scrolls the grid so month bucket `index` starts at the top.
    pub fn scroll_to_month(&mut self, index: usize) {
        let Some(row) = self.vm.timeline_jump(index) else {
            return;
        };
        let header = row
            .checked_sub(1)
            .filter(|r| matches!(self.vm.layout().rows.get(*r), Some(GridRow::Header { .. })));
        let top = self.vm.layout().row_top(header.unwrap_or(row));
        self.scroll.set_offset(point(px(0.), px(-top)));
    }

    /// Removes the date range set by shift-dragging the scrubber.
    pub fn clear_timeline_filter(&mut self, cx: &mut Context<Self>) {
        let chips = FilterChips {
            date: None,
            ..self.vm.view().chips.clone()
        };
        self.vm.set_chips(chips);
        cx.notify();
    }
}

fn month_name(month: u32) -> &'static str {
    const NAMES: [&str; 12] = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    NAMES
        .get(month.saturating_sub(1) as usize)
        .copied()
        .unwrap_or("?")
}
