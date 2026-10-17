//! Non-destructive edit stacks: push, slider replace, undo, redo, revert and previews
//! (SPEC §6.6.1).

use crate::error::{AppError, Result};
use crate::events::AppEvent;
use crate::lightbox::Lightbox;
use image::{DynamicImage, RgbaImage};
use lb_core::{EditOp, EditStack, Photo, PhotoId};
use lb_imaging::convert::to_rgba8;
use lb_imaging::{Cancelled, RenderCtx, RenderTarget, render_cancellable};
use std::sync::atomic::AtomicBool;

/// A stack together with its redo history. Each redo entry is a batch of operations: undoing
/// one op creates a batch of one, reverting creates a single batch of the whole stack so that
/// one redo restores it (SPEC §6.6.1 "Revert is undoable once").
#[derive(Debug, Clone, Default, PartialEq)]
struct EditState {
    stack: EditStack,
    redo: Vec<Vec<EditOp>>,
}

/// Whether consecutive applications of this op replace each other instead of stacking, i.e.
/// whether it is driven by a slider.
fn is_slider(op: &EditOp) -> bool {
    matches!(
        op,
        EditOp::Straighten { .. }
            | EditOp::FillLight { .. }
            | EditOp::Tuning { .. }
            | EditOp::Sharpen { .. }
            | EditOp::FilmGrain { .. }
            | EditOp::Tint { .. }
            | EditOp::Saturation { .. }
            | EditOp::SoftFocus { .. }
            | EditOp::Glow { .. }
            | EditOp::FocalBlackAndWhite { .. }
            | EditOp::GraduatedTint { .. }
            | EditOp::Vignette { .. }
    )
}

/// Reads and changes the edit stacks of photos.
#[derive(Clone)]
pub struct EditService {
    lb: Lightbox,
}

impl Lightbox {
    /// Edit stacks, undo/redo and preview rendering.
    pub fn edits(&self) -> EditService {
        EditService { lb: self.clone() }
    }
}

impl EditService {
    fn photo(&self, id: PhotoId) -> Result<Photo> {
        self.lb
            .catalog()
            .get_photo(id)?
            .ok_or(AppError::NotFound("photo"))
    }

    fn load(&self, photo: &Photo) -> Result<EditState> {
        let Some(rec) = self.lb.catalog().get_edit(photo.id)? else {
            return Ok(EditState {
                stack: EditStack {
                    version: photo.edit_version,
                    ops: Vec::new(),
                },
                redo: Vec::new(),
            });
        };
        let ops = serde_json::from_str(&rec.ops_json).unwrap_or_else(|e| {
            tracing::warn!(photo = %photo.id, error = %e, "unreadable edit stack ignored");
            Vec::new()
        });
        let redo = serde_json::from_str(&rec.redo_json).unwrap_or_default();
        Ok(EditState {
            stack: EditStack {
                version: photo.edit_version,
                ops,
            },
            redo,
        })
    }

    fn save(&self, photo: &Photo, mut state: EditState) -> Result<EditStack> {
        let catalog = self.lb.catalog();
        let version = if state.stack.ops.is_empty() && state.redo.is_empty() {
            catalog.clear_edit(photo.id)?
        } else {
            catalog.set_edit(
                photo.id,
                serde_json::to_string(&state.stack.ops).map_err(invalid_json)?,
                serde_json::to_string(&state.redo).map_err(invalid_json)?,
                self.lb.clock().now(),
            )?
        };
        state.stack.version = version;
        self.lb.emit(AppEvent::PhotoEdited {
            id: photo.id,
            version,
        });
        Ok(state.stack)
    }

    fn modify(
        &self,
        id: PhotoId,
        f: impl FnOnce(&mut EditState) -> Result<bool>,
    ) -> Result<EditStack> {
        let _guard = self.lb.inner.edit_lock.lock();
        let photo = self.photo(id)?;
        let mut state = self.load(&photo)?;
        if f(&mut state)? {
            self.save(&photo, state)
        } else {
            Ok(state.stack)
        }
    }

