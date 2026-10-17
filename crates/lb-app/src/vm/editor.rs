//! State of the photo editor: tabs, slider drafts and undo (SPEC §9.5).

use crate::error::Result;
use crate::lightbox::Lightbox;
use image::RgbaImage;
use lb_core::{EditOp, EditStack, PhotoId};
use std::sync::atomic::AtomicBool;

/// Editor tabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EditorTab {
    /// Crop, straighten, red-eye, auto fixes.
    Basic,
    /// Fill light, highlights, shadows, temperature.
    Tuning,
    /// Effect tiles.
    Effects,
}

/// The sliders merged into one `Tuning` op.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuningSlider {
    /// Brighten shadows, `0..=1`.
    FillLight,
    /// Recover highlights, `0..=1`.
    Highlights,
    /// Deepen shadows, `0..=1`.
    Shadows,
    /// Cool to warm, `-1..=1`.
    Temperature,
}

/// The editor's view-model. A slider drag only changes the draft; [`EditorVm::commit`] writes one
/// op to the photo's edit stack.
pub struct EditorVm {
    lb: Lightbox,
    id: PhotoId,
    tab: EditorTab,
    draft: Option<EditOp>,
    show_before: bool,
}

fn tuning_of(op: Option<&EditOp>) -> (f32, f32, f32, f32) {
    match op {
        Some(EditOp::Tuning {
            fill_light,
            highlights,
            shadows,
            temperature,
            ..
        }) => (*fill_light, *highlights, *shadows, *temperature),
        _ => (0.0, 0.0, 0.0, 0.0),
    }
}

impl EditorVm {
    /// An editor for `id` on the Basic Fixes tab.
    pub fn new(lb: &Lightbox, id: PhotoId) -> Self {
        Self {
            lb: lb.clone(),
            id,
            tab: EditorTab::Basic,
            draft: None,
            show_before: false,
        }
    }

    /// The photo being edited.
    pub fn photo(&self) -> PhotoId {
        self.id
    }

    /// The selected tab.
    pub fn tab(&self) -> EditorTab {
        self.tab
    }

    /// Selects a tab; an uncommitted draft is dropped.
    pub fn set_tab(&mut self, tab: EditorTab) {
        if tab != self.tab {
            self.draft = None;
        }
        self.tab = tab;
    }

    /// The stack stored in the catalog.
    pub fn committed(&self) -> Result<EditStack> {
        self.lb.edits().stack(self.id)
    }

    /// The uncommitted op, if a slider is being dragged.
    pub fn draft(&self) -> Option<&EditOp> {
        self.draft.as_ref()
    }

    /// The stack to preview: committed ops with the draft replacing a same-kind top op or
    /// appended after them. Before/After shows the empty stack.
    pub fn preview_stack(&self) -> Result<EditStack> {
        let mut stack = self.committed()?;
        if self.show_before {
            stack.ops.clear();
            return Ok(stack);
        }
        if let Some(draft) = &self.draft {
            let same = stack
                .ops
                .last()
                .is_some_and(|top| std::mem::discriminant(top) == std::mem::discriminant(draft));
            if same {
                stack.ops.pop();
            }
            stack.ops.push(draft.clone());
        }
        Ok(stack)
    }

    /// The current value of a tuning slider (draft first, then the top committed op).
    pub fn tuning_value(&self, slider: TuningSlider) -> Result<f32> {
        let committed = self.committed()?;
        let source = self.draft.as_ref().or_else(|| committed.ops.last());
        let (fill, high, shadow, temp) = tuning_of(source);
        Ok(match slider {
            TuningSlider::FillLight => fill,
            TuningSlider::Highlights => high,
            TuningSlider::Shadows => shadow,
            TuningSlider::Temperature => temp,
        })
    }

    /// Moves a tuning slider; nothing is stored until [`Self::commit`].
    pub fn set_tuning(&mut self, slider: TuningSlider, value: f32) -> Result<()> {
        let committed = self.committed()?;
        let base = self.draft.as_ref().or_else(|| committed.ops.last());
        let (mut fill, mut high, mut shadow, mut temp) = tuning_of(base);
        let neutral = match base {
            Some(EditOp::Tuning { neutral, .. }) => *neutral,
            _ => None,
        };
        match slider {
            TuningSlider::FillLight => fill = value,
            TuningSlider::Highlights => high = value,
            TuningSlider::Shadows => shadow = value,
            TuningSlider::Temperature => temp = value,
        }
        self.set_draft(EditOp::Tuning {
            fill_light: fill,
            highlights: high,
            shadows: shadow,
            temperature: temp,
            neutral,
        })
    }

    /// Sets any parametrized op as the draft (straighten, sharpen, vignette, ...); out-of-range
    /// values are clamped.
    pub fn set_draft(&mut self, op: EditOp) -> Result<()> {
        self.draft = Some(op.validate()?);
        Ok(())
    }

    /// Drops the draft (Cancel).
    pub fn cancel(&mut self) {
        self.draft = None;
    }

    /// Stores the draft as one op, replacing the top op of the same kind. Returns whether
    /// anything was stored.
    pub fn commit(&mut self) -> Result<bool> {
        let Some(op) = self.draft.take() else {
            return Ok(false);
        };
        self.lb.edits().adjust(self.id, op)?;
        Ok(true)
    }

    /// Applies a one-shot op (Lucky, Auto Contrast, rotate, effect tile) as its own step.
    pub fn apply(&mut self, op: EditOp) -> Result<()> {
        self.draft = None;
        self.lb.edits().push(self.id, op)?;
        Ok(())
    }

    /// "I'm Feeling Lucky".
    pub fn lucky(&mut self) -> Result<()> {
        self.apply(EditOp::FeelingLucky)
    }

    /// Undo; drops the draft first.
    pub fn undo(&mut self) -> Result<bool> {
        self.draft = None;
        let had = self.can_undo()?;
        self.lb.edits().undo(self.id)?;
        Ok(had)
    }

    /// Redo.
    pub fn redo(&mut self) -> Result<bool> {
        self.draft = None;
        let had = self.can_redo()?;
        self.lb.edits().redo(self.id)?;
        Ok(had)
    }

    /// Removes every edit as one undoable step.
    pub fn revert(&mut self) -> Result<()> {
        self.draft = None;
        self.lb.edits().revert(self.id)?;
        Ok(())
    }

    /// Whether Undo is enabled.
    pub fn can_undo(&self) -> Result<bool> {
        self.lb.edits().can_undo(self.id)
    }

    /// Whether Redo is enabled.
    pub fn can_redo(&self) -> Result<bool> {
        self.lb.edits().can_redo(self.id)
    }

    /// Holding `\` shows the original.
    pub fn set_show_before(&mut self, on: bool) {
        self.show_before = on;
    }

    /// Whether the before view is showing.
    pub fn showing_before(&self) -> bool {
        self.show_before
    }

    /// Renders the preview stack at a proxy size; `cancel` abandons a stale render.
    pub fn render(&self, max_edge: u32, cancel: &AtomicBool) -> Result<RgbaImage> {
        let stack = self.preview_stack()?;
        self.lb
            .edits()
            .render_stack_preview(self.id, &stack, max_edge, cancel)
    }
}
