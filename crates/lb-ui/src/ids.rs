//! Stable element ids shared by views and UI tests (SPEC §9.8).
//!
//! Every interactive or asserted element takes its id from here and calls `.test_support()`.

use gpui_kit::ElementId;
use lb_core::{AlbumId, PhotoId};

/// Label shown by the M0.6 hello window.
pub const HELLO_LABEL: &str = "hello-label";

macro_rules! ids {
    ($($name:ident = $value:literal),* $(,)?) => {
        $(
            #[doc = concat!("Element id `", $value, "`.")]
            pub const $name: &str = $value;
        )*
    };
}

ids! {
    ROOT = "lightbox-root",
    TITLE_BAR = "title-bar",
    SEARCH_INPUT = "search-input",
    CHIP_BAR = "chip-bar",
    CHIP_STARRED = "chip-starred",
    CHIP_VIDEO = "chip-video",
    CHIP_FACES = "chip-faces",
    CHIP_GEO = "chip-geo",
    CHIP_EDITED = "chip-edited",
    SIDEBAR = "sidebar",
    SIDEBAR_ALL = "sidebar-all",
    SIDEBAR_STARRED = "sidebar-starred",
    SIDEBAR_RECENT = "sidebar-recent",
    SIDEBAR_UNNAMED = "sidebar-unnamed",
    NEW_ALBUM_BUTTON = "new-album",
    GRID = "grid",
    GRID_EMPTY = "grid-empty",
    GRID_BAND = "grid-band",
    INFO_PANEL = "info-panel",
    ZOOM_SLIDER = "zoom-slider",
    STATUS_BAR = "status-bar",
    STATUS_TEXT = "status-text",
    TRAY = "tray",
    TRAY_HOLD = "tray-hold",
    TRAY_CLEAR = "tray-clear",
    TRAY_STAR = "tray-star",
    TRAY_ROTATE_LEFT = "tray-rotate-left",
    TRAY_ROTATE_RIGHT = "tray-rotate-right",
    TRAY_ADD_TAG = "tray-add-tag",
    TRAY_ADD_ALBUM = "tray-add-album",
    TRAY_EXPORT = "tray-export",
    EXPORT_BUTTON = "export-button",
    CONTEXT_MENU = "context-menu",
    MENU_OPEN = "menu-open",
    MENU_STAR = "menu-star",
    MENU_HOLD = "menu-hold",
    MENU_EXPORT = "menu-export",
    MENU_RENAME = "menu-rename",
    MENU_DELETE = "menu-delete",
    MENU_HIDE = "menu-hide",
    MENU_REVEAL = "menu-reveal",
    VIEWER = "viewer",
    VIEWER_NEXT = "viewer-next",
    VIEWER_PREV = "viewer-prev",
    EDITOR = "editor",
    EDITOR_TAB_BASIC = "editor-tab-basic",
    EDITOR_TAB_TUNING = "editor-tab-tuning",
    EDITOR_TAB_EFFECTS = "editor-tab-effects",
    EDITOR_LUCKY = "editor-lucky",
    EDITOR_UNDO = "editor-undo",
    EDITOR_REDO = "editor-redo",
    EDITOR_REVERT = "editor-revert",
    SLIDER_FILL_LIGHT = "slider-fill-light",
    SLIDER_HIGHLIGHTS = "slider-highlights",
    SLIDER_SHADOWS = "slider-shadows",
    SLIDER_TEMPERATURE = "slider-temperature",
    EXPORT_DIALOG = "export-dialog",
    EXPORT_CONFIRM = "export-confirm",
    EXPORT_CANCEL = "export-cancel",
    EXPORT_DEST = "export-dest",
    EXPORT_STATUS = "export-status",
    CONFIRM_DIALOG = "confirm-dialog",
    CONFIRM_OK = "confirm-ok",
    CONFIRM_CANCEL = "confirm-cancel",
    RENAME_DIALOG = "rename-dialog",
    RENAME_INPUT = "rename-input",
    RENAME_ERROR = "rename-error",
    RENAME_OK = "rename-ok",
    RENAME_CANCEL = "rename-cancel",
    ALBUM_PICKER = "album-picker",
    ALBUM_PICKER_NEW = "album-picker-new",
    TAG_DIALOG = "tag-dialog",
    TAG_INPUT = "tag-input",
    TAG_OK = "tag-ok",
    TAG_CANCEL = "tag-cancel",
    TOAST = "toast",
    THEME_TOGGLE = "theme-toggle",
    VIEWER_BACK = "viewer-back",
    VIEWER_EDIT = "viewer-edit",
    VIEWER_INFO = "viewer-info",
    VIEWER_INFO_PANEL = "viewer-info-panel",
    VIEWER_IMAGE = "viewer-image",
    VIEWER_FILMSTRIP = "viewer-filmstrip",
    VIEWER_ZOOM_LABEL = "viewer-zoom-label",
    EDITOR_BACK = "editor-back",
    EDITOR_BEFORE_AFTER = "editor-before-after",
    EDITOR_EXPORT = "editor-export",
    EDITOR_STAGE = "editor-stage",
    EDITOR_IMAGE = "editor-image",
    EDITOR_BUSY = "editor-busy",
    EDITOR_AUTO_CONTRAST = "editor-auto-contrast",
    EDITOR_AUTO_COLOR = "editor-auto-color",
    EDITOR_CROP = "editor-crop",
    EDITOR_STRAIGHTEN = "editor-straighten",
    EDITOR_REDEYE = "editor-redeye",
    SLIDER_BASIC_FILL_LIGHT = "slider-basic-fill-light",
    SLIDER_STRAIGHTEN = "slider-straighten",
    TOOL_PANEL = "tool-panel",
    TOOL_APPLY = "tool-apply",
    TOOL_CANCEL = "tool-cancel",
    TOOL_CLEAR = "tool-clear",
    CROP_OVERLAY = "crop-overlay",
    CROP_RECT = "crop-rect",
    REDEYE_OVERLAY = "redeye-overlay",
    EFFECT_PANEL = "effect-panel",
    EFFECT_APPLY = "effect-apply",
    EFFECT_CANCEL = "effect-cancel",
    SLIDESHOW_BUTTON = "slideshow-button",
    SLIDESHOW = "slideshow",
    SLIDESHOW_CAPTION = "slideshow-caption",
    SLIDESHOW_CONTROLS = "slideshow-controls",
    SLIDESHOW_PREV = "slideshow-prev",
    SLIDESHOW_PLAY = "slideshow-play",
    SLIDESHOW_NEXT = "slideshow-next",
    SLIDESHOW_DURATION = "slideshow-duration",
    SLIDESHOW_SHUFFLE = "slideshow-shuffle",
    SLIDESHOW_LOOP = "slideshow-loop",
    SLIDESHOW_CAPTIONS = "slideshow-captions",
    SLIDESHOW_EXIT = "slideshow-exit",
    TIMELINE = "timeline",
    TIMELINE_CLEAR = "timeline-clear",
}

