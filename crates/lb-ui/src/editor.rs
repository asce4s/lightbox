//! The photo editor: tabs, sliders, effect tiles and the crop, straighten and red-eye tools
//! (SPEC §9.5).

use crate::ids;
use crate::stage::{RenderJob, Stage, StageKind, frame_key};
use crate::window::{LightboxWindow, Overlay};
use gpui_kit::component::ActiveTheme as _;
use gpui_kit::component::Disableable as _;
use gpui_kit::component::button::Button;
use gpui_kit::component::slider::{Slider, SliderEvent, SliderState};
use gpui_kit::prelude::*;
use gpui_kit::{
    AnyElement, Context, Entity, FontWeight, IntoElement, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, ObjectFit, Pixels, Point, Subscription, TestSupportExt as _,
    Window, div, img, px,
};
use lb_app::PreviewFile;
use lb_app::vm::{
    AspectPreset, ConfirmVm, CropHandle, CropVm, DisplayRect, EFFECTS, EditorTab, EditorVm,
    EffectPanelVm, RedEyeVm, TuningSlider,
};
use lb_core::{EditOp, EditStack, PhotoId};
use std::collections::{HashMap, HashSet};

/// Height of the editor's top bar.
pub const EDITOR_BAR_HEIGHT: f32 = 48.0;
/// Width of the editor's left panel.
pub const EDITOR_PANEL_WIDTH: f32 = 280.0;
/// Padding between the stage edge and the picture.
const STAGE_PAD: f32 = 16.0;
/// Longest edge of the preview while editing.
const PREVIEW_EDGE: u32 = 1280;
/// Longest edge of an effect tile's mini preview.
const TILE_EDGE: u32 = 128;
const TILE_SIZE: f32 = 78.0;
const HANDLE: f32 = 12.0;

/// The tool whose overlay is on the picture.
pub enum Tool {
    /// No tool.
    None,
    /// Crop with aspect lock.
    Crop(CropVm),
    /// Straighten with a grid.
    Straighten,
    /// Red-eye rectangles.
    RedEye(RedEyeVm),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SliderKind {
    Tuning(TuningSlider),
    BasicFill,
    Straighten,
    Param(usize),
}

pub(crate) struct CropDrag {
    handle: CropHandle,
    last: Point<Pixels>,
}

pub(crate) struct EditorState {
    pub(crate) vm: EditorVm,
    pub(crate) stage: Stage,
    pub(crate) tool: Tool,
    drag: Option<CropDrag>,
    redeye_drag: bool,
    tuning: [Entity<SliderState>; 4],
    basic_fill: Entity<SliderState>,
    straighten: Entity<SliderState>,
    effect: Option<(EffectPanelVm, Vec<Entity<SliderState>>)>,
    pub(crate) tiles: HashMap<String, PreviewFile>,
    pub(crate) tiles_base: String,
    tiles_requested: HashSet<String>,
    sliders_dirty: bool,
    committed: EditStack,
    can_undo: bool,
    can_redo: bool,
    name: String,
    _subs: Vec<Subscription>,
    effect_subs: Vec<Subscription>,
}

const TUNING_SLIDERS: [(TuningSlider, &str, &str, f32, f32); 4] = [
    (
        TuningSlider::FillLight,
        ids::SLIDER_FILL_LIGHT,
        "Fill light",
        0.0,
        1.0,
    ),
    (
        TuningSlider::Highlights,
        ids::SLIDER_HIGHLIGHTS,
        "Highlights",
        0.0,
        1.0,
    ),
    (
        TuningSlider::Shadows,
        ids::SLIDER_SHADOWS,
        "Shadows",
        0.0,
        1.0,
    ),
    (
        TuningSlider::Temperature,
        ids::SLIDER_TEMPERATURE,
        "Color temperature",
        -1.0,
        1.0,
    ),
];

fn new_slider(
    min: f32,
    max: f32,
    step: f32,
    value: f32,
    cx: &mut Context<LightboxWindow>,
) -> Entity<SliderState> {
    cx.new(|_| {
        SliderState::new()
            .min(min)
            .max(max)
            .step(step)
            .default_value(value)
    })
}

fn slider_event(
    kind: SliderKind,
    event: &SliderEvent,
    this: &mut LightboxWindow,
    cx: &mut Context<LightboxWindow>,
) {
    match event {
        SliderEvent::Change(v) => this.slider_changed(kind, v.end(), cx),
        SliderEvent::Release(_) => this.slider_released(kind, cx),
    }
}

impl LightboxWindow {
    // ---- state ----

    /// The editor's view-model while the editor is open.
    pub fn editor(&self) -> Option<&EditorVm> {
        self.editor.as_ref().map(|e| &e.vm)
    }

    /// The active tool.
    pub fn editor_tool(&self) -> Option<&Tool> {
        self.editor.as_ref().map(|e| &e.tool)
    }

    /// The open effect parameter panel.
    pub fn effect_panel(&self) -> Option<&EffectPanelVm> {
        self.editor
            .as_ref()
            .and_then(|e| e.effect.as_ref())
            .map(|(p, _)| p)
    }

    /// The file the editor stage shows.
    pub fn editor_image(&self) -> Option<std::path::PathBuf> {
        let e = self.editor.as_ref()?;
        e.stage.shown().map(|f| f.path.clone())
    }

