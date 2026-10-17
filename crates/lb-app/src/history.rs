//! App-level undo for organize actions (SPEC §7.4).

use crate::error::Result;
use crate::events::{AppEvent, EventBus};
use lb_catalog::Catalog;
use parking_lot::Mutex;
use std::sync::Arc;

/// Maximum number of undoable actions kept.
pub const HISTORY_LIMIT: usize = 200;

/// Applies one direction of an action and reports what changed.
pub type HistoryAction = Box<dyn Fn(&Catalog) -> Result<Vec<AppEvent>> + Send + Sync>;

/// A reversible organize action.
pub struct HistoryEntry {
    /// Shown in menus, e.g. "Star 3 photos".
    pub label: String,
    /// Reverts the action.
    pub undo: HistoryAction,
    /// Re-applies the action.
    pub redo: HistoryAction,
}

impl HistoryEntry {
    /// An entry from a label and its two directions.
    pub fn new(
        label: impl Into<String>,
        undo: impl Fn(&Catalog) -> Result<Vec<AppEvent>> + Send + Sync + 'static,
        redo: impl Fn(&Catalog) -> Result<Vec<AppEvent>> + Send + Sync + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            undo: Box::new(undo),
            redo: Box::new(redo),
        }
    }
}

#[derive(Default)]
struct Stacks {
    undo: Vec<HistoryEntry>,
    redo: Vec<HistoryEntry>,
}

/// Undo/redo stacks for organize commands (star, rate, caption, tags, albums, hide).
///
/// File operations are deliberately not recorded: the system trash is their safety net.
#[derive(Clone)]
pub struct HistoryService {
    catalog: Catalog,
    bus: EventBus,
    stacks: Arc<Mutex<Stacks>>,
}

impl HistoryService {
    pub(crate) fn new(catalog: Catalog, bus: EventBus) -> Self {
        Self {
            catalog,
            bus,
            stacks: Arc::new(Mutex::new(Stacks::default())),
        }
    }

    /// Applies `entry` (running its `redo` direction) and, if that succeeds, records it and
    /// publishes the events it produced. Nothing is recorded when the action fails.
    pub fn execute(&self, entry: HistoryEntry) -> Result<()> {
        let events = (entry.redo)(&self.catalog)?;
        self.record(entry);
        events.into_iter().for_each(|e| self.bus.emit(e));
        Ok(())
    }

    /// Records an action that has just been applied. Clears the redo stack.
    pub fn record(&self, entry: HistoryEntry) {
        let mut s = self.stacks.lock();
        s.redo.clear();
        s.undo.push(entry);
        if s.undo.len() > HISTORY_LIMIT {
            s.undo.remove(0);
        }
    }

    /// Whether [`HistoryService::undo`] would do something.
    pub fn can_undo(&self) -> bool {
        !self.stacks.lock().undo.is_empty()
    }

    /// Whether [`HistoryService::redo`] would do something.
    pub fn can_redo(&self) -> bool {
        !self.stacks.lock().redo.is_empty()
    }

    /// Label of the action [`HistoryService::undo`] would revert.
    pub fn undo_label(&self) -> Option<String> {
        self.stacks.lock().undo.last().map(|e| e.label.clone())
    }

    /// Label of the action [`HistoryService::redo`] would re-apply.
    pub fn redo_label(&self) -> Option<String> {
        self.stacks.lock().redo.last().map(|e| e.label.clone())
    }

    /// Reverts the most recent action and returns its label. On failure the entry stays on the
    /// undo stack.
    pub fn undo(&self) -> Result<Option<String>> {
        let mut s = self.stacks.lock();
        let Some(entry) = s.undo.last() else {
            return Ok(None);
        };
        let events = (entry.undo)(&self.catalog)?;
        let Some(entry) = s.undo.pop() else {
            return Ok(None);
        };
        let label = entry.label.clone();
        s.redo.push(entry);
        drop(s);
        events.into_iter().for_each(|e| self.bus.emit(e));
        Ok(Some(label))
    }

    /// Re-applies the most recently undone action and returns its label.
    pub fn redo(&self) -> Result<Option<String>> {
        let mut s = self.stacks.lock();
        let Some(entry) = s.redo.last() else {
            return Ok(None);
        };
        let events = (entry.redo)(&self.catalog)?;
        let Some(entry) = s.redo.pop() else {
            return Ok(None);
        };
        let label = entry.label.clone();
        s.undo.push(entry);
        drop(s);
        events.into_iter().for_each(|e| self.bus.emit(e));
        Ok(Some(label))
    }

    /// Forgets all history.
    pub fn clear(&self) {
        let mut s = self.stacks.lock();
        s.undo.clear();
        s.redo.clear();
    }
}
