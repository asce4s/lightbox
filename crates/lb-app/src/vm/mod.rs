//! Plain-Rust view-models: UI state and intents without any GPUI types (SPEC §7.5).

mod editor;
mod export_dialog;
mod grid;
mod library;
mod viewer;

pub use editor::{EditorTab, EditorVm, TuningSlider};
pub use export_dialog::{ExportDialogVm, ExportField, ExportPhase, FieldError, SizeChoice};
pub use grid::{GAP, GridLayout, GridRow, HEADER_HEIGHT, ZOOM_RANGE, columns_for};
pub use library::{DEFAULT_ZOOM, LibraryVm, Modifiers, NavKey, SEARCH_DEBOUNCE};
pub use viewer::{ViewerVm, ZOOM_LIMITS, ZoomMode};