fn named(name: &'static str, n: u64) -> ElementId {
    ElementId::NamedInteger(name.into(), n)
}

fn unsigned(n: i64) -> u64 {
    u64::try_from(n).unwrap_or(0)
}

/// A photo cell in the grid.
pub fn grid_cell(id: PhotoId) -> ElementId {
    named("grid-cell", unsigned(id.get()))
}

/// The duration badge of a video cell.
pub fn video_badge(id: PhotoId) -> ElementId {
    named("video-badge", unsigned(id.get()))
}

/// The header of group `idx`.
pub fn group_header(idx: usize) -> ElementId {
    named("group-header", idx as u64)
}

/// An album row in the sidebar.
pub fn album_item(id: AlbumId) -> ElementId {
    named("album-item", unsigned(id.get()))
}

/// An entry of the album picker.
pub fn album_pick(id: AlbumId) -> ElementId {
    named("album-pick", unsigned(id.get()))
}

/// A folder row in the sidebar.
pub fn folder_item(id: lb_core::FolderId) -> ElementId {
    named("folder-item", unsigned(id.get()))
}

/// A person row in the sidebar.
pub fn person_item(id: lb_core::PersonId) -> ElementId {
    named("person-item", unsigned(id.get()))
}

/// A thumbnail in the photo tray.
pub fn tray_cell(id: PhotoId) -> ElementId {
    named("tray-cell", unsigned(id.get()))
}

/// An effect tile in the editor.
pub fn effect_tile(name: &str) -> ElementId {
    ElementId::Name(format!("effect-{name}").into())
}

/// A thumbnail in the viewer's filmstrip.
pub fn filmstrip_cell(id: PhotoId) -> ElementId {
    named("filmstrip-cell", unsigned(id.get()))
}

/// An aspect preset button of the crop tool (`free`, `original`, `1x1`, ...).
pub fn crop_aspect(key: &str) -> ElementId {
    ElementId::Name(format!("crop-aspect-{key}").into())
}

/// A month segment of the timeline scrubber.
pub fn timeline_month(index: usize) -> ElementId {
    named("timeline-month", index as u64)
}

/// A resize handle of the crop rectangle (`top-left`, `right`, ...).
pub fn crop_handle(key: &str) -> ElementId {
    ElementId::Name(format!("crop-handle-{key}").into())
}

/// A slider of an effect's parameter panel.
pub fn effect_param(key: &str) -> ElementId {
    ElementId::Name(format!("effect-param-{key}").into())
}

/// A rectangle drawn by the red-eye tool.
pub fn redeye_region(index: usize) -> ElementId {
    named("redeye-region", index as u64)
}