    /// The preview file shown for effect tile `key`.
    pub fn effect_tile_preview(&self, key: &str) -> Option<std::path::PathBuf> {
        self.editor.as_ref()?.tiles.get(key).map(|f| f.path.clone())
    }

    /// Opens the editor on `id`.
    pub fn open_editor(&mut self, id: PhotoId, window: &mut Window, cx: &mut Context<Self>) {
        let vm = EditorVm::new(&self.app, id);
        let mut subs = Vec::new();
        let tuning = TUNING_SLIDERS.map(|(_, _, _, min, max)| new_slider(min, max, 0.01, 0.0, cx));
        for (slider, (which, ..)) in tuning.iter().zip(TUNING_SLIDERS) {
            let kind = SliderKind::Tuning(which);
            subs.push(cx.subscribe(slider, move |this, _, ev, cx| {
                slider_event(kind, ev, this, cx);
            }));
        }
        let basic_fill = new_slider(0.0, 1.0, 0.01, 0.0, cx);
        subs.push(cx.subscribe(&basic_fill, |this, _, ev, cx| {
            slider_event(SliderKind::BasicFill, ev, this, cx);
        }));
        let straighten = new_slider(-45.0, 45.0, 0.1, 0.0, cx);
        subs.push(cx.subscribe(&straighten, |this, _, ev, cx| {
            slider_event(SliderKind::Straighten, ev, this, cx);
        }));
        let name = self
            .app
            .photos()
            .get(id)
            .ok()
            .flatten()
            .and_then(|p| p.path.file_name().map(|n| n.to_string_lossy().into_owned()))
            .unwrap_or_default();
        self.editor = Some(EditorState {
            vm,
            stage: Stage::new(),
            tool: Tool::None,
            drag: None,
            redeye_drag: false,
            tuning,
            basic_fill,
            straighten,
            effect: None,
            tiles: HashMap::new(),
            tiles_base: String::new(),
            tiles_requested: HashSet::new(),
            sliders_dirty: true,
            committed: EditStack::new(),
            can_undo: false,
            can_redo: false,
            name,
            _subs: subs,
            effect_subs: Vec::new(),
        });
        window.focus(&self.focus, cx);
        self.editor_changed(cx);
    }

