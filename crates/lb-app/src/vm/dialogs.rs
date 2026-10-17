//! Small modal dialogs: confirmations and rename (SPEC §9.6).

use crate::error::{AppError, Result};
use crate::lightbox::Lightbox;
use lb_core::PhotoId;

/// What a confirmation dialog does when accepted.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ConfirmAction {
    /// Move photos to the system trash.
    Delete(Vec<PhotoId>),
    /// Remove every edit of a photo (one undoable step).
    Revert(PhotoId),
}

/// A yes/no question with the action it guards.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ConfirmVm {
    title: String,
    message: String,
    ok_label: String,
    action: ConfirmAction,
}

impl ConfirmVm {
    /// Asks before moving `ids` to the trash. Deleting is not undoable from the history, which the
    /// message says (SPEC §7.4).
    pub fn delete(ids: Vec<PhotoId>) -> Self {
        let n = ids.len();
        let what = if n == 1 {
            "this photo".to_owned()
        } else {
            format!("these {n} photos")
        };
        Self {
            title: if n == 1 {
                "Delete photo?"
            } else {
                "Delete photos?"
            }
            .to_owned(),
            message: format!(
                "Move {what} to the trash? This cannot be undone with Ctrl+Z; restore them from the system trash."
            ),
            ok_label: "Delete".to_owned(),
            action: ConfirmAction::Delete(ids),
        }
    }

    /// Asks before removing all edits of `id`.
    pub fn revert(id: PhotoId) -> Self {
        Self {
            title: "Revert to original?".to_owned(),
            message: "Remove all edits from this photo. You can undo this with Redo.".to_owned(),
            ok_label: "Revert".to_owned(),
            action: ConfirmAction::Revert(id),
        }
    }

    /// Dialog title.
    pub fn title(&self) -> &str {
        &self.title
    }

    /// Explanation shown under the title.
    pub fn message(&self) -> &str {
        &self.message
    }

    /// Label of the accept button.
    pub fn ok_label(&self) -> &str {
        &self.ok_label
    }

    /// The guarded action.
    pub fn action(&self) -> &ConfirmAction {
        &self.action
    }

    /// Runs the guarded action; returns how many photos were affected.
    pub fn accept(&self, lb: &Lightbox) -> Result<usize> {
        match &self.action {
            ConfirmAction::Delete(ids) => Ok(lb.photos().delete(ids)?.deleted.len()),
            ConfirmAction::Revert(id) => {
                lb.edits().revert(*id)?;
                Ok(1)
            }
        }
    }
}

/// Rename dialog for one photo file.
#[derive(Debug, Clone)]
pub struct RenameVm {
    id: PhotoId,
    original: String,
    text: String,
    error: Option<String>,
}

impl RenameVm {
    /// A dialog prefilled with the current file stem.
    pub fn new(lb: &Lightbox, id: PhotoId) -> Result<Self> {
        let photo = lb.photos().get(id)?.ok_or(AppError::NotFound("photo"))?;
        let stem = photo
            .path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        Ok(Self {
            id,
            original: stem.clone(),
            text: stem,
            error: None,
        })
    }

    /// The photo being renamed.
    pub fn id(&self) -> PhotoId {
        self.id
    }

    /// The text in the field.
    pub fn text(&self) -> &str {
        &self.text
    }

    /// Why the current text is not acceptable, if it is not.
    pub fn error(&self) -> Option<&str> {
        self.error.as_deref()
    }

    /// Whether the dialog can be accepted.
    pub fn can_accept(&self) -> bool {
        self.error.is_none() && self.text.trim() != self.original
    }

    /// Updates the field and validates it.
    pub fn set_text(&mut self, lb: &Lightbox, text: &str) {
        self.text = text.to_owned();
        self.error = lb
            .photos()
            .check_rename(self.id, text)
            .err()
            .map(|e| e.user_message());
    }

    /// Renames the file. On failure the error is kept for display and returned.
    pub fn accept(&mut self, lb: &Lightbox) -> Result<()> {
        match lb.photos().rename(self.id, &self.text) {
            Ok(_) => Ok(()),
            Err(e) => {
                self.error = Some(e.user_message());
                Err(e)
            }
        }
    }
}
