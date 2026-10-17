//! The single-photo viewer: fit/zoom/pan, filmstrip and info panel (SPEC §9.4).

use crate::ids;
use crate::stage::{RenderJob, Stage, StageKind, frame_key};
use crate::window::{INFO_WIDTH, LightboxWindow};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::button::Button;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, FontWeight, IntoElement, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, ObjectFit, Pixels, Point, ScrollWheelEvent, TestSupportExt as _, Window, div,
    img, px,
};
use lb_app::ThumbSize;
use lb_app::vm::{Modifiers, ViewerVm, ZoomMode};
use lb_core::{EditStack, Photo, PhotoId};

/// Height of the viewer's top bar.
pub const VIEWER_BAR_HEIGHT: f32 = 44.0;
/// Height of the filmstrip.
pub const FILMSTRIP_HEIGHT: f32 = 84.0;
/// Width of one filmstrip cell including its gap.
const FILM_STEP: f32 = 72.0;
const FILM_CELL: f32 = 64.0;
/// Longest edge of the proxy shown while fitting the window.
const PROXY_EDGE: u32 = 2560;
/// Longest edge rendered when zoomed in.
const ZOOM_EDGE: u32 = 8192;

/// State of the open viewer.
pub(crate) struct ViewerState {
    pub(crate) vm: ViewerVm,
    pub(crate) stage: Stage,
    pub(crate) drag: Option<Point<Pixels>>,
    pub(crate) photo: Option<Photo>,
    pub(crate) stack: EditStack,
}

impl LightboxWindow {
    // ---- state ----

    /// The viewer's view-model while the viewer is open.
    pub fn viewer(&self) -> Option<&ViewerVm> {
        self.viewer.as_ref().map(|v| &v.vm)
    }

    /// The image file the viewer currently draws (a thumbnail until the proxy is rendered).
    pub fn viewer_image(&self) -> Option<std::path::PathBuf> {
        let v = self.viewer.as_ref()?;
        let id = v.vm.current();
        shown_for(v, id).map(|f| f.path.clone()).or_else(|| {
            self.thumbs
                .get(&(id, ThumbSize::L))
                .or_else(|| self.thumbs.get(&(id, ThumbSize::M)))
                .cloned()
        })
    }

    /// Opens the viewer on `id`, with the current grid order as the filmstrip.
    pub fn open_photo(&mut self, id: PhotoId, cx: &mut Context<Self>) {
        let is_video = self
            .app
            .photos()
            .get(id)
            .ok()
            .flatten()
            .is_some_and(|p| p.media_type == lb_core::MediaType::Video);
        if is_video {
            // Playback is out of scope (SPEC §6.16): hand the file to the default player.
            if let Err(e) = self.app.photos().open_externally(id) {
                self.notify_message(e.user_message(), cx);
            }
            return;
        }
        let ids = self.vm.order().to_vec();
        let Some(vm) = ViewerVm::open(&self.app, ids, id) else {
            return;
        };
        self.viewer = Some(ViewerState {
            vm,
            stage: Stage::new(),
            drag: None,
            photo: None,
            stack: EditStack::new(),
        });
        self.opened = Some(id);
        self.viewer_reload(cx);
    }

    pub(crate) fn viewer_reload(&mut self, cx: &mut Context<Self>) {
        let Some(v) = &self.viewer else { return };
        let id = v.vm.current();
        let photo = v.vm.photo().ok().flatten();
        let stack = self.app.edits().stack(id).unwrap_or_default();
        if let Some(v) = &mut self.viewer {
            v.photo = photo;
            v.stack = stack;
        }
        self.opened = Some(id);
        self.sync_viewer(cx);
        cx.notify();
    }

    fn viewer_edge(&self) -> u32 {
        let Some(v) = &self.viewer else {
            return PROXY_EDGE;
        };
        if v.vm.zoom() == ZoomMode::Fit {
            return PROXY_EDGE;
        }
        v.photo
            .as_ref()
            .and_then(|p| Some(p.width?.max(p.height?)))
            .map_or(PROXY_EDGE, |e| e.clamp(PROXY_EDGE, ZOOM_EDGE))
    }

    /// Starts the proxy render the viewer needs, unless it is shown or already running.
    pub(crate) fn sync_viewer(&mut self, cx: &mut Context<Self>) {
        let edge = self.viewer_edge();
        let Some(v) = &mut self.viewer else { return };
        let id = v.vm.current();
        let key = frame_key(id, &v.stack, edge);
        let stack = v.stack.clone();
        if let Some(cancel) = v.stage.want(&key) {
            let job = RenderJob {
                kind: StageKind::Viewer,
                key,
                id,
                stack,
                max_edge: edge,
                cancel,
            };
            self.spawn_render(job, cx);
        }
        self.thumb_for_size(id, ThumbSize::L);
    }

