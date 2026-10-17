//! Rubber-band selection in the grid (SPEC §9.3).

use crate::ids;
use crate::render::GRID_PADDING;
use crate::window::LightboxWindow;
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Bounds, Context, IntoElement, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels,
    Point, TestSupportExt as _, Window, div, px,
};
use std::cell::Cell;
use std::rc::Rc;

/// Pointer travel (pixels) before a press on empty space becomes a band.
const DRAG_THRESHOLD: f32 = 4.0;

/// Where the grid was last laid out, shared with a measuring element.
pub(crate) type GridBounds = Rc<Cell<Bounds<Pixels>>>;

/// A press on empty grid space that has not been released yet. Points are in content
/// coordinates: x from the first cell column, y from the top of the scrolled content.
#[derive(Debug, Clone, Copy)]
pub(crate) struct BandDrag {
    origin: (f32, f32),
    current: (f32, f32),
    moved: bool,
}

impl LightboxWindow {
    fn content_point(&self, position: Point<Pixels>) -> (f32, f32) {
        let bounds = self.grid_bounds.get();
        let scroll_top = (-f32::from(self.scroll.offset().y)).max(0.0);
        (
            f32::from(position.x - bounds.origin.x) - GRID_PADDING,
            f32::from(position.y - bounds.origin.y) + scroll_top,
        )
    }

    pub(crate) fn band_press(
        &mut self,
        ev: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus, cx);
        let point = self.content_point(ev.position);
        let additive = ev.modifiers.control || ev.modifiers.platform || ev.modifiers.shift;
        self.vm.begin_band(additive);
        self.band = Some(BandDrag {
            origin: point,
            current: point,
            moved: false,
        });
        cx.notify();
    }

    pub(crate) fn band_move(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        if ev.pressed_button != Some(gpui_kit::MouseButton::Left) {
            return;
        }
        let point = self.content_point(ev.position);
        let Some(mut band) = self.band else { return };
        band.current = point;
        let travel = (point.0 - band.origin.0)
            .abs()
            .max((point.1 - band.origin.1).abs());
        band.moved |= travel >= DRAG_THRESHOLD;
        if band.moved {
            self.vm
                .update_band(band.origin.0, band.origin.1, point.0, point.1);
        }
        self.band = Some(band);
        cx.notify();
    }

    pub(crate) fn band_release(&mut self, cx: &mut Context<Self>) {
        if self.band.take().is_some() {
            self.vm.end_band();
            cx.notify();
        }
    }

    /// The translucent rectangle drawn over the grid while dragging.
    pub(crate) fn render_band(&self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let band = self.band.filter(|b| b.moved)?;
        let theme = cx.theme().clone();
        let scroll_top = (-f32::from(self.scroll.offset().y)).max(0.0);
        let (x0, x1) = (
            band.origin.0.min(band.current.0),
            band.origin.0.max(band.current.0),
        );
        let (y0, y1) = (
            band.origin.1.min(band.current.1),
            band.origin.1.max(band.current.1),
        );
        Some(
            div()
                .id(ids::GRID_BAND)
                .test_support()
                .absolute()
                .left(px(x0 + GRID_PADDING))
                .top(px(y0 - scroll_top))
                .w(px(x1 - x0))
                .h(px(y1 - y0))
                .border_1()
                .border_color(theme.primary)
                .bg(theme.primary.opacity(0.2))
                .into_any_element(),
        )
    }
}

pub(crate) fn release_listener(
    cx: &mut Context<LightboxWindow>,
) -> impl Fn(&MouseUpEvent, &mut Window, &mut gpui_kit::App) + 'static {
    cx.listener(|this, _: &MouseUpEvent, _, cx| this.band_release(cx))
}