    /// The current stack of `id` (empty when unedited). `version` is the photo's `edit_version`.
    pub fn stack(&self, id: PhotoId) -> Result<EditStack> {
        Ok(self.load(&self.photo(id)?)?.stack)
    }

    /// Whether [`EditService::undo`] would change anything.
    pub fn can_undo(&self, id: PhotoId) -> Result<bool> {
        Ok(!self.stack(id)?.is_empty())
    }

    /// Whether [`EditService::redo`] would change anything.
    pub fn can_redo(&self, id: PhotoId) -> Result<bool> {
        Ok(!self.load(&self.photo(id)?)?.redo.is_empty())
    }

    /// Appends `op` after validating it. Clears the redo history.
    pub fn push(&self, id: PhotoId, op: EditOp) -> Result<EditStack> {
        let op = op.validate()?;
        self.modify(id, |s| {
            s.stack.ops.push(op);
            s.redo.clear();
            Ok(true)
        })
    }

    /// Commits a slider change: replaces the top op if it is the same kind of slider op,
    /// otherwise appends. Dragging a slider therefore produces a single undo step.
    pub fn adjust(&self, id: PhotoId, op: EditOp) -> Result<EditStack> {
        let op = op.validate()?;
        self.modify(id, |s| {
            let same_kind = s.stack.ops.last().is_some_and(|top| {
                is_slider(&op) && std::mem::discriminant(top) == std::mem::discriminant(&op)
            });
            if same_kind {
                if let Some(top) = s.stack.ops.last_mut() {
                    *top = op;
                }
            } else {
                s.stack.ops.push(op);
            }
            s.redo.clear();
            Ok(true)
        })
    }

    /// Removes the last op, keeping it for [`EditService::redo`]. No-op on an empty stack.
    pub fn undo(&self, id: PhotoId) -> Result<EditStack> {
        self.modify(id, |s| {
            let Some(op) = s.stack.ops.pop() else {
                return Ok(false);
            };
            s.redo.push(vec![op]);
            Ok(true)
        })
    }

    /// Re-applies the most recently undone op (or reverted stack). No-op without redo history.
    pub fn redo(&self, id: PhotoId) -> Result<EditStack> {
        self.modify(id, |s| {
            let Some(batch) = s.redo.pop() else {
                return Ok(false);
            };
            s.stack.ops.extend(batch);
            Ok(true)
        })
    }

    /// Removes every op. One [`EditService::redo`] brings the whole stack back.
    pub fn revert(&self, id: PhotoId) -> Result<EditStack> {
        self.modify(id, |s| {
            if s.stack.ops.is_empty() {
                return Ok(false);
            }
            let ops = std::mem::take(&mut s.stack.ops);
            s.redo.clear();
            s.redo.push(ops);
            Ok(true)
        })
    }

    /// Renders the photo's committed stack at a proxy size for the editor and viewer.
    pub fn render_preview(&self, id: PhotoId, max_edge: u32) -> Result<RgbaImage> {
        let stack = self.stack(id)?;
        self.render_stack_preview(id, &stack, max_edge, &AtomicBool::new(false))
    }

    /// Renders an uncommitted `stack` (slider drag) at a proxy size. Returns
    /// [`AppError::Cancelled`] when `cancel` is raised.
    pub fn render_stack_preview(
        &self,
        id: PhotoId,
        stack: &EditStack,
        max_edge: u32,
        cancel: &AtomicBool,
    ) -> Result<RgbaImage> {
        let photo = self.photo(id)?;
        let src: DynamicImage = lb_media::decode(&photo.path, Some(max_edge))?;
        let ctx = RenderCtx::new(&photo.quick_key, RenderTarget::Proxy { max_edge });
        let out = render_cancellable(&src, stack, &ctx, cancel)
            .map_err(|Cancelled| AppError::Cancelled)?;
        Ok(to_rgba8(&out))
    }
}

fn invalid_json(e: serde_json::Error) -> AppError {
    AppError::Invalid(format!("edit stack could not be serialized: {e}"))
}