    // ---- intents ----

    fn viewer_move(&mut self, moved: bool, cx: &mut Context<Self>) {
        if moved {
            self.viewer_reload(cx);
        }
    }

    /// Right arrow.
    pub fn viewer_next(&mut self, cx: &mut Context<Self>) {
        let moved = self.viewer.as_mut().is_some_and(|v| v.vm.go_next());
        self.viewer_move(moved, cx);
    }

    /// Left arrow.
    pub fn viewer_prev(&mut self, cx: &mut Context<Self>) {
        let moved = self.viewer.as_mut().is_some_and(|v| v.vm.go_prev());
        self.viewer_move(moved, cx);
    }

    /// Home.
    pub fn viewer_home(&mut self, cx: &mut Context<Self>) {
        let moved = self.viewer.as_mut().is_some_and(|v| v.vm.home());
        self.viewer_move(moved, cx);
    }

    /// End.
    pub fn viewer_end(&mut self, cx: &mut Context<Self>) {
        let moved = self.viewer.as_mut().is_some_and(|v| v.vm.end());
        self.viewer_move(moved, cx);
    }

    /// A click on a filmstrip cell.
    pub fn viewer_goto(&mut self, id: PhotoId, cx: &mut Context<Self>) {
        let moved = self.viewer.as_mut().is_some_and(|v| v.vm.goto(id));
        self.viewer_move(moved, cx);
    }

    /// `I`.
    pub fn viewer_toggle_info(&mut self, cx: &mut Context<Self>) {
        if let Some(v) = &mut self.viewer {
            v.vm.toggle_info();
        }
        cx.notify();
    }

    /// The scale that fits the current photo into the image area.
    pub fn viewer_fit_scale(&self) -> f32 {
        let (w, h) = self.viewer_image_size();
        let (aw, ah) = self.viewer_area();
        (aw / w.max(1.0)).min(ah / h.max(1.0))
    }

    fn viewer_area(&self) -> (f32, f32) {
        let info_open = self.viewer.as_ref().is_some_and(|v| v.vm.info_visible());
        let info = if info_open { INFO_WIDTH } else { 0.0 };
        (
            (self.viewport.0 - info).max(1.0),
            (self.viewport.1 - VIEWER_BAR_HEIGHT - FILMSTRIP_HEIGHT).max(1.0),
        )
    }

    fn viewer_image_size(&self) -> (f32, f32) {
        let Some(v) = &self.viewer else {
            return (1.0, 1.0);
        };
        let shown = shown_for(v, v.vm.current());
        if v.stack.is_empty()
            && let Some(p) = &v.photo
            && let (Some(w), Some(h)) = (p.width, p.height)
        {
            return (w as f32, h as f32);
        }
        shown.map_or_else(
            || {
                v.photo
                    .as_ref()
                    .and_then(|p| Some((p.width? as f32, p.height? as f32)))
                    .unwrap_or((1.0, 1.0))
            },
            |f| (f.width as f32, f.height as f32),
        )
    }

    /// `+` and wheel up.
    pub fn viewer_zoom_in(&mut self, cx: &mut Context<Self>) {
        let fit = self.viewer_fit_scale();
        if let Some(v) = &mut self.viewer {
            v.vm.zoom_in(fit);
        }
        self.sync_viewer(cx);
        cx.notify();
    }

    /// `-` and wheel down.
    pub fn viewer_zoom_out(&mut self, cx: &mut Context<Self>) {
        let fit = self.viewer_fit_scale();
        if let Some(v) = &mut self.viewer {
            v.vm.zoom_out(fit);
        }
        self.sync_viewer(cx);
        cx.notify();
    }

    /// `1` and double-click: fit becomes 100% and back.
    pub fn viewer_toggle_zoom(&mut self, cx: &mut Context<Self>) {
        if let Some(v) = &mut self.viewer {
            v.vm.toggle_fit_actual();
        }
        self.sync_viewer(cx);
        cx.notify();
    }

