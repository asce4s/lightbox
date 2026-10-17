//! Plain-Rust view-models: UI state and intents without any GPUI types (SPEC §7.5).

mod crop;
mod dialogs;
mod editor;
mod effects;
mod export_dialog;
mod grid;
mod library;
mod redeye;
mod sidebar;
mod slideshow;
mod viewer;

pub use crop::{AspectPreset, CropHandle, CropVm};
pub use dialogs::{ConfirmAction, ConfirmVm, RenameVm};
pub use editor::{EditorTab, EditorVm, TuningSlider};
pub use effects::{EFFECTS, EffectPanelVm, EffectSpec, ParamSpec, effect, hue_to_rgb};
pub use export_dialog::{ExportDialogVm, ExportField, ExportPhase, FieldError, SizeChoice};
pub use grid::{GAP, GridLayout, GridRow, HEADER_HEIGHT, ZOOM_RANGE, columns_for};
pub use library::{DEFAULT_ZOOM, LibraryVm, Modifiers, NavKey, SEARCH_DEBOUNCE};
pub use redeye::{DisplayRect, RedEyeVm};
pub use sidebar::{SidebarItem, SidebarKey, SidebarSection, SidebarVm};
pub use slideshow::{
    CONTROLS_IDLE, DURATION_RANGE, PRELOAD_AHEAD, SlideshowOptions, SlideshowState,
};
pub use viewer::{ViewerVm, ZOOM_LIMITS, ZoomMode};
