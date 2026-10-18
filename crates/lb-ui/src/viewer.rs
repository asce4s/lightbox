//! Photo viewer overlay: stage, chrome and the open/close motion (SPEC-VIEWER §V3, §V8).

use crate::ids;
use crate::render::GRID_PADDING;
use crate::stage::{RenderJob, Stage, StageKind, frame_key};
use crate::window::LightboxWindow;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::Button;
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, ClipboardItem, Context, Image, ImageFormat, IntoElement, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ObjectFit, Pixels, Point, ScrollWheelEvent,
    TestSupportExt as _, Window, div, hsla, img, point, px,
};
use lb_app::ThumbSize;
use lb_app::viewer::{
    Clip, ClipboardBody, DRAG_THRESHOLD_PX, RectF, clipboard_for_path, decode_rgba, entry_error,
    fit_area, fit_rect, fit_scale, has_alpha, image_for_clipboard, read_animation,
    save_checker_png, save_rgba_png, stage_insets,
};
use lb_app::vm::{Modifiers, ViewerVm, ZoomMode};
use lb_core::{EditStack, Photo, PhotoId};
use std::path::PathBuf;
use std::time::Duration;

/// Height of the viewer's top bar. Matches [`lb_app::viewer::TOP_BAR_HEIGHT`].
pub const VIEWER_BAR_HEIGHT: f32 = 48.0;
/// Height of the filmstrip.
pub const FILMSTRIP_HEIGHT: f32 = 84.0;
/// Width of one filmstrip cell including its gap.
const FILM_STEP: f32 = 72.0;
const FILM_CELL: f32 = 64.0;
/// Longest edge of the proxy shown while fitting the window.
const PROXY_EDGE: u32 = 2560;
/// Longest edge rendered when zoomed in.
const ZOOM_EDGE: u32 = 8192;
const CHROME_TEXT: f32 = 0.949;
const CHROME_MUTED: f32 = 0.69;

/// Inputs for the floating toolbar. Kept together so the render method stays under clippy's
/// argument limit.
struct ToolbarView<'a> {
    id: PhotoId,
    catalog: bool,
    has_prev: bool,
    has_next: bool,
    info_open: bool,
    zoom_label: &'a str,
    chrome: f32,
}

/// A press inside the stage, used to tell a click from a drag.
struct PointerPress {
    origin: (f32, f32),
    last: (f32, f32),
    dragging: bool,
}

/// State of the open viewer.
pub(crate) struct ViewerState {
    pub(crate) vm: ViewerVm,
    pub(crate) stage: Stage,
    pub(crate) photo: Option<Photo>,
    pub(crate) stack: EditStack,
    pub(crate) opened_id: PhotoId,
    pub(crate) opened_at: Duration,
    pub(crate) closing: bool,
    pub(crate) error: Option<&'static str>,
    outgoing: Option<PathBuf>,
    press: Option<PointerPress>,
    playback: Option<Clip>,
    playback_tried: bool,
    frame_files: Vec<PathBuf>,
    checker: Option<PathBuf>,
    checker_tried: bool,
}

impl LightboxWindow {
    // ---- state ----

    /// File name shown in the top bar and the standalone window title.
    pub(crate) fn viewer_file_name(&self) -> String {
        self.viewer
            .as_ref()
            .map(entry_file_name)
            .unwrap_or_default()
    }

    /// `{file name} — Lightbox`, the standalone window title (SPEC-VIEWER §V3.2).
    pub fn standalone_title(&self) -> String {
        let name = self.viewer_file_name();
        if name.is_empty() {
            "Lightbox".to_owned()
        } else {
            format!("{name} — Lightbox")
        }
    }

    /// The viewer's view-model while the viewer is open.
    pub fn viewer(&self) -> Option<&ViewerVm> {
        self.viewer.as_ref().map(|v| &v.vm)
    }

    /// The image file the viewer currently draws (a thumbnail until the proxy is rendered).
    pub fn viewer_image(&self) -> Option<PathBuf> {
        let v = self.viewer.as_ref()?;
        let id = v.vm.current();
        shown_for(v, id).map(|f| f.path.clone()).or_else(|| {
            self.thumbs
                .get(&(id, ThumbSize::L))
                .or_else(|| self.thumbs.get(&(id, ThumbSize::M)))
                .or_else(|| self.thumbs.get(&(id, ThumbSize::S)))
                .cloned()
        })
    }

    /// Window-space rectangle of the thumbnail content for `id`, if that cell is on screen.
    pub fn thumbnail_content_rect(&self, id: PhotoId) -> Option<RectF> {
        self.vm
            .thumbnail_rect(id, self.grid_content_origin(), self.grid_scroll())
    }

    fn grid_content_origin(&self) -> (f32, f32) {
        let bounds = self.grid_bounds.get();
        (
            f32::from(bounds.origin.x) + GRID_PADDING,
            f32::from(bounds.origin.y),
        )
    }

    fn grid_scroll(&self) -> f32 {
        (-f32::from(self.scroll.offset().y)).max(0.0)
    }

    fn now(&self) -> Duration {
        self.app.clock().elapsed()
    }

    /// The library grid is drawn under the stage until the backdrop is opaque, and again while
    /// the viewer is closing (SPEC-VIEWER §V3.1).
    pub(crate) fn viewer_shows_library(&self) -> bool {
        let Some(v) = &self.viewer else {
            return false;
        };
        v.closing || v.vm.presented(self.now()).backdrop_opacity < 0.999
    }

    /// Another pump should redraw while a transition or a close is in progress.
    pub(crate) fn viewer_needs_frame(&self) -> bool {
        let Some(v) = &self.viewer else {
            return false;
        };
        let now = self.now();
        v.closing
            || v.vm.is_animating(now)
            || v.playback.as_ref().is_some_and(|clip| !clip.is_paused())
    }