    /// Esc: back to the library with the shown photo focused.
    pub fn close_viewer(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut v) = self.viewer.take() else {
            return;
        };
        v.stage.cancel();
        let id = v.vm.close();
        self.vm.click(id, Modifiers::NONE);
        self.reveal_focused();
        window.focus(&self.focus, cx);
        cx.notify();
    }

    pub(crate) fn viewer_key(&mut self, key: &str, window: &mut Window, cx: &mut Context<Self>) {
        match key {
            "left" => self.viewer_prev(cx),
            "right" => self.viewer_next(cx),
            "home" => self.viewer_home(cx),
            "end" => self.viewer_end(cx),
            "escape" => self.close_viewer(window, cx),
            "i" => self.viewer_toggle_info(cx),
            "+" | "=" => self.viewer_zoom_in(cx),
            "-" => self.viewer_zoom_out(cx),
            "1" => self.viewer_toggle_zoom(cx),
            "e" => {
                if let Some(id) = self.viewer.as_ref().map(|v| v.vm.current()) {
                    self.open_editor(id, window, cx);
                }
            }
            _ => {}
        }
    }

    // ---- rendering ----

    pub(crate) fn render_viewer(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(v) = &self.viewer else {
            return div().into_any_element();
        };
        let id = v.vm.current();
        self.thumb_for_size(id, ThumbSize::L);
        let Some(v) = &self.viewer else {
            return div().into_any_element();
        };
        let name = v
            .photo
            .as_ref()
            .and_then(|p| p.path.file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let info_open = v.vm.info_visible();
        let (has_prev, has_next) = (v.vm.has_prev(), v.vm.has_next());
        let zoom_label = match v.vm.zoom() {
            ZoomMode::Fit => "Fit".to_owned(),
            ZoomMode::Actual => "100%".to_owned(),
            ZoomMode::Factor(f) => format!("{:.0}%", f * 100.0),
        };
        let top = div()
            .id("viewer-bar")
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .h(px(VIEWER_BAR_HEIGHT))
            .border_b_1()
            .border_color(theme.border)
            .child(
                Button::new(ids::VIEWER_BACK)
                    .label("← Back")
                    .tooltip("Back to library (Esc)")
                    .on_click(cx.listener(|this, _, window, cx| this.close_viewer(window, cx))),
            )
            .child(div().font_weight(FontWeight::SEMIBOLD).child(name))
            .child(div().flex_1())
            .child(
                div()
                    .id(ids::VIEWER_ZOOM_LABEL)
                    .test_support()
                    .aria_label(zoom_label.clone())
                    .text_color(theme.muted_foreground)
                    .child(zoom_label),
            )
            .child(
                Button::new(ids::VIEWER_EDIT)
                    .label("Edit")
                    .tooltip("Edit (E)")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.open_editor(id, window, cx);
                    })),
            )
            .child(
                Button::new(ids::VIEWER_INFO)
                    .label("Info")
                    .tooltip("Info panel (I)")
                    .toggled(info_open)
                    .on_click(cx.listener(|this, _, _, cx| this.viewer_toggle_info(cx))),
            );

        let (aw, ah) = self.viewer_area();
        let fit = self.viewer_fit_scale();
        let (rw, rh) = self.viewer_image_size();
        let (scale, pan) = self
            .viewer
            .as_ref()
            .map_or((fit, (0.0, 0.0)), |v| (v.vm.zoom().scale(fit), v.vm.pan()));
        let (dw, dh) = (rw * scale, rh * scale);
        let path = self.viewer_image();
        let busy = self.viewer.as_ref().is_some_and(|v| v.stage.busy());
        let mut image_area = div()
            .id(ids::VIEWER_IMAGE)
            .test_support()
            .relative()
            .overflow_hidden()
            .flex_1()
            .h_full()
            .bg(theme.muted)
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                    if ev.click_count >= 2 {
                        this.viewer_toggle_zoom(cx);
                    } else if let Some(v) = &mut this.viewer {
                        v.drag = Some(ev.position);
                    }
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                let Some(v) = &mut this.viewer else { return };
                let Some(last) = v.drag else { return };
                if ev.pressed_button != Some(MouseButton::Left) {
                    v.drag = None;
                    return;
                }
                v.vm.pan_by(
                    f32::from(ev.position.x - last.x),
                    f32::from(ev.position.y - last.y),
                );
                v.drag = Some(ev.position);
                cx.notify();
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, _| {
                    if let Some(v) = &mut this.viewer {
                        v.drag = None;
                    }
                }),
            )
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                let dy = f32::from(ev.delta.pixel_delta(px(16.0)).y);
                if dy > 0.0 {
                    this.viewer_zoom_in(cx);
                } else if dy < 0.0 {
                    this.viewer_zoom_out(cx);
                }
            }));
        if let Some(path) = path {
            image_area = image_area.child(
                div()
                    .absolute()
                    .left(px((aw - dw) / 2.0 + pan.0))
                    .top(px((ah - dh) / 2.0 + pan.1))
                    .w(px(dw))
                    .h(px(dh))
                    .child(
                        img(path)
                            .object_fit(ObjectFit::Fill)
                            .size_full()
                            .with_fallback(|| div().size_full().into_any_element()),
                    ),
            );
        }
        if busy {
            image_area = image_area.child(
                div()
                    .absolute()
                    .top_2()
                    .right_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("Rendering…"),
            );
        }
        if has_prev {
            image_area = image_area.child(self.nav_button(ids::VIEWER_PREV, "‹", true, cx));
        }
        if has_next {
            image_area = image_area.child(self.nav_button(ids::VIEWER_NEXT, "›", false, cx));
        }

        let info_panel = info_open.then(|| self.render_viewer_info(cx));
        let middle = div()
            .id(ids::VIEWER)
            .test_support()
            .flex()
            .flex_1()
            .min_h_0()
            .child(image_area)
            .children(info_panel);
        div()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(top)
            .child(middle)
            .child(self.render_filmstrip(cx))
            .into_any_element()
    }

    fn nav_button(
        &self,
        id: &'static str,
        label: &'static str,
        left: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .absolute()
            .top_0()
            .bottom_0()
            .when(left, |d| d.left_2())
            .when(!left, |d| d.right_2())
            .flex()
            .items_center()
            .child(
                Button::new(id)
                    .label(label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        if left {
                            this.viewer_prev(cx);
                        } else {
                            this.viewer_next(cx);
                        }
                    })),
            )
            .into_any_element()
    }

    fn render_filmstrip(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(v) = &self.viewer else {
            return div().into_any_element();
        };
        let strip = v.vm.filmstrip().to_vec();
        let current = v.vm.index();
        let half = (self.viewport.0 / 2.0 / FILM_STEP).ceil() as usize + 2;
        let start = current.saturating_sub(half);
        let end = (current + half + 1).min(strip.len());
        let left = self.viewport.0 / 2.0 - FILM_CELL / 2.0 - (current - start) as f32 * FILM_STEP;
        let mut row = div()
            .absolute()
            .top(px(10.0))
            .left(px(left))
            .flex()
            .gap(px(FILM_STEP - FILM_CELL));
        for (offset, id) in strip[start..end].iter().copied().enumerate() {
            let active = start + offset == current;
            let thumb = self.thumb_for_size(id, ThumbSize::S);
            let mut cell = div()
                .id(ids::filmstrip_cell(id))
                .test_support()
                .flex_none()
                .w(px(FILM_CELL))
                .h(px(FILM_CELL))
                .rounded_md()
                .overflow_hidden()
                .bg(theme.muted)
                .border_2()
                .border_color(if active {
                    theme.primary
                } else {
                    theme.background
                })
                .cursor_pointer()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |this, _: &MouseDownEvent, _, cx| this.viewer_goto(id, cx)),
                );
            if let Some(path) = thumb {
                cell = cell.child(
                    img(path)
                        .object_fit(ObjectFit::Cover)
                        .size_full()
                        .with_fallback(|| div().size_full().into_any_element()),
                );
            }
            row = row.child(cell);
        }
        div()
            .id(ids::VIEWER_FILMSTRIP)
            .test_support()
            .relative()
            .overflow_hidden()
            .h(px(FILMSTRIP_HEIGHT))
            .w_full()
            .border_t_1()
            .border_color(theme.border)
            .child(row)
            .into_any_element()
    }

    fn render_viewer_info(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let lines = self
            .viewer
            .as_ref()
            .and_then(|v| v.photo.as_ref())
            .map(info_lines)
            .unwrap_or_default();
        div()
            .id(ids::VIEWER_INFO_PANEL)
            .test_support()
            .w(px(INFO_WIDTH))
            .h_full()
            .flex_none()
            .p_3()
            .flex()
            .flex_col()
            .gap_1()
            .border_l_1()
            .border_color(theme.border)
            .children(lines.into_iter().map(|l| div().child(l)))
            .into_any_element()
    }
}

fn shown_for(v: &ViewerState, id: PhotoId) -> Option<&lb_app::PreviewFile> {
    let prefix = format!("{}:", id.get());
    v.stage.shown_if(|k| k.starts_with(&prefix))
}

/// The text lines of an info panel.
pub(crate) fn info_lines(p: &Photo) -> Vec<String> {
    let mut v = vec![
        p.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default(),
        format_size(p.size),
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
    if let Some(g) = &p.gps {
        v.push(format!("{:.5}, {:.5}", g.lat, g.lon));
    }
    if let Some(c) = &p.caption {
        v.push(c.clone());
    }
    v
}

fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut value = bytes as f64;
    let mut unit = 0;
    while value >= 1024.0 && unit + 1 < UNITS.len() {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{bytes} B")
    } else {
        format!("{value:.1} {}", UNITS[unit])
    }
}