    /// Leaves the editor: back to the viewer when it was opened from there, else the library.
    pub fn close_editor(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(mut e) = self.editor.take() else {
            return;
        };
        e.stage.cancel();
        e.vm.cancel();
        let id = e.vm.photo();
        if self.viewer.is_some() {
            self.viewer_reload(cx);
        } else {
            self.vm.click(id, lb_app::vm::Modifiers::NONE);
            self.reveal_focused();
        }
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Refreshes cached state after any change to the stack or draft and re-renders the preview.
    pub(crate) fn editor_changed(&mut self, cx: &mut Context<Self>) {
        let Some(e) = &mut self.editor else { return };
        e.committed = e.vm.committed().unwrap_or_default();
        e.can_undo = e.vm.can_undo().unwrap_or(false);
        e.can_redo = e.vm.can_redo().unwrap_or(false);
        e.sliders_dirty = true;
        self.sync_editor(cx);
        cx.notify();
    }

    /// Starts the preview and tile renders the editor needs.
    pub(crate) fn sync_editor(&mut self, cx: &mut Context<Self>) {
        let Some(e) = &mut self.editor else { return };
        let id = e.vm.photo();
        let Ok(stack) = e.vm.preview_stack() else {
            return;
        };
        let key = frame_key(id, &stack, PREVIEW_EDGE);
        let mut jobs = Vec::new();
        if let Some(cancel) = e.stage.want(&key) {
            jobs.push(RenderJob {
                kind: StageKind::Editor,
                key,
                id,
                stack,
                max_edge: PREVIEW_EDGE,
                cancel,
            });
        }
        if e.vm.tab() == EditorTab::Effects {
            let base = e.committed.hash();
            if base != e.tiles_base {
                e.tiles.clear();
                e.tiles_requested.clear();
                e.tiles_base = base.clone();
            }
            for spec in EFFECTS {
                if !e.tiles_requested.insert(spec.key.to_owned()) {
                    continue;
                }
                let mut tile_stack = e.committed.clone();
                tile_stack.ops.push(spec.build(&spec.defaults()));
                let key = frame_key(id, &tile_stack, TILE_EDGE);
                jobs.push(RenderJob {
                    kind: StageKind::Tile {
                        effect: spec.key.to_owned(),
                        base: base.clone(),
                    },
                    key,
                    id,
                    stack: tile_stack,
                    max_edge: TILE_EDGE,
                    cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
                });
            }
        }
        for job in jobs {
            self.spawn_render(job, cx);
        }
    }

    fn with_editor<R>(&mut self, f: impl FnOnce(&mut EditorState) -> R) -> Option<R> {
        self.editor.as_mut().map(f)
    }

    // ---- intents ----

    /// Selects an editor tab.
    pub fn editor_set_tab(&mut self, tab: EditorTab, cx: &mut Context<Self>) {
        self.with_editor(|e| {
            e.tool = Tool::None;
            e.effect = None;
            e.effect_subs.clear();
            e.vm.set_tab(tab);
        });
        self.editor_changed(cx);
    }

    fn editor_result<T>(&mut self, result: lb_app::Result<T>, cx: &mut Context<Self>) {
        self.report(result, cx);
        self.editor_changed(cx);
    }

    /// I'm Feeling Lucky.
    pub fn editor_lucky(&mut self, cx: &mut Context<Self>) {
        self.editor_apply(EditOp::FeelingLucky, cx);
    }

    /// Stores a one-shot op as its own undo step.
    pub fn editor_apply(&mut self, op: EditOp, cx: &mut Context<Self>) {
        if let Some(r) = self.with_editor(|e| {
            e.tool = Tool::None;
            e.vm.apply(op)
        }) {
            self.editor_result(r, cx);
        }
    }

    /// Undo.
    pub fn editor_undo(&mut self, cx: &mut Context<Self>) {
        if let Some(r) = self.with_editor(|e| {
            e.tool = Tool::None;
            e.vm.undo()
        }) {
            self.editor_result(r, cx);
        }
    }

    /// Redo.
    pub fn editor_redo(&mut self, cx: &mut Context<Self>) {
        if let Some(r) = self.with_editor(|e| {
            e.tool = Tool::None;
            e.vm.redo()
        }) {
            self.editor_result(r, cx);
        }
    }

    /// Asks before removing every edit.
    pub fn editor_ask_revert(&mut self, cx: &mut Context<Self>) {
        let Some(id) = self.editor.as_ref().map(|e| e.vm.photo()) else {
            return;
        };
        self.overlay = Overlay::Confirm(ConfirmVm::revert(id));
        cx.notify();
    }

    /// Shows the original while `on` (hold `\`).
    pub fn editor_set_before(&mut self, on: bool, cx: &mut Context<Self>) {
        self.with_editor(|e| e.vm.set_show_before(on));
        self.editor_changed(cx);
    }

    /// Opens the export dialog for the photo being edited.
    pub fn editor_export(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(id) = self.editor.as_ref().map(|e| e.vm.photo()) {
            self.open_export_for(vec![id], window, cx);
        }
    }

    fn slider_changed(&mut self, kind: SliderKind, value: f32, cx: &mut Context<Self>) {
        let result = self.with_editor(|e| match kind {
            SliderKind::Tuning(which) => e.vm.set_tuning(which, value),
            SliderKind::BasicFill => e.vm.set_draft(EditOp::FillLight { amount: value }),
            SliderKind::Straighten => e.vm.set_draft(EditOp::Straighten { degrees: value }),
            SliderKind::Param(i) => {
                let Some((panel, _)) = &mut e.effect else {
                    return Ok(());
                };
                panel.set(i, value);
                e.vm.set_draft(panel.op())
            }
        });
        if let Some(r) = result {
            if let Err(err) = r {
                self.toast = Some(err.user_message());
            }
            self.sync_editor(cx);
            cx.notify();
        }
    }

    fn slider_released(&mut self, kind: SliderKind, cx: &mut Context<Self>) {
        if matches!(kind, SliderKind::Tuning(_) | SliderKind::BasicFill)
            && let Some(r) = self.with_editor(|e| e.vm.commit())
        {
            self.editor_result(r, cx);
        }
    }

    /// Opens the crop tool.
    pub fn editor_start_crop(&mut self, cx: &mut Context<Self>) {
        let dims = self.editor_image_dims();
        self.with_editor(|e| {
            e.vm.cancel();
            e.tool = Tool::Crop(CropVm::new(dims.0, dims.1));
        });
        self.editor_changed(cx);
    }

    /// Opens the straighten tool.
    pub fn editor_start_straighten(&mut self, cx: &mut Context<Self>) {
        self.with_editor(|e| {
            e.vm.cancel();
            e.tool = Tool::Straighten;
        });
        self.editor_changed(cx);
    }

    /// Opens the red-eye tool.
    pub fn editor_start_redeye(&mut self, cx: &mut Context<Self>) {
        self.with_editor(|e| {
            e.vm.cancel();
            e.tool = Tool::RedEye(RedEyeVm::new());
        });
        self.editor_changed(cx);
    }

    /// Chooses a crop aspect preset.
    pub fn editor_set_aspect(&mut self, preset: AspectPreset, cx: &mut Context<Self>) {
        self.with_editor(|e| {
            if let Tool::Crop(c) = &mut e.tool {
                c.set_aspect(preset);
            }
        });
        cx.notify();
    }

    /// Apply of the active tool.
    pub fn editor_tool_apply(&mut self, cx: &mut Context<Self>) {
        let result = self.with_editor(|e| {
            let tool = std::mem::replace(&mut e.tool, Tool::None);
            match tool {
                Tool::None => Ok(()),
                Tool::Crop(c) => c.op().map_or(Ok(()), |op| e.vm.apply(op)),
                Tool::Straighten => e.vm.commit().map(|_| ()),
                Tool::RedEye(r) => r.op().map_or(Ok(()), |op| e.vm.apply(op)),
            }
        });
        if let Some(r) = result {
            self.editor_result(r, cx);
        }
    }

    /// Cancel of the active tool or effect panel.
    pub fn editor_tool_cancel(&mut self, cx: &mut Context<Self>) {
        self.with_editor(|e| {
            e.vm.cancel();
            e.tool = Tool::None;
            e.effect = None;
            e.effect_subs.clear();
        });
        self.editor_changed(cx);
    }

    /// Clears the red-eye rectangles.
    pub fn editor_tool_clear(&mut self, cx: &mut Context<Self>) {
        self.with_editor(|e| {
            if let Tool::RedEye(r) = &mut e.tool {
                r.clear();
            }
        });
        cx.notify();
    }

    /// A click on effect tile `key`: effects without parameters apply at once, others open a panel.
    pub fn editor_effect_click(&mut self, key: &str, cx: &mut Context<Self>) {
        let Some(mut panel) = EffectPanelVm::open(key) else {
            return;
        };
        if !panel.spec().is_parametrized() {
            self.editor_apply(panel.op(), cx);
            return;
        }
        let specs = panel.spec().params;
        let sliders: Vec<_> = specs
            .iter()
            .map(|p| new_slider(p.min, p.max, (p.max - p.min) / 100.0, p.default, cx))
            .collect();
        let subs = sliders
            .iter()
            .enumerate()
            .map(|(i, s)| {
                cx.subscribe(s, move |this, _, ev, cx| {
                    slider_event(SliderKind::Param(i), ev, this, cx);
                })
            })
            .collect();
        for (i, p) in specs.iter().enumerate() {
            panel.set(i, p.default);
        }
        let op = panel.op();
        self.with_editor(|e| {
            e.tool = Tool::None;
            e.effect = Some((panel, sliders));
            e.effect_subs = subs;
            e.vm.set_draft(op)
        });
        self.sync_editor(cx);
        cx.notify();
    }

    /// Apply of the effect panel.
    pub fn editor_effect_apply(&mut self, cx: &mut Context<Self>) {
        let result = self.with_editor(|e| {
            e.effect = None;
            e.effect_subs.clear();
            e.vm.commit()
        });
        if let Some(r) = result {
            self.editor_result(r, cx);
        }
    }

    // ---- geometry ----

    fn editor_image_dims(&self) -> (u32, u32) {
        let Some(e) = &self.editor else { return (1, 1) };
        if let Some(f) = e.stage.shown() {
            return (f.width, f.height);
        }
        self.app
            .photos()
            .get(e.vm.photo())
            .ok()
            .flatten()
            .and_then(|p| Some((p.width?, p.height?)))
            .unwrap_or((1, 1))
    }

    fn stage_size(&self) -> (f32, f32) {
        (
            (self.viewport.0 - EDITOR_PANEL_WIDTH).max(1.0),
            (self.viewport.1 - EDITOR_BAR_HEIGHT).max(1.0),
        )
    }

    /// Where the picture sits inside the stage.
    fn editor_display(&self) -> DisplayRect {
        let (iw, ih) = self.editor_image_dims();
        let (sw, sh) = self.stage_size();
        let inner = DisplayRect::fit(
            iw as f32,
            ih as f32,
            (sw - 2.0 * STAGE_PAD).max(1.0),
            (sh - 2.0 * STAGE_PAD).max(1.0),
        );
        DisplayRect {
            x: inner.x + STAGE_PAD,
            y: inner.y + STAGE_PAD,
            ..inner
        }
    }

    /// The picture's rectangle in window coordinates.
    fn editor_display_in_window(&self) -> DisplayRect {
        let d = self.editor_display();
        DisplayRect {
            x: d.x + EDITOR_PANEL_WIDTH,
            y: d.y + EDITOR_BAR_HEIGHT,
            ..d
        }
    }

    // ---- keyboard ----

    pub(crate) fn editor_key(
        &mut self,
        key: &str,
        shift: bool,
        cmd: bool,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match key {
            "escape" => {
                let busy = self.editor.as_ref().is_some_and(|e| {
                    !matches!(e.tool, Tool::None) || e.effect.is_some() || e.vm.draft().is_some()
                });
                if busy {
                    self.editor_tool_cancel(cx);
                } else {
                    self.close_editor(window, cx);
                }
            }
            "z" if cmd && shift => self.editor_redo(cx),
            "z" if cmd => self.editor_undo(cx),
            "y" if cmd => self.editor_redo(cx),
            "\\" => self.editor_set_before(true, cx),
            "enter" => {
                let tool_open = self
                    .editor
                    .as_ref()
                    .is_some_and(|e| !matches!(e.tool, Tool::None));
                if tool_open {
                    self.editor_tool_apply(cx);
                }
            }
            "left" | "right" | "up" | "down" => {
                let step = if shift { 10 } else { 1 };
                let (dx, dy) = match key {
                    "left" => (-step, 0),
                    "right" => (step, 0),
                    "up" => (0, -step),
                    _ => (0, step),
                };
                self.with_editor(|e| {
                    if let Tool::Crop(c) = &mut e.tool {
                        c.nudge(dx, dy);
                    }
                });
                cx.notify();
            }
            _ => {}
        }
    }

    pub(crate) fn editor_key_up(&mut self, key: &str, cx: &mut Context<Self>) {
        if key == "\\" && self.editor.as_ref().is_some_and(|e| e.vm.showing_before()) {
            self.editor_set_before(false, cx);
        }
    }

    // ---- rendering ----

    pub(crate) fn render_editor(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        self.sync_editor_sliders(window, cx);
        let panel = self.render_editor_panel(cx);
        let stage = self.render_editor_stage(cx);
        div()
            .id(ids::EDITOR)
            .test_support()
            .flex_1()
            .min_h_0()
            .flex()
            .flex_col()
            .child(self.render_editor_bar(cx))
            .child(div().flex().flex_1().min_h_0().child(panel).child(stage))
            .into_any_element()
    }

    fn sync_editor_sliders(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(e) = &mut self.editor else { return };
        if !e.sliders_dirty {
            return;
        }
        e.sliders_dirty = false;
        for (slider, (which, ..)) in e.tuning.iter().zip(TUNING_SLIDERS) {
            let v = e.vm.tuning_value(which).unwrap_or(0.0);
            slider.update(cx, |s, cx| s.set_value(v, window, cx));
        }
        let top = e.vm.draft().or(e.committed.ops.last());
        let fill = match top {
            Some(EditOp::FillLight { amount }) => *amount,
            _ => 0.0,
        };
        let degrees = match top {
            Some(EditOp::Straighten { degrees }) => *degrees,
            _ => 0.0,
        };
        e.basic_fill
            .update(cx, |s, cx| s.set_value(fill, window, cx));
        e.straighten
            .update(cx, |s, cx| s.set_value(degrees, window, cx));
    }

    fn render_editor_bar(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(e) = &self.editor else {
            return div().into_any_element();
        };
        let (can_undo, can_redo) = (e.can_undo, e.can_redo);
        let before = e.vm.showing_before();
        let name = e.name.clone();
        div()
            .id("editor-bar")
            .flex()
            .items_center()
            .gap_2()
            .px_3()
            .h(px(EDITOR_BAR_HEIGHT))
            .border_b_1()
            .border_color(theme.border)
            .child(
                Button::new(ids::EDITOR_BACK)
                    .label("← Back")
                    .tooltip("Back (Esc)")
                    .on_click(cx.listener(|this, _, window, cx| this.close_editor(window, cx))),
            )
            .child(div().font_weight(FontWeight::SEMIBOLD).child(name))
            .child(div().flex_1())
            .child(
                Button::new(ids::EDITOR_UNDO)
                    .label("Undo")
                    .tooltip("Undo (Ctrl+Z)")
                    .disabled(!can_undo)
                    .on_click(cx.listener(|this, _, _, cx| this.editor_undo(cx))),
            )
            .child(
                Button::new(ids::EDITOR_REDO)
                    .label("Redo")
                    .tooltip("Redo (Ctrl+Shift+Z)")
                    .disabled(!can_redo)
                    .on_click(cx.listener(|this, _, _, cx| this.editor_redo(cx))),
            )
            .child(
                Button::new(ids::EDITOR_REVERT)
                    .label("Revert")
                    .tooltip("Remove all edits")
                    .disabled(!can_undo)
                    .on_click(cx.listener(|this, _, _, cx| this.editor_ask_revert(cx))),
            )
            .child(
                Button::new(ids::EDITOR_BEFORE_AFTER)
                    .label("Before/After")
                    .tooltip("Hold \\ to show the original")
                    .toggled(before)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.editor_set_before(!before, cx);
                    })),
            )
            .child(
                Button::new(ids::EDITOR_EXPORT)
                    .label("Export")
                    .on_click(cx.listener(|this, _, window, cx| this.editor_export(window, cx))),
            )
            .into_any_element()
    }

    fn render_editor_panel(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(e) = &self.editor else {
            return div().into_any_element();
        };
        let tab = e.vm.tab();
        let tabs = div()
            .flex()
            .gap_1()
            .child(
                Button::new(ids::EDITOR_TAB_BASIC)
                    .label("Basic Fixes")
                    .compact()
                    .toggled(tab == EditorTab::Basic)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.editor_set_tab(EditorTab::Basic, cx);
                    })),
            )
            .child(
                Button::new(ids::EDITOR_TAB_TUNING)
                    .label("Tuning")
                    .compact()
                    .toggled(tab == EditorTab::Tuning)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.editor_set_tab(EditorTab::Tuning, cx);
                    })),
            )
            .child(
                Button::new(ids::EDITOR_TAB_EFFECTS)
                    .label("Effects")
                    .compact()
                    .toggled(tab == EditorTab::Effects)
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.editor_set_tab(EditorTab::Effects, cx);
                    })),
            );
        let body = match tab {
            EditorTab::Basic => self.render_basic_tab(cx),
            EditorTab::Tuning => self.render_tuning_tab(cx),
            EditorTab::Effects => self.render_effects_tab(cx),
        };
        div()
            .id("editor-panel")
            .w(px(EDITOR_PANEL_WIDTH))
            .flex_none()
            .h_full()
            .p_2()
            .flex()
            .flex_col()
            .gap_3()
            .overflow_y_scroll()
            .border_r_1()
            .border_color(theme.border)
            .child(tabs)
            .child(body)
            .into_any_element()
    }

    fn slider_row(
        &self,
        id: &'static str,
        label: &str,
        state: &Entity<SliderState>,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let value = state.read(cx).value().end();
        div()
            .flex()
            .flex_col()
            .gap_1()
            .child(
                div()
                    .flex()
                    .justify_between()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child(label.to_owned())
                    .child(format!("{value:.2}")),
            )
            .child(
                div()
                    .id(id)
                    .test_support()
                    .w_full()
                    .child(Slider::new(state)),
            )
            .into_any_element()
    }

    fn render_basic_tab(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(e) = &self.editor else {
            return div().into_any_element();
        };
        let (crop_on, straighten_on, redeye_on) = (
            matches!(e.tool, Tool::Crop(_)),
            matches!(e.tool, Tool::Straighten),
            matches!(e.tool, Tool::RedEye(_)),
        );
        let basic_fill = e.basic_fill.clone();
        let tool_panel = self.render_tool_panel(cx);
        let fill_row = self.slider_row(ids::SLIDER_BASIC_FILL_LIGHT, "Fill light", &basic_fill, cx);
        let button = |id: &'static str, label: &'static str, on: bool| {
            Button::new(id).label(label).toggled(on)
        };
        div()
            .flex()
            .flex_col()
            .gap_2()
            .child(
                button(ids::EDITOR_CROP, "Crop", crop_on)
                    .on_click(cx.listener(|this, _, _, cx| this.editor_start_crop(cx))),
            )
            .child(
                button(ids::EDITOR_STRAIGHTEN, "Straighten", straighten_on)
                    .on_click(cx.listener(|this, _, _, cx| this.editor_start_straighten(cx))),
            )
            .child(
                button(ids::EDITOR_REDEYE, "Red-eye", redeye_on)
                    .on_click(cx.listener(|this, _, _, cx| this.editor_start_redeye(cx))),
            )
            .child(
                button(ids::EDITOR_LUCKY, "I'm Feeling Lucky", false)
                    .on_click(cx.listener(|this, _, _, cx| this.editor_lucky(cx))),
            )
            .child(
                button(ids::EDITOR_AUTO_CONTRAST, "Auto Contrast", false).on_click(
                    cx.listener(|this, _, _, cx| this.editor_apply(EditOp::AutoContrast, cx)),
                ),
            )
            .child(
                button(ids::EDITOR_AUTO_COLOR, "Auto Color", false).on_click(
                    cx.listener(|this, _, _, cx| this.editor_apply(EditOp::AutoColor, cx)),
                ),
            )
            .child(fill_row)
            .children(tool_panel)
            .into_any_element()
    }

    fn render_tool_panel(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let theme = cx.theme().clone();
        let e = self.editor.as_ref()?;
        let mut panel = div()
            .id(ids::TOOL_PANEL)
            .test_support()
            .flex()
            .flex_col()
            .gap_2()
            .p_2()
            .rounded_md()
            .border_1()
            .border_color(theme.border);
        match &e.tool {
            Tool::None => return None,
            Tool::Crop(c) => {
                let selected = c.aspect();
                let (w, h) = c.pixel_size();
                let mut presets = div().flex().flex_wrap().gap_1();
                for preset in AspectPreset::ALL {
                    presets = presets.child(
                        Button::new(ids::crop_aspect(preset.key()))
                            .label(preset.label())
                            .compact()
                            .toggled(preset == selected)
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.editor_set_aspect(preset, cx);
                            })),
                    );
                }
                panel = panel
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Crop"))
                    .child(presets)
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(format!("{w} x {h}")),
                    );
            }
            Tool::Straighten => {
                let straighten = e.straighten.clone();
                panel = panel
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Straighten"))
                    .child(self.slider_row(ids::SLIDER_STRAIGHTEN, "Degrees", &straighten, cx));
            }
            Tool::RedEye(r) => {
                let n = r.regions().len();
                panel = panel
                    .child(div().font_weight(FontWeight::SEMIBOLD).child("Red-eye"))
                    .child(
                        div()
                            .text_xs()
                            .text_color(theme.muted_foreground)
                            .child(format!("Drag a rectangle over each eye ({n} selected)")),
                    )
                    .child(
                        Button::new(ids::TOOL_CLEAR)
                            .label("Clear")
                            .compact()
                            .on_click(cx.listener(|this, _, _, cx| this.editor_tool_clear(cx))),
                    );
            }
        }
        Some(
            panel
                .child(
                    div()
                        .flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new(ids::TOOL_CANCEL)
                                .label("Cancel")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.editor_tool_cancel(cx);
                                })),
                        )
                        .child(
                            Button::new(ids::TOOL_APPLY)
                                .label("Apply")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.editor_tool_apply(cx);
                                })),
                        ),
                )
                .into_any_element(),
        )
    }

    fn render_tuning_tab(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let Some(e) = &self.editor else {
            return div().into_any_element();
        };
        let sliders = e.tuning.clone();
        let mut col = div().flex().flex_col().gap_3();
        for (state, (_, id, label, ..)) in sliders.iter().zip(TUNING_SLIDERS) {
            col = col.child(self.slider_row(id, label, state, cx));
        }
        col.into_any_element()
    }

    fn render_effects_tab(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let Some(e) = &self.editor else {
            return div().into_any_element();
        };
        let mut grid = div().flex().flex_wrap().gap_2();
        for spec in EFFECTS {
            let key = spec.key;
            let preview = e.tiles.get(key).map(|f| f.path.clone());
            let mut thumb = div()
                .w(px(TILE_SIZE))
                .h(px(TILE_SIZE - 18.0))
                .rounded_md()
                .overflow_hidden()
                .bg(theme.muted);
            if let Some(path) = preview {
                thumb = thumb.child(
                    img(path)
                        .object_fit(ObjectFit::Cover)
                        .size_full()
                        .with_fallback(|| div().size_full().into_any_element()),
                );
            }
            grid = grid.child(
                div()
                    .id(ids::effect_tile(key))
                    .test_support()
                    .w(px(TILE_SIZE))
                    .flex()
                    .flex_col()
                    .items_center()
                    .gap_1()
                    .cursor_pointer()
                    .text_xs()
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, _: &MouseDownEvent, _, cx| {
                            this.editor_effect_click(key, cx);
                        }),
                    )
                    .child(thumb)
                    .child(spec.label),
            );
        }
        let sliders = e
            .effect
            .as_ref()
            .map(|(panel, states)| (panel.spec().label, panel.spec().params, states.clone()));
        let panel =
            sliders.map(|(label, params, states)| {
                let mut p = div()
                    .id(ids::EFFECT_PANEL)
                    .test_support()
                    .flex()
                    .flex_col()
                    .gap_2()
                    .p_2()
                    .rounded_md()
                    .border_1()
                    .border_color(theme.border)
                    .child(div().font_weight(FontWeight::SEMIBOLD).child(label));
                for (spec, state) in params.iter().zip(&states) {
                    let id: gpui_kit::ElementId = ids::effect_param(spec.key);
                    let value = state.read(cx).value().end();
                    p = p.child(
                        div()
                            .flex()
                            .flex_col()
                            .gap_1()
                            .child(
                                div()
                                    .flex()
                                    .justify_between()
                                    .text_xs()
                                    .text_color(theme.muted_foreground)
                                    .child(spec.label)
                                    .child(format!("{value:.2}")),
                            )
                            .child(
                                div()
                                    .id(id)
                                    .test_support()
                                    .w_full()
                                    .child(Slider::new(state)),
                            ),
                    );
                }
                p.child(
                    div()
                        .flex()
                        .gap_2()
                        .justify_end()
                        .child(
                            Button::new(ids::EFFECT_CANCEL).label("Cancel").on_click(
                                cx.listener(|this, _, _, cx| this.editor_tool_cancel(cx)),
                            ),
                        )
                        .child(Button::new(ids::EFFECT_APPLY).label("Apply").on_click(
                            cx.listener(|this, _, _, cx| {
                                this.editor_effect_apply(cx);
                            }),
                        )),
                )
            });
        div()
            .flex()
            .flex_col()
            .gap_3()
            .child(grid)
            .children(panel)
            .into_any_element()
    }

    fn render_editor_stage(&mut self, cx: &mut Context<Self>) -> AnyElement {
        let theme = cx.theme().clone();
        let display = self.editor_display();
        let window_display = self.editor_display_in_window();
        let Some(e) = &self.editor else {
            return div().into_any_element();
        };
        let busy = e.stage.busy();
        let path = e.stage.shown().map(|f| f.path.clone());
        let mut stage = div()
            .id(ids::EDITOR_STAGE)
            .test_support()
            .relative()
            .flex_1()
            .h_full()
            .overflow_hidden()
            .bg(theme.muted)
            .on_mouse_move(cx.listener(move |this, ev: &MouseMoveEvent, _, cx| {
                this.editor_mouse_move(ev, window_display, cx);
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _: &MouseUpEvent, _, cx| this.editor_mouse_up(cx)),
            );
        let picture = div()
            .id(ids::EDITOR_IMAGE)
            .test_support()
            .absolute()
            .left(px(display.x))
            .top(px(display.y))
            .w(px(display.w))
            .h(px(display.h));
        let picture = match path {
            Some(path) => picture.child(
                img(path)
                    .object_fit(ObjectFit::Fill)
                    .size_full()
                    .with_fallback(|| div().size_full().into_any_element()),
            ),
            None => picture,
        };
        stage = stage.child(picture);
        match &e.tool {
            Tool::Crop(c) => {
                let rect = c.rect();
                stage = stage.child(self.render_crop_overlay(display, rect, cx));
            }
            Tool::Straighten => stage = stage.child(grid_overlay(display, cx)),
            Tool::RedEye(r) => {
                let mut regions: Vec<_> = r.regions().to_vec();
                regions.extend(r.in_progress());
                stage =
                    stage.child(self.render_redeye_overlay(display, window_display, &regions, cx));
            }
            Tool::None => {}
        }
        if busy {
            stage = stage.child(
                div()
                    .id(ids::EDITOR_BUSY)
                    .test_support()
                    .absolute()
                    .top_2()
                    .right_2()
                    .text_xs()
                    .text_color(theme.muted_foreground)
                    .child("Rendering…"),
            );
        }
        stage.into_any_element()
    }

    fn render_crop_overlay(
        &mut self,
        display: DisplayRect,
        rect: lb_core::NormRect,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let (rx, ry, rw, rh) = (
            rect.x * display.w,
            rect.y * display.h,
            rect.w * display.w,
            rect.h * display.h,
        );
        let dim = theme.background.opacity(0.55);
        let shade = |x: f32, y: f32, w: f32, h: f32| {
            div()
                .absolute()
                .left(px(x))
                .top(px(y))
                .w(px(w.max(0.0)))
                .h(px(h.max(0.0)))
                .bg(dim)
        };
        let mut inner = div()
            .id(ids::CROP_RECT)
            .test_support()
            .absolute()
            .left(px(rx))
            .top(px(ry))
            .w(px(rw))
            .h(px(rh))
            .border_2()
            .border_color(theme.primary)
            .cursor_move()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, _, _| {
                    this.begin_crop_drag(CropHandle::Move, ev.position);
                }),
            );
        for i in 1..3 {
            let f = i as f32 / 3.0;
            inner = inner
                .child(
                    div()
                        .absolute()
                        .left(px(rw * f))
                        .top_0()
                        .bottom_0()
                        .w(px(1.0))
                        .bg(theme.primary.opacity(0.6)),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(rh * f))
                        .left_0()
                        .right_0()
                        .h(px(1.0))
                        .bg(theme.primary.opacity(0.6)),
                );
        }
        let mut overlay = div()
            .id(ids::CROP_OVERLAY)
            .test_support()
            .absolute()
            .left(px(display.x))
            .top(px(display.y))
            .w(px(display.w))
            .h(px(display.h))
            .child(shade(0.0, 0.0, display.w, ry))
            .child(shade(0.0, ry + rh, display.w, display.h - ry - rh))
            .child(shade(0.0, ry, rx, rh))
            .child(shade(rx + rw, ry, display.w - rx - rw, rh))
            .child(inner);
        for handle in CropHandle::RESIZE {
            let (ax, ay) = handle.anchor();
            overlay = overlay.child(
                div()
                    .id(ids::crop_handle(handle.key()))
                    .test_support()
                    .absolute()
                    .left(px(rx + rw * ax - HANDLE / 2.0))
                    .top(px(ry + rh * ay - HANDLE / 2.0))
                    .w(px(HANDLE))
                    .h(px(HANDLE))
                    .rounded_sm()
                    .bg(theme.primary)
                    .border_1()
                    .border_color(theme.background)
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                            this.begin_crop_drag(handle, ev.position);
                            cx.stop_propagation();
                        }),
                    ),
            );
        }
        overlay.into_any_element()
    }

    fn render_redeye_overlay(
        &mut self,
        display: DisplayRect,
        window_display: DisplayRect,
        regions: &[lb_core::NormRect],
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let theme = cx.theme().clone();
        let mut overlay = div()
            .id(ids::REDEYE_OVERLAY)
            .test_support()
            .absolute()
            .left(px(display.x))
            .top(px(display.y))
            .w(px(display.w))
            .h(px(display.h))
            .cursor_crosshair()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    this.begin_redeye_drag(window_display, ev.position, cx);
                }),
            );
        for (i, r) in regions.iter().enumerate() {
            overlay = overlay.child(
                div()
                    .id(ids::redeye_region(i))
                    .test_support()
                    .absolute()
                    .left(px(r.x * display.w))
                    .top(px(r.y * display.h))
                    .w(px(r.w * display.w))
                    .h(px(r.h * display.h))
                    .rounded_full()
                    .border_2()
                    .border_color(theme.danger),
            );
        }
        overlay.into_any_element()
    }

    fn begin_crop_drag(&mut self, handle: CropHandle, at: Point<Pixels>) {
        self.with_editor(|e| e.drag = Some(CropDrag { handle, last: at }));
    }

    fn begin_redeye_drag(
        &mut self,
        display: DisplayRect,
        at: Point<Pixels>,
        cx: &mut Context<Self>,
    ) {
        self.with_editor(|e| {
            if let Tool::RedEye(r) = &mut e.tool {
                r.begin(display, f32::from(at.x), f32::from(at.y));
                e.redeye_drag = true;
            }
        });
        cx.notify();
    }

    fn editor_mouse_move(
        &mut self,
        ev: &MouseMoveEvent,
        display: DisplayRect,
        cx: &mut Context<Self>,
    ) {
        let pressed = ev.pressed_button == Some(MouseButton::Left);
        let Some(e) = &mut self.editor else { return };
        if !pressed {
            e.drag = None;
            e.redeye_drag = false;
            return;
        }
        if let (Some(drag), Tool::Crop(c)) = (&mut e.drag, &mut e.tool) {
            let dx = f32::from(ev.position.x - drag.last.x) / display.w.max(1.0);
            let dy = f32::from(ev.position.y - drag.last.y) / display.h.max(1.0);
            c.drag(drag.handle, dx, dy);
            drag.last = ev.position;
            cx.notify();
        } else if e.redeye_drag
            && let Tool::RedEye(r) = &mut e.tool
        {
            r.update(display, f32::from(ev.position.x), f32::from(ev.position.y));
            cx.notify();
        }
    }

    fn editor_mouse_up(&mut self, cx: &mut Context<Self>) {
        let Some(e) = &mut self.editor else { return };
        e.drag = None;
        if e.redeye_drag {
            e.redeye_drag = false;
            if let Tool::RedEye(r) = &mut e.tool {
                r.end();
            }
        }
        cx.notify();
    }
}

fn grid_overlay(display: DisplayRect, cx: &mut Context<LightboxWindow>) -> AnyElement {
    let theme = cx.theme().clone();
    let mut overlay = div()
        .absolute()
        .left(px(display.x))
        .top(px(display.y))
        .w(px(display.w))
        .h(px(display.h));
    for i in 1..8 {
        let f = i as f32 / 8.0;
        overlay = overlay
            .child(
                div()
                    .absolute()
                    .left(px(display.w * f))
                    .top_0()
                    .bottom_0()
                    .w(px(1.0))
                    .bg(theme.foreground.opacity(0.25)),
            )
            .child(
                div()
                    .absolute()
                    .top(px(display.h * f))
                    .left_0()
                    .right_0()
                    .h(px(1.0))
                    .bg(theme.foreground.opacity(0.25)),
            );
    }
    overlay.into_any_element()
}
