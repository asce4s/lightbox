//! UI-agnostic viewer: geometry, motion, navigation and loading (SPEC-VIEWER).

mod ambient;
mod entry;
mod loader;
mod motion;
mod perf;
mod service;
mod transform;

pub use ambient::{AmbientCache, bucket_stage};
pub use entry::{
    FolderRefresh, FolderSort, FolderWatch, NavSet, NavSource, ViewerEntry, entry_error,
    entry_from_id, list_folder,
};
pub use loader::{LoadLevel, MEMORY_BUDGET, MediaBytes, ViewerLoader};
pub use perf::{ViewerPerf, capture_viewer_perf};
pub use service::ViewerService;

pub use motion::{
    LayerState, Motion, MotionMode, Presented, ease_in_cubic, ease_in_out_cubic, ease_out_cubic,
    ease_out_quad,
};
pub use transform::{
    DRAG_THRESHOLD_PX, FILMSTRIP_HEIGHT, FramedView, Gesture, GestureState, INFO_PANEL_WIDTH,
    Insets, NAV_DEBOUNCE, PIXEL_ZOOM_DIVISOR, RectF, SWIPE_THRESHOLD, ScrollDelta, ScrollMods,
    TOOLBAR_GAP, TOOLBAR_HEIGHT, TOP_BAR_HEIGHT, ViewTransform, WheelMode, ZOOM_MAX, ZOOM_STEP,
    clamp_pan, fit_area, fit_rect, fit_scale, is_drag, stage_insets, zoom_about,
};