    /// Drops the viewer once the close transition has finished.
    pub(crate) fn complete_viewer_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let done = self
            .viewer
            .as_ref()
            .is_some_and(|v| v.closing && !v.vm.is_animating(self.now()));
        if done {
            self.finish_close(window, cx);
        }
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
            if let Err(e) = self.app.photos().open_externally(id) {
                self.notify_message(e.user_message(), cx);
            }
            return;
        }
        let ids = self.vm.order().to_vec();
        let Some(vm) = ViewerVm::open(&self.app, ids, id) else {
            return;
        };
        let source = self.thumbnail_content_rect(id);
        let now = self.now();
        let photo = vm.photo().ok().flatten();
        let error = photo.as_ref().and_then(|p| entry_error(&p.path));
        self.viewer = Some(ViewerState {
            vm,
            stage: Stage::new(),
            photo,
            stack: EditStack::new(),
            opened_id: id,
            opened_at: now,
            closing: false,
            error,
            outgoing: None,
            press: None,
            playback: None,
            playback_tried: false,
            frame_files: Vec::new(),
            checker: None,
            checker_tried: false,
        });
        self.opened = Some(id);
        self.viewer_reload(cx);
        let fit = self.current_fit();
        if let Some(v) = &mut self.viewer {
            v.vm.set_stage(self.viewport);
            v.vm.begin_open(now, source, fit);
        }
        cx.notify();
    }

    /// Opens the viewer on files from a second launch. There is no thumbnail to animate from.
    pub fn open_external_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        let Ok(vm) = ViewerVm::open_paths(&self.app, paths) else {
            return;
        };
        let now = self.now();
        let error = entry_error(&vm.current_entry().path);
        let opened_id = vm.current();
        self.viewer = Some(ViewerState {
            vm,
            stage: Stage::new(),
            photo: None,
            stack: EditStack::new(),
            opened_id,
            opened_at: now,
            closing: false,
            error,
            outgoing: None,
            press: None,
            playback: None,
            playback_tried: false,
            frame_files: Vec::new(),
            checker: None,
            checker_tried: false,
        });
        self.opened = Some(opened_id);
        self.viewer_reload(cx);
        let fit = self.current_fit();
        if let Some(v) = &mut self.viewer {
            v.vm.set_stage(self.viewport);
            v.vm.begin_open(now, None, fit);
        }
        cx.notify();
    }

    pub(crate) fn viewer_reload(&mut self, cx: &mut Context<Self>) {
        let Some(v) = &self.viewer else { return };
        let id = v.vm.current();
        let photo = v.vm.photo().ok().flatten();
        let stack = self.app.edits().stack(id).unwrap_or_default();
        let error = self
            .viewer
            .as_ref()
            .and_then(|v| entry_error(&v.vm.current_entry().path));
        if let Some(v) = &mut self.viewer {
            v.photo = photo;
            v.stack = stack;
            v.error = error;
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
        if id.get() == 0 {
            return;
        }
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
        self.thumb_for_size(id, ThumbSize::S);
    }

    fn current_fit(&self) -> RectF {
        let info = self.viewer.as_ref().is_some_and(|v| v.vm.info_visible());
        let area = fit_area(self.viewport, stage_insets(self.viewport, true, info));
        fit_rect(self.viewer_image_size(), area, false)
    }

    /// The scale that fits the current photo into the fit area.
    pub fn viewer_fit_scale(&self) -> f32 {
        let info = self.viewer.as_ref().is_some_and(|v| v.vm.info_visible());
        let area = fit_area(self.viewport, stage_insets(self.viewport, true, info));
        fit_scale(self.viewer_image_size(), area, false)
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

    fn note_activity(&mut self) {
        let now = self.now();
        if let Some(v) = &mut self.viewer {
            v.vm.motion_mut().key_pressed(now);
        }
    }

    fn viewer_after_move(
        &mut self,
        moved: bool,
        dir: i32,
        outgoing: Option<PathBuf>,
        cx: &mut Context<Self>,
    ) {
        let now = self.now();
        if !moved {
            if let Some(v) = &mut self.viewer {
                v.vm.motion_mut().bounce(now, dir);
            }
            cx.notify();
            return;
        }
        self.viewer_reload(cx);
        let fit = self.current_fit();
        if let Some(v) = &mut self.viewer {
            v.outgoing = outgoing;
            v.playback = None;
            v.playback_tried = false;
            v.frame_files.clear();
            v.checker = None;
            v.checker_tried = false;
            v.vm.set_stage(self.viewport);
            v.vm.motion_mut().navigate(now, dir, fit);
        }
        cx.notify();
    }

    /// Right arrow.
    pub fn viewer_next(&mut self, cx: &mut Context<Self>) {
        if self.viewer.as_ref().is_some_and(|v| v.closing) {
            return;
        }
        let outgoing = self.displayed_image();
        let now = self.now();
        let can = self.viewer.as_ref().is_some_and(|v| v.vm.has_next());
        if !can {
            if let Some(v) = &mut self.viewer {
                v.vm.motion_mut().bounce(now, 1);
            }
            cx.notify();
            return;
        }
        let moved = self.viewer.as_mut().is_some_and(|v| v.vm.go_next());
        self.viewer_after_move(moved, 1, outgoing, cx);
    }

    /// Left arrow.
    pub fn viewer_prev(&mut self, cx: &mut Context<Self>) {
        if self.viewer.as_ref().is_some_and(|v| v.closing) {
            return;
        }
        let outgoing = self.displayed_image();
        let now = self.now();
        let can = self.viewer.as_ref().is_some_and(|v| v.vm.has_prev());
        if !can {
            if let Some(v) = &mut self.viewer {
                v.vm.motion_mut().bounce(now, -1);
            }
            cx.notify();
            return;
        }
        let moved = self.viewer.as_mut().is_some_and(|v| v.vm.go_prev());
        self.viewer_after_move(moved, -1, outgoing, cx);
    }

    /// Home.
    pub fn viewer_home(&mut self, cx: &mut Context<Self>) {
        let from = self.viewer.as_ref().map(|v| v.vm.index()).unwrap_or(0);
        if from == 0 {
            self.viewer_prev(cx);
            return;
        }
        let outgoing = self.displayed_image();
        let moved = self.viewer.as_mut().is_some_and(|v| v.vm.home());
        self.viewer_after_move(moved, -1, outgoing, cx);
    }

    /// End.
    pub fn viewer_end(&mut self, cx: &mut Context<Self>) {
        let (from, last) = self
            .viewer
            .as_ref()
            .map(|v| (v.vm.index(), v.vm.filmstrip().len().saturating_sub(1)))
            .unwrap_or((0, 0));
        if from >= last {
            self.viewer_next(cx);
            return;
        }
        let outgoing = self.displayed_image();
        let moved = self.viewer.as_mut().is_some_and(|v| v.vm.end());
        self.viewer_after_move(moved, 1, outgoing, cx);
    }

    /// A click on a filmstrip cell.
    pub fn viewer_goto(&mut self, id: PhotoId, cx: &mut Context<Self>) {
        let from = self.viewer.as_ref().map(|v| v.vm.index());
        let to = self
            .viewer
            .as_ref()
            .and_then(|v| v.vm.filmstrip().iter().position(|p| *p == id));
        let (Some(from), Some(to)) = (from, to) else {
            return;
        };
        if from == to {
            return;
        }
        let outgoing = self.displayed_image();
        let dir = if to > from { 1 } else { -1 };
        let moved = self.viewer.as_mut().is_some_and(|v| v.vm.goto(id));
        self.viewer_after_move(moved, dir, outgoing, cx);
    }

    /// `I`.
    pub fn viewer_toggle_info(&mut self, cx: &mut Context<Self>) {
        let now = self.now();
        let fit_mode = self
            .viewer
            .as_ref()
            .is_some_and(|v| v.vm.zoom() == ZoomMode::Fit);
        if let Some(v) = &mut self.viewer {
            v.vm.toggle_info();
        }
        if fit_mode {
            let fit = self.current_fit();
            if let Some(v) = &mut self.viewer {
                v.vm.motion_mut().refit(now, fit);
            }
        }
        cx.notify();
    }

    fn animate_zoom(&mut self, cx: &mut Context<Self>) {
        let now = self.now();
        let fit = self.viewer_fit_scale();
        let image = self.viewer_image_size();
        let anchor = (self.viewport.0 * 0.5, self.viewport.1 * 0.5);
        if let Some(v) = &mut self.viewer {
            let scale = v.vm.zoom().scale(fit);
            v.vm.motion_mut()
                .zoom_to(now, image, anchor, scale.max(1.0e-3));
        }
        self.sync_viewer(cx);
        cx.notify();
    }

    /// `+` and wheel up.
    pub fn viewer_zoom_in(&mut self, cx: &mut Context<Self>) {
        let fit = self.viewer_fit_scale();
        if let Some(v) = &mut self.viewer {
            v.vm.zoom_in(fit);
        }
        self.animate_zoom(cx);
    }

    /// `-` and wheel down.
    pub fn viewer_zoom_out(&mut self, cx: &mut Context<Self>) {
        let fit = self.viewer_fit_scale();
        if let Some(v) = &mut self.viewer {
            v.vm.zoom_out(fit);
        }
        if self
            .viewer
            .as_ref()
            .is_some_and(|v| v.vm.zoom() == ZoomMode::Fit)
        {
            let now = self.now();
            let fit_rect = self.current_fit();
            if let Some(v) = &mut self.viewer {
                v.vm.motion_mut().refit(now, fit_rect);
            }
            cx.notify();
            return;
        }
        self.animate_zoom(cx);
    }

    /// `1` and double-click: fit becomes 100% and back.
    pub fn viewer_toggle_zoom(&mut self, cx: &mut Context<Self>) {
        if let Some(v) = &mut self.viewer {
            v.vm.toggle_fit_actual();
        }
        if self
            .viewer
            .as_ref()
            .is_some_and(|v| v.vm.zoom() == ZoomMode::Fit)
        {
            let now = self.now();
            let fit = self.current_fit();
            if let Some(v) = &mut self.viewer {
                v.vm.motion_mut().refit(now, fit);
            }
            self.sync_viewer(cx);
            cx.notify();
            return;
        }
        self.animate_zoom(cx);
    }

    /// `0`.
    pub fn viewer_fit(&mut self, cx: &mut Context<Self>) {
        if let Some(v) = &mut self.viewer {
            v.vm.zoom_to_fit();
        }
        let now = self.now();
        let fit = self.current_fit();
        if let Some(v) = &mut self.viewer {
            v.vm.motion_mut().refit(now, fit);
        }
        self.sync_viewer(cx);
        cx.notify();
    }

    /// The 1:1 button.
    pub fn viewer_actual(&mut self, cx: &mut Context<Self>) {
        if let Some(v) = &mut self.viewer {
            v.vm.zoom_to_actual();
        }
        self.animate_zoom(cx);
    }

    /// Star for a catalog photo. Non-catalog entries are ignored until M13.7 fills the hint.
    pub fn viewer_star(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.viewer.as_ref().and_then(|v| v.vm.current_photo()) else {
            return;
        };
        self.vm.click(id, Modifiers::NONE);
        self.toggle_star_targets(cx);
    }

    /// Rotate. Catalog photos push an edit; other files rotate for this session only.
    pub fn viewer_rotate(&mut self, turns: i8, cx: &mut Context<Self>) {
        let ok = self
            .viewer
            .as_mut()
            .is_some_and(|v| v.vm.rotate(turns).is_ok());
        if ok {
            self.viewer_reload(cx);
        }
    }

    /// Delete asks for confirmation, then the existing trash path.
    pub fn viewer_delete(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.viewer.as_ref().and_then(|v| v.vm.current_photo()) else {
            return;
        };
        self.vm.click(id, Modifiers::NONE);
        self.ask_delete(cx);
    }

    fn scroll_photo_into_view(&mut self, id: PhotoId) {
        let Some((group, index)) = self.vm.position_of(id) else {
            return;
        };
        let Some(cell) = self.vm.layout().cell_rect(group, index) else {
            return;
        };
        let view_h = self.vm.viewport_height();
        let scroll = self.grid_scroll();
        let bottom = cell.y + cell.h;
        let new_scroll = if cell.y < scroll {
            cell.y
        } else if bottom > scroll + view_h {
            (bottom - view_h).max(0.0)
        } else {
            scroll
        };
        if (new_scroll - scroll).abs() > 0.5 {
            self.scroll.set_offset(point(px(0.0), px(-new_scroll)));
        }
    }

    fn begin_close(&mut self, cx: &mut Context<Self>) {
        let now = self.now();
        let Some(v) = &self.viewer else { return };
        if v.closing {
            return;
        }
        let id = v.vm.current();
        let opened = v.opened_id;
        self.vm.click(id, Modifiers::NONE);
        if id != opened {
            self.scroll_photo_into_view(id);
        }
        let target = self.thumbnail_content_rect(id);
        let Some(v) = &mut self.viewer else { return };
        v.closing = true;
        v.press = None;
        match target {
            Some(rect) => v.vm.motion_mut().close_to_thumbnail(now, rect),
            None => v.vm.motion_mut().close_fade(now),
        }
        cx.notify();
    }

    fn finish_close(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut v) = self.viewer.take() else {
            return;
        };
        v.stage.cancel();
        let id = v.vm.current();
        if id.get() != 0 {
            self.vm.click(id, Modifiers::NONE);
            self.reveal_focused();
        }
        if self.standalone {
            window.remove_window();
            cx.notify();
            return;
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Esc or backdrop: play the close transition, then return to the grid.
    pub fn close_viewer(&mut self, _window: &mut Window, cx: &mut Context<Self>) {
        self.begin_close(cx);
    }

    pub(crate) fn viewer_key(
        &mut self,
        key: &str,
        shift: bool,
        cmd: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.note_activity();
        match key {
            "left" | "pageup" => self.viewer_prev(cx),
            "right" | "pagedown" => self.viewer_next(cx),
            "space" => {
                if shift {
                    self.viewer_prev(cx);
                } else {
                    self.viewer_next(cx);
                }
            }
            "home" => self.viewer_home(cx),
            "end" => self.viewer_end(cx),
            "escape" => self.close_viewer(window, cx),
            "f" | "f11" if self.standalone && !cmd => window.toggle_fullscreen(),
            "i" => self.viewer_toggle_info(cx),
            "+" | "=" => self.viewer_zoom_in(cx),
            "-" => self.viewer_zoom_out(cx),
            "0" if !shift => self.viewer_fit(cx),
            "1" => self.viewer_toggle_zoom(cx),
            "r" if cmd => self.viewer_rotate(if shift { -1 } else { 1 }, cx),
            "s" if !cmd => self.viewer_star(cx),
            "e" if !cmd => {
                if let Some(id) = self.viewer.as_ref().and_then(|v| v.vm.current_photo()) {
                    self.open_editor(id, window, cx);
                }
            }
            "enter" | "f5" => self.viewer_play(window, cx),
            "delete" | "backspace" => self.viewer_delete(cx),
            "p" if !cmd => self.viewer_toggle_playback(cx),
            "c" if cmd => self.copy_viewer_image(cx),
            _ => {}
        }
    }

    /// `P` pauses or resumes a GIF or WebP. Still images are unchanged.
    fn viewer_toggle_playback(&mut self, cx: &mut Context<Self>) {
        self.ensure_playback();
        let now = self.now();
        if let Some(clip) = self.viewer.as_mut().and_then(|v| v.playback.as_mut()) {
            clip.toggle(now);
        }
        cx.notify();
    }

    /// Ctrl/Cmd+C copies the current image (the playing frame, when there is one).
    fn copy_viewer_image(&mut self, cx: &mut Context<Self>) {
        let now = self.now();
        let frame = self
            .viewer
            .as_ref()
            .and_then(|v| v.playback.as_ref())
            .and_then(|clip| clip.frame(now).cloned());
        let Some(path) = self
            .viewer
            .as_ref()
            .map(|v| v.vm.current_entry().path.clone())
        else {
            return;
        };
        let body = if let Some(image) = &frame {
            image_for_clipboard(image, &path, true)
        } else {
            clipboard_for_path(&path)
        };
        match body {
            Ok(ClipboardBody::Png(bytes)) => {
                cx.write_to_clipboard(ClipboardItem::new_image(&Image {
                    format: ImageFormat::Png,
                    bytes,
                    id: 1,
                }));
            }
            Ok(ClipboardBody::Path(file)) => {
                cx.write_to_clipboard(ClipboardItem::new_string(file.display().to_string()));
            }
            Err(error) => {
                tracing::warn!(%error, "could not copy the viewer image");
            }
        }
    }

    /// Loads animation frames and a checkerboard composite the first time they are needed.
    fn prepare_viewer_pixels(&mut self) {
        self.ensure_playback();
        self.ensure_frame_file();
        self.ensure_checker();
    }

    fn ensure_playback(&mut self) {
        let Some(v) = &self.viewer else {
            return;
        };
        if v.playback_tried || v.playback.is_some() {
            return;
        }
        let path = v.vm.current_entry().path.clone();
        if !is_gif_or_webp(&path) {
            if let Some(v) = &mut self.viewer {
                v.playback_tried = true;
            }
            return;
        }
        let now = self.now();
        match read_animation(&path) {
            Ok(Some(mut clip)) => {
                clip.restart(now);
                if let Some(v) = &mut self.viewer {
                    v.playback = Some(clip);
                    v.playback_tried = true;
                    v.frame_files.clear();
                }
            }
            Ok(None) => {
                if let Some(v) = &mut self.viewer {
                    v.playback_tried = true;
                }
            }
            Err(error) => {
                tracing::warn!(%error, "could not read an animation");
                if let Some(v) = &mut self.viewer {
                    v.playback_tried = true;
                }
            }
        }
    }

    fn ensure_frame_file(&mut self) {
        let now = self.now();
        let prepared = {
            let Some(v) = &self.viewer else {
                return;
            };
            let Some(clip) = &v.playback else {
                return;
            };
            let index = clip.index(now);
            if v.frame_files
                .get(index)
                .is_some_and(|path| !path.as_os_str().is_empty())
            {
                return;
            }
            clip.frame(now).cloned().map(|image| (index, image))
        };
        let Some((index, image)) = prepared else {
            return;
        };
        let path = self
            .app
            .paths()
            .cache
            .join("viewer")
            .join("frames")
            .join(format!("{index}.png"));
        let saved = if has_alpha(&image) {
            save_checker_png(&path, &image)
        } else {
            save_rgba_png(&path, &image)
        };
        if let Err(error) = saved {
            tracing::warn!(%error, "could not cache an animation frame");
            return;
        }
        if let Some(v) = &mut self.viewer {
            if v.frame_files.len() <= index {
                v.frame_files.resize_with(index + 1, PathBuf::new);
            }
            v.frame_files[index] = path;
        }
    }

    fn ensure_checker(&mut self) {
        let Some(v) = &self.viewer else {
            return;
        };
        if v.checker_tried || v.playback.is_some() {
            return;
        }
        let path = v.vm.current_entry().path.clone();
        let ext = extension_lower(&path);
        if ext != "png" && ext != "webp" {
            if let Some(v) = &mut self.viewer {
                v.checker_tried = true;
            }
            return;
        }
        let decoded = decode_rgba(&path);
        let Ok(image) = decoded else {
            if let Some(v) = &mut self.viewer {
                v.checker_tried = true;
            }
            return;
        };
        if !has_alpha(&image) {
            if let Some(v) = &mut self.viewer {
                v.checker_tried = true;
            }
            return;
        }
        let out = self
            .app
            .paths()
            .cache
            .join("viewer")
            .join(format!("checker-{}.png", v.vm.index()));
        if let Err(error) = save_checker_png(&out, &image) {
            tracing::warn!(%error, "could not composite the checkerboard");
            if let Some(v) = &mut self.viewer {
                v.checker_tried = true;
            }
            return;
        }
        if let Some(v) = &mut self.viewer {
            v.checker = Some(out);
            v.checker_tried = true;
        }
    }

    /// The file drawn for the current entry: a cached frame, a checkerboard composite, or the proxy.
    fn displayed_image(&self) -> Option<PathBuf> {
        let v = self.viewer.as_ref()?;
        if let Some(clip) = &v.playback {
            let index = clip.index(self.now());
            if let Some(path) = v.frame_files.get(index)
                && !path.as_os_str().is_empty()
            {
                return Some(path.clone());
            }
        }
        if let Some(path) = &v.checker {
            return Some(path.clone());
        }
        self.viewer_image()
    }

    fn viewer_play(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.viewer.as_ref().and_then(|v| v.vm.current_photo()) {
            self.vm.click(id, Modifiers::NONE);
        }
        self.start_slideshow(window, cx);
    }

    fn presented_now(&self) -> Option<lb_app::viewer::Presented> {
        self.viewer.as_ref().map(|v| v.vm.presented(self.now()))
    }

    fn point_on_image(&self, at: (f32, f32)) -> bool {
        self.presented_now()
            .and_then(|p| p.image)
            .is_some_and(|layer| layer.rect.contains(at))
    }

    fn viewer_pointer_down(&mut self, ev: &MouseDownEvent, cx: &mut Context<Self>) {
        if ev.button != MouseButton::Left {
            return;
        }
        let at = (f32::from(ev.position.x), f32::from(ev.position.y));
        if ev.click_count >= 2 && self.point_on_image(at) {
            self.viewer_toggle_zoom(cx);
        }
        let now = self.now();
        if let Some(v) = &mut self.viewer {
            v.press = Some(PointerPress {
                origin: at,
                last: at,
                dragging: false,
            });
            v.vm.motion_mut().pointer_moved(now);
        }
    }

    fn viewer_pointer_move(&mut self, ev: &MouseMoveEvent, cx: &mut Context<Self>) {
        let now = self.now();
        let at = (f32::from(ev.position.x), f32::from(ev.position.y));
        let zoomed = self
            .viewer
            .as_ref()
            .is_some_and(|v| v.vm.zoom() != ZoomMode::Fit);
        let delta = {
            let Some(v) = &mut self.viewer else { return };
            v.vm.motion_mut().pointer_moved(now);
            let Some(press) = v.press.as_mut() else {
                cx.notify();
                return;
            };
            if ev.pressed_button != Some(MouseButton::Left) {
                return;
            }
            let travel = (at.0 - press.origin.0)
                .abs()
                .max((at.1 - press.origin.1).abs());
            if travel > DRAG_THRESHOLD_PX {
                press.dragging = true;
            }
            let delta = (at.0 - press.last.0, at.1 - press.last.1);
            press.last = at;
            if press.dragging { Some(delta) } else { None }
        };
        if let Some((dx, dy)) = delta
            && zoomed
            && let Some(v) = &mut self.viewer
        {
            v.vm.pan_by(dx, dy);
            v.vm.motion_mut().translate_image(now, dx, dy);
        }
        cx.notify();
    }

    fn viewer_pointer_up(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let at = (f32::from(position.x), f32::from(position.y));
        let Some(press) = self.viewer.as_mut().and_then(|v| v.press.take()) else {
            return;
        };
        let travel = (at.0 - press.origin.0)
            .abs()
            .max((at.1 - press.origin.1).abs());
        // Chrome controls sit above the backdrop hit target, and a button click does not
        // stop the event, so the backdrop also sees it. A press that started on the toolbar,
        // filmstrip or info panel is not a backdrop click.
        if press.dragging
            || travel > DRAG_THRESHOLD_PX
            || self.point_on_image(at)
            || self.point_on_chrome(press.origin)
        {
            return;
        }
        self.begin_close(cx);
    }

    /// Toolbar, filmstrip and info panel occupy these bands (SPEC-VIEWER §V8).
    ///
    /// The top bar is not included: only its buttons are hit targets, and a click in the
    /// empty top-left (the backdrop) closes.
    fn point_on_chrome(&self, at: (f32, f32)) -> bool {
        let (w, h) = self.viewport;
        let film_top = h - FILMSTRIP_HEIGHT;
        if at.1 >= film_top {
            return true;
        }
        let toolbar_bottom = film_top - 12.0;
        let toolbar_top = toolbar_bottom - 56.0;
        if at.1 >= toolbar_top && at.1 < toolbar_bottom {
            return true;
        }
        self.viewer.as_ref().is_some_and(|v| v.vm.info_visible())
            && at.0 >= w - lb_app::viewer::INFO_PANEL_WIDTH
    }

    // ---- rendering ----

    pub(crate) fn render_viewer(&mut self, cx: &mut Context<Self>) -> AnyElement {
        if self.viewer.is_none() {
            return div().into_any_element();
        }
        self.prepare_viewer_pixels();
        let now = self.now();
        let Some(v) = &self.viewer else {
            return div().into_any_element();
        };
        let id = v.vm.current();
        if id.get() != 0 {
            self.thumb_for_size(id, ThumbSize::L);
            self.thumb_for_size(id, ThumbSize::S);
        }
        let Some(v) = &self.viewer else {
            return div().into_any_element();
        };
        let presented = v.vm.presented(now);
        let name = entry_file_name(v);
        let counter = format!("{} / {}", v.vm.index() + 1, v.vm.filmstrip().len());
        let info_open = v.vm.info_visible();
        let (has_prev, has_next) = (v.vm.has_prev(), v.vm.has_next());
        let catalog = v.vm.current_photo().is_some();
        let zoom_label = match v.vm.zoom() {
            ZoomMode::Fit => "Fit".to_owned(),
            ZoomMode::Actual => "100%".to_owned(),
            ZoomMode::Factor(f) => format!("{:.0}%", f * 100.0),
        };
        let error = v.error;
        let loading = error.is_none()
            && self.viewer_image().is_none()
            && now.saturating_sub(v.opened_at) >= Duration::from_millis(120);
        let chrome = presented.chrome_opacity;
        let hint = self.viewer.as_ref().and_then(|v| v.vm.rotation_hint());
        let text = hsla(0.0, 0.0, CHROME_TEXT, 1.0);
        let muted = hsla(0.0, 0.0, CHROME_MUTED, 1.0);
        let path = if error.is_some() {
            None
        } else {
            self.displayed_image()
        };
        let outgoing = self.viewer.as_ref().and_then(|v| v.outgoing.clone());
        let (stage_w, stage_h) = self.viewport;

        let mut backdrop = div()
            .id(ids::VIEWER_BACKDROP)
            .test_support()
            .absolute()
            .top_0()
            .left_0()
            .size_full()
            .bg(hsla(0.0, 0.0, 0.0, 0.88 * presented.backdrop_opacity))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, _, cx| this.viewer_pointer_down(ev, cx)),
            )
            .on_mouse_move(
                cx.listener(|this, ev: &MouseMoveEvent, _, cx| this.viewer_pointer_move(ev, cx)),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseUpEvent, _, cx| {
                    this.viewer_pointer_up(ev.position, cx);
                }),
            );
        if let Some(layer) = presented.outgoing
            && let Some(outgoing) = outgoing
        {
            backdrop = backdrop.child(placed_image(Some(outgoing), layer));
        }

        let mut stage = div()
            .id(ids::VIEWER_STAGE)
            .test_support()
            .track_focus(&self.viewer_focus)
            .absolute()
            .top_0()
            .left_0()
            .w(px(stage_w))
            .h(px(stage_h))
            .overflow_hidden()
            .text_color(text)
            .on_mouse_move(
                cx.listener(|this, ev: &MouseMoveEvent, _, cx| this.viewer_pointer_move(ev, cx)),
            )
            .on_scroll_wheel(cx.listener(|this, ev: &ScrollWheelEvent, _, cx| {
                let dy = f32::from(ev.delta.pixel_delta(px(16.0)).y);
                if dy > 0.0 {
                    this.viewer_zoom_in(cx);
                } else if dy < 0.0 {
                    this.viewer_zoom_out(cx);
                }
            }))
            .child(backdrop);

        if error.is_none()
            && let Some(layer) = presented.image
        {
            let image = placed_image(path.clone(), layer)
                .id(ids::VIEWER_IMAGE)
                .test_support()
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, ev: &MouseDownEvent, _, cx| {
                        this.viewer_pointer_down(ev, cx);
                    }),
                )
                .on_mouse_move(cx.listener(|this, ev: &MouseMoveEvent, _, cx| {
                    this.viewer_pointer_move(ev, cx);
                }))
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, ev: &MouseUpEvent, _, cx| {
                        this.viewer_pointer_up(ev.position, cx);
                    }),
                );
            stage = stage.child(image);
        }
        if let Some(reason) = error {
            let rect = presented
                .image
                .map(|l| l.rect)
                .unwrap_or_else(|| self.current_fit());
            stage = stage.child(self.render_error_card(&name, reason, rect, cx));
        }
        if loading {
            stage = stage.child(
                div()
                    .id(ids::VIEWER_LOADING)
                    .test_support()
                    .aria_label("Loading")
                    .absolute()
                    .top(px(stage_h * 0.5))
                    .left(px(stage_w * 0.5))
                    .text_color(muted)
                    .child("Loading…"),
            );
        }

        if let Some(hint) = hint {
            stage = stage.child(
                div()
                    .id(ids::VIEWER_HINT)
                    .test_support()
                    .aria_label(hint)
                    .absolute()
                    .top(px(VIEWER_BAR_HEIGHT + 8.0))
                    .left_0()
                    .right_0()
                    .flex()
                    .justify_center()
                    .child(
                        div()
                            .px_3()
                            .py_1()
                            .rounded_md()
                            .bg(hsla(0.0, 0.0, 0.08, 0.88))
                            .child(hint),
                    ),
            );
        }
        stage = stage
            .child(self.render_viewer_top(&name, &counter, chrome, text, cx))
            .child(self.render_viewer_toolbar(
                ToolbarView {
                    id,
                    catalog,
                    has_prev,
                    has_next,
                    info_open,
                    zoom_label: &zoom_label,
                    chrome,
                },
                cx,
            ))
            .child(self.render_filmstrip_overlay(chrome, cx))
            .child(
                div()
                    .id(ids::VIEWER)
                    .test_support()
                    .absolute()
                    .top_0()
                    .left_0()
                    .w(px(1.0))
                    .h(px(1.0)),
            );
        if info_open {
            stage = stage.child(self.render_viewer_info(chrome, cx));
        }
        stage.into_any_element()
    }

    fn render_error_card(
        &self,
        name: &str,
        reason: &'static str,
        rect: RectF,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .id(ids::VIEWER_ERROR_CARD)
            .test_support()
            .aria_label(reason)
            .absolute()
            .left(px(rect.x))
            .top(px(rect.y))
            .w(px(rect.w.max(220.0)))
            .h(px(rect.h.max(120.0)))
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_2()
            .p_4()
            .bg(hsla(0.0, 0.0, 0.08, 0.92))
            .text_color(hsla(0.0, 0.0, CHROME_TEXT, 1.0))
            .rounded_md()
            .child(div().child(name.to_owned()))
            .child(
                div()
                    .text_color(hsla(0.0, 0.0, CHROME_MUTED, 1.0))
                    .child(reason),
            )
            .child(
                Button::new("viewer-show-folder")
                    .label("Show in folder")
                    .on_click(cx.listener(|this, _, _, cx| {
                        if let Some(id) = this.viewer.as_ref().and_then(|v| v.vm.current_photo()) {
                            let result = this.app.photos().reveal(id);
                            this.report(result, cx);
                        }
                    })),
            )
            .into_any_element()
    }

    fn render_viewer_top(
        &self,
        name: &str,
        counter: &str,
        chrome: f32,
        text: gpui_kit::Hsla,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        div()
            .absolute()
            .top_0()
            .left_0()
            .right_0()
            .h(px(VIEWER_BAR_HEIGHT))
            .flex()
            .items_center()
            .gap_3()
            .px_3()
            .opacity(chrome)
            .bg(hsla(0.0, 0.0, 0.0, 0.45))
            .text_color(text)
            .child(
                div()
                    .id(ids::VIEWER_TITLE)
                    .test_support()
                    .aria_label(name.to_owned())
                    .child(name.to_owned()),
            )
            .child(
                div()
                    .id(ids::VIEWER_COUNTER)
                    .test_support()
                    .aria_label(counter.to_owned())
                    .text_color(hsla(0.0, 0.0, CHROME_MUTED, 1.0))
                    .child(counter.to_owned()),
            )
            .child(div().flex_1())
            .child(
                Button::new(ids::VIEWER_BACK)
                    .label("×")
                    .tooltip("Close (Esc)")
                    .on_click(cx.listener(|this, _, window, cx| this.close_viewer(window, cx))),
            )
            .into_any_element()
    }

    fn render_viewer_toolbar(&self, view: ToolbarView<'_>, cx: &mut Context<Self>) -> AnyElement {
        let ToolbarView {
            id,
            catalog,
            has_prev,
            has_next,
            info_open,
            zoom_label,
            chrome,
        } = view;
        let mut row = div()
            .flex()
            .items_center()
            .gap_1()
            .px_3()
            .h(px(56.0))
            .rounded_xl()
            .bg(hsla(0.0, 0.0, 0.08, 0.72))
            .border_1()
            .border_color(hsla(0.0, 0.0, 1.0, 0.08));
        if has_prev {
            row = row.child(
                Button::new(ids::VIEWER_PREV)
                    .label("◀")
                    .tooltip("Previous (Left)")
                    .on_click(cx.listener(|this, _, _, cx| this.viewer_prev(cx))),
            );
        }
        if has_next {
            row = row.child(
                Button::new(ids::VIEWER_NEXT)
                    .label("▶")
                    .tooltip("Next (Right)")
                    .on_click(cx.listener(|this, _, _, cx| this.viewer_next(cx))),
            );
        }
        row = row
            .child(
                Button::new(ids::VIEWER_PLAY)
                    .label("▷")
                    .tooltip("Slideshow (Enter)")
                    .on_click(cx.listener(|this, _, window, cx| this.viewer_play(window, cx))),
            )
            .child(
                Button::new(ids::VIEWER_ROTATE_LEFT)
                    .label("⟲")
                    .tooltip("Rotate left")
                    .on_click(cx.listener(|this, _, _, cx| this.viewer_rotate(-1, cx))),
            )
            .child(
                Button::new(ids::VIEWER_ROTATE_RIGHT)
                    .label("⟳")
                    .tooltip("Rotate right")
                    .on_click(cx.listener(|this, _, _, cx| this.viewer_rotate(1, cx))),
            )
            .child(
                Button::new(ids::VIEWER_STAR)
                    .label("☆")
                    .disabled(!catalog)
                    .tooltip(if catalog {
                        "Star (S)"
                    } else {
                        "Add this folder to Lightbox to star photos"
                    })
                    .on_click(cx.listener(|this, _, _, cx| this.viewer_star(cx))),
            )
            .child(
                Button::new(ids::VIEWER_ZOOM_FIT)
                    .label("Fit")
                    .tooltip("Fit (0)")
                    .on_click(cx.listener(|this, _, _, cx| this.viewer_fit(cx))),
            )
            .child(
                Button::new(ids::VIEWER_ZOOM_ACTUAL)
                    .label("1:1")
                    .tooltip("Actual size")
                    .on_click(cx.listener(|this, _, _, cx| this.viewer_actual(cx))),
            )
            .child(
                div()
                    .id(ids::VIEWER_ZOOM_LABEL)
                    .test_support()
                    .aria_label(zoom_label.to_owned())
                    .px_2()
                    .child(zoom_label.to_owned()),
            )
            .child(
                Button::new(ids::VIEWER_INFO)
                    .label("Info")
                    .tooltip("Info panel (I)")
                    .toggled(info_open)
                    .on_click(cx.listener(|this, _, _, cx| this.viewer_toggle_info(cx))),
            )
            .child(
                Button::new(ids::VIEWER_EDIT)
                    .label("Edit")
                    .disabled(!catalog)
                    .tooltip(if catalog {
                        "Edit (E)"
                    } else {
                        "Add this folder to Lightbox to star photos"
                    })
                    .on_click(cx.listener(move |this, _, window, cx| {
                        if let Some(photo) = this.viewer.as_ref().and_then(|v| v.vm.current_photo())
                        {
                            this.open_editor(photo, window, cx);
                        }
                    })),
            )
            .child(
                Button::new(ids::VIEWER_DELETE)
                    .label("Delete")
                    .tooltip("Delete")
                    .on_click(cx.listener(|this, _, _, cx| this.viewer_delete(cx))),
            );
        if catalog {
            row = row.child(
                Button::new(ids::VIEWER_OPEN_LIBRARY)
                    .label("Library")
                    .tooltip("Show in library")
                    .on_click(cx.listener(move |this, _, window, cx| {
                        this.vm.click(id, Modifiers::NONE);
                        this.close_viewer(window, cx);
                    })),
            );
        } else {
            row = row.child(
                Button::new(ids::VIEWER_ADD_FOLDER)
                    .label("Add folder")
                    .tooltip("Add this folder to Lightbox"),
            );
        }
        div()
            .id(ids::VIEWER_TOOLBAR)
            .test_support()
            .absolute()
            .bottom(px(FILMSTRIP_HEIGHT + 12.0))
            .left_0()
            .right_0()
            .flex()
            .justify_center()
            .opacity(chrome)
            .child(row)
            .into_any_element()
    }

    fn render_filmstrip_overlay(&mut self, chrome: f32, cx: &mut Context<Self>) -> AnyElement {
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
            if id.get() == 0 {
                continue;
            }
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
                .bg(hsla(0.0, 0.0, 0.15, 1.0))
                .border_2()
                .border_color(if active {
                    hsla(0.0, 0.0, CHROME_TEXT, 1.0)
                } else {
                    hsla(0.0, 0.0, 0.0, 0.0)
                })
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
            .absolute()
            .bottom_0()
            .left_0()
            .right_0()
            .h(px(FILMSTRIP_HEIGHT))
            .opacity(chrome)
            .bg(hsla(0.0, 0.0, 0.0, 0.45))
            .child(row)
            .into_any_element()
    }

    fn render_viewer_info(&mut self, chrome: f32, cx: &mut Context<Self>) -> AnyElement {
        let _ = cx;
        let lines = self
            .viewer
            .as_ref()
            .and_then(|v| v.photo.as_ref())
            .map(info_lines)
            .unwrap_or_default();
        div()
            .id(ids::VIEWER_INFO_PANEL)
            .test_support()
            .absolute()
            .top_0()
            .right_0()
            .bottom_0()
            .w(px(lb_app::viewer::INFO_PANEL_WIDTH))
            .opacity(chrome)
            .p_3()
            .flex()
            .flex_col()
            .gap_1()
            .bg(hsla(0.0, 0.0, 0.06, 0.88))
            .text_color(hsla(0.0, 0.0, CHROME_TEXT, 1.0))
            .children(lines.into_iter().map(|l| div().child(l)))
            .into_any_element()
    }
}

