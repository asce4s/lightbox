//! UI-agnostic viewer: geometry, motion, navigation and loading (SPEC-VIEWER).

mod ambient;
mod motion;
mod transform;

pub use ambient::{AmbientCache, bucket_stage};

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