fn extension_lower(path: &std::path::Path) -> String {
    path.extension()
        .and_then(|ext| ext.to_str())
        .unwrap_or("")
        .to_ascii_lowercase()
}

fn is_gif_or_webp(path: &std::path::Path) -> bool {
    matches!(extension_lower(path).as_str(), "gif" | "webp")
}

fn placed_image(path: Option<PathBuf>, layer: lb_app::viewer::LayerState) -> gpui_kit::Div {
    let mut el = div()
        .absolute()
        .left(px(layer.rect.x))
        .top(px(layer.rect.y))
        .w(px(layer.rect.w.max(1.0)))
        .h(px(layer.rect.h.max(1.0)))
        .opacity(layer.opacity)
        .overflow_hidden();
    if layer.corner_radius > 0.5 {
        el = el.rounded_md();
    }
    if let Some(path) = path {
        el = el.child(
            img(path)
                .object_fit(ObjectFit::Fill)
                .size_full()
                .with_fallback(|| div().size_full().into_any_element()),
        );
    }
    el
}

fn shown_for(v: &ViewerState, id: PhotoId) -> Option<&lb_app::PreviewFile> {
    if id.get() == 0 {
        return None;
    }
    let prefix = format!("{}:", id.get());
    v.stage.shown_if(|k| k.starts_with(&prefix))
}

fn entry_file_name(v: &ViewerState) -> String {
    v.photo
        .as_ref()
        .and_then(|p| p.path.file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| {
            v.vm.current_entry()
                .path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        })
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
