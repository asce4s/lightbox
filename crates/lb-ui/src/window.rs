//! The library window: state, intents and event handling (SPEC §9.1). Rendering lives in
//! `render`, `chrome` and `overlays`.

use crate::ids;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::slider::{SliderEvent, SliderState};
use gpui_kit::component::{ResizableState, VirtualListScrollHandle};
use gpui_kit::{
    AnyWindowHandle, App, AppContext as _, Context, Entity, FocusHandle, Focusable, Pixels, Point,
    Result, ScrollStrategy, Subscription, Window, WindowBounds, WindowOptions, px, size,
};
use lb_app::vm::{
    ConfirmVm, ExportDialogVm, LibraryVm, Modifiers, NavKey, RenameVm, SidebarKey, SidebarVm,
    ZOOM_RANGE,
};
use lb_app::{AppEvent, EventReceiver, InstanceServer, Launch, Lightbox, ThemeChoice, ThumbSize};
use lb_core::{AlbumId, PhotoId};
use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::time::Duration;

/// Width of the sidebar panel when the window opens.
pub const SIDEBAR_WIDTH: f32 = 220.0;
/// Width of the info panel when it is shown.
pub const INFO_WIDTH: f32 = 280.0;
/// Size of the window created by [`LightboxWindow::open_for_test`].
pub const TEST_WINDOW_SIZE: (f32, f32) = (1280.0, 800.0);
/// How often the production window drains events and applies debounced searches.
pub const PUMP_INTERVAL: Duration = Duration::from_millis(16);

/// The screen the window shows.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Route {
    /// Grid, sidebar and tray.
    Library,
    /// Single photo.
    Viewer,
    /// Editor.
    Editor,
    /// Full-window slideshow.
    Slideshow,
}

/// A modal or popup drawn above the library.
pub enum Overlay {
    /// Nothing is open.
    None,
    /// Right-click menu for a photo.
    Menu {
        /// Photo the menu acts on.
        photo: PhotoId,
        /// Window position of the click.
        at: Point<Pixels>,
    },
    /// Yes/no confirmation.
    Confirm(ConfirmVm),
    /// Rename a photo file.
    Rename {
        /// Dialog state.
        vm: RenameVm,
        /// The text field.
        input: Entity<InputState>,
        /// Keeps the field subscription alive.
        _sub: Subscription,
    },
    /// Choose the album to add the target photos to.
    AlbumPicker,
    /// Type tags to add to the target photos.
    Tag {
        /// The text field (comma separated names).
        input: Entity<InputState>,
    },
    /// Export dialog.
    Export {
        /// Dialog state.
        vm: Box<ExportDialogVm>,
        /// Destination folder field.
        dest: Entity<InputState>,
        /// Keeps the field subscription alive.
        _sub: Subscription,
    },
}

/// Root view of the library.
pub struct LightboxWindow {
    pub(crate) app: Lightbox,
    pub(crate) vm: LibraryVm,
    pub(crate) sidebar: SidebarVm,
    events: EventReceiver<AppEvent>,
    pub(crate) focus: FocusHandle,
    pub(crate) viewer_focus: FocusHandle,
    pub(crate) scroll: VirtualListScrollHandle,
    pub(crate) panels: Entity<ResizableState>,
    pub(crate) search: Entity<InputState>,
    pub(crate) zoom: Entity<SliderState>,
    pub(crate) overlay: Overlay,
    pub(crate) toast: Option<String>,
    pub(crate) thumbs: HashMap<(PhotoId, ThumbSize), PathBuf>,
    requested: HashSet<(PhotoId, ThumbSize)>,
    pub(crate) scan: Option<(usize, usize)>,
    pub(crate) jobs: usize,
    pub(crate) info_open: bool,
    pub(crate) opened: Option<PhotoId>,
    pub(crate) viewer: Option<crate::viewer::ViewerState>,
    pub(crate) editor: Option<crate::editor::EditorState>,
    pub(crate) viewport: (f32, f32),
    pub(crate) theme: ThemeChoice,
    pub(crate) slideshow: Option<lb_app::vm::SlideshowState>,
    pub(crate) slideshow_slider: Entity<SliderState>,
    pub(crate) slideshow_fullscreen: bool,
    pub(crate) slideshow_controls_shown: bool,
    last_anchor_row: Option<usize>,
    pub(crate) timeline_drag: Option<crate::timeline::TimelineDrag>,
    pub(crate) band: Option<crate::band::BandDrag>,
    pub(crate) grid_bounds: crate::band::GridBounds,
    instance: Option<InstanceServer>,
    _subscriptions: Vec<Subscription>,
}

impl LightboxWindow {
    /// Builds the window state inside `window`. `ticker` starts the background loop that drains
    /// application events; tests pass `false` and call [`LightboxWindow::pump`] themselves.
    pub fn new(app: Lightbox, ticker: bool, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let theme = app.settings().load().map(|s| s.theme).unwrap_or_default();
        apply_theme(theme, Some(window), cx);
        let mut vm = LibraryVm::new(&app);
        if let Err(e) = vm.refresh() {
            tracing::error!(error = %e, "initial query failed");
        }
        let sidebar = SidebarVm::new(&app).unwrap_or_else(|e| {
            tracing::error!(error = %e, "sidebar failed to load");
            SidebarVm::empty(&app)
        });
        let search = cx.new(|cx| InputState::new(window, cx).placeholder("Search photos"));
        let zoom = cx.new(|_| {
            SliderState::new()
                .min(ZOOM_RANGE.0 as f32)
                .max(ZOOM_RANGE.1 as f32)
                .step(8.0)
                .default_value(vm.zoom() as f32)
        });
        let panels = cx.new(|_| ResizableState::default());
        let focus = cx.focus_handle();
        let viewer_focus = cx.focus_handle();
        let mut subscriptions = Vec::new();
        subscriptions.push(cx.subscribe(&search, |this: &mut Self, input, event, cx| {
            let text = input.read(cx).value().to_string();
            match event {
                InputEvent::Change => this.vm.type_search(&text),
                InputEvent::PressEnter { .. } => this.vm.set_search(&text),
                _ => return,
            }
            this.pump(cx);
        }));
        subscriptions.push(cx.subscribe(&zoom, |this: &mut Self, _, event, cx| {
            if let SliderEvent::Change(value) = event {
                this.vm.set_zoom(value.start().round() as u32);
                cx.notify();
            }
        }));
        let slideshow_slider = crate::slideshow::duration_slider(3, cx);
        subscriptions.push(Self::slideshow_subscription(&slideshow_slider, cx));
        window.focus(&focus, cx);
        let events = app.events();
        let this = Self {
            events,
            app,
            vm,
            sidebar,
            focus,
            viewer_focus,
            scroll: VirtualListScrollHandle::new(),
            panels,
            search,
            zoom,
            overlay: Overlay::None,
            toast: None,
            thumbs: HashMap::new(),
            requested: HashSet::new(),
            scan: None,
            jobs: 0,
            info_open: false,
            opened: None,
            viewer: None,
            editor: None,
            viewport: TEST_WINDOW_SIZE,
            theme,
            slideshow: None,
            slideshow_slider,
            slideshow_fullscreen: false,
            slideshow_controls_shown: false,
            last_anchor_row: None,
            timeline_drag: None,
            band: None,
            grid_bounds: crate::band::GridBounds::default(),
            instance: None,
            _subscriptions: subscriptions,
        };
        if ticker {
            cx.spawn_in(window, async move |this, cx| {
                loop {
                    cx.background_executor().timer(PUMP_INTERVAL).await;
                    if this.update(cx, |this, cx| this.pump(cx)).is_err() {
                        break;
                    }
                }
            })
            .detach();
        }
        this
    }

    /// Opens the library window on `cx` and drains `instance` from the pump.
    /// [`gpui_kit::init`] must have run.
    pub fn open(
        app: Lightbox,
        instance: InstanceServer,
        cx: &mut App,
    ) -> Result<(AnyWindowHandle, Entity<Self>)> {
        gpui_kit::open_window(WindowOptions::default(), cx, move |window, cx| {
            cx.new(|cx| {
                let mut this = Self::new(app, true, window, cx);
                this.instance = Some(instance);
                this
            })
        })
    }

    /// Opens a library window that does not own the instance endpoint.
    pub fn open_library(app: Lightbox, cx: &mut App) -> Result<(AnyWindowHandle, Entity<Self>)> {
        gpui_kit::open_window(WindowOptions::default(), cx, move |window, cx| {
            cx.new(|cx| Self::new(app, true, window, cx))
        })
    }

    /// Opens a 1280x800 window whose events are pumped by the test (SPEC §12.5).
    pub fn open_for_test(app: Lightbox, cx: &mut App) -> (AnyWindowHandle, Entity<Self>) {
        let (w, h) = TEST_WINDOW_SIZE;
        let bounds = gpui_kit::Bounds {
            origin: Point::default(),
            size: size(px(w), px(h)),
        };
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            ..WindowOptions::default()
        };
        match gpui_kit::open_window(options, cx, move |window, cx| {
            cx.new(|cx| Self::new(app, false, window, cx))
        }) {
            Ok(opened) => opened,
            Err(e) => panic!("could not open the test window: {e}"),
        }
    }

    /// The grid's view-model.
    pub fn vm(&self) -> &LibraryVm {
        &self.vm
    }

    /// The sidebar's view-model.
    pub fn sidebar(&self) -> &SidebarVm {
        &self.sidebar
    }

    /// The open overlay.
    pub fn overlay(&self) -> &Overlay {
        &self.overlay
    }

    /// The chosen colour scheme.
    pub fn theme(&self) -> ThemeChoice {
        self.theme
    }

    /// Switches System, Light and Dark in turn and stores the choice.
    pub fn cycle_theme(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let next = match self.theme {
            ThemeChoice::System => ThemeChoice::Light,
            ThemeChoice::Light => ThemeChoice::Dark,
            ThemeChoice::Dark => ThemeChoice::System,
        };
        self.theme = next;
        apply_theme(next, Some(window), cx);
        let result = self.app.settings().set_theme(next);
        self.report(result, cx);
        cx.notify();
    }

    /// Which screen is showing.
    pub fn route(&self) -> Route {
        if self.slideshow.is_some() {
            Route::Slideshow
        } else if self.editor.is_some() {
            Route::Editor
        } else if self.viewer.is_some() {
            Route::Viewer
        } else {
            Route::Library
        }
    }

    /// The photo most recently opened in the viewer.
    pub fn opened(&self) -> Option<PhotoId> {
        self.opened
    }

    /// The text of the message toast, if one is showing.
    pub fn toast(&self) -> Option<&str> {
        self.toast.as_deref()
    }

    /// Photos the tray toolbar and menus act on.
    pub fn target_ids(&self) -> Vec<PhotoId> {
        self.vm.target_ids()
    }

    /// Drains application events and applies a due search. Called by the ticker and by tests.
    pub fn pump(&mut self, cx: &mut Context<Self>) {
        let mut changed = self.vm.tick();
        changed |= self.drain_instance(cx);
        changed |= self.tick_slideshow();
        let mut refresh_sidebar = false;
        let mut viewer_stale = false;
        let mut editor_stale = false;
        while let Ok(event) = self.events.try_recv() {
            changed |= self.vm.handle_event(&event);
            if let Some(v) = &mut self.viewer
                && !v.vm.handle_event(&event)
            {
                self.viewer = None;
                self.editor = None;
                changed = true;
            }
            match &event {
                AppEvent::ThumbReady { id, .. } => {
                    self.thumbs.retain(|(p, _), _| p != id);
                    self.requested.retain(|(p, _)| p != id);
                    changed = true;
                }
                AppEvent::PhotoEdited { id, .. } => {
                    self.thumbs.retain(|(p, _), _| p != id);
                    self.requested.retain(|(p, _)| p != id);
                    changed = true;
                    if self.viewer.as_ref().is_some_and(|v| v.vm.current() == *id) {
                        viewer_stale = true;
                    }
                    if self.editor.as_ref().is_some_and(|e| e.vm.photo() == *id) {
                        editor_stale = true;
                    }
                }
                AppEvent::ScanProgress {
                    done,
                    total_estimate,
                    ..
                } => {
                    self.scan = Some((*done, *total_estimate));
                    changed = true;
                }
                AppEvent::ScanFinished { .. } => {
                    self.scan = None;
                    refresh_sidebar = true;
                }
                AppEvent::JobProgress { .. } => {
                    self.jobs = self.jobs.max(1);
                    changed = true;
                }
                AppEvent::JobFinished { .. } => {
                    self.jobs = self.jobs.saturating_sub(1);
                    changed = true;
                }
                AppEvent::SettingsChanged { key } => {
                    if let Ok(settings) = self.app.settings().load() {
                        if key == "theme" && settings.theme != self.theme {
                            self.theme = settings.theme;
                            apply_theme(settings.theme, None, cx);
                        }
                        if key == "grid" || key == "library" {
                            self.vm.apply_settings(&settings);
                        }
                        changed = true;
                    }
                }
                AppEvent::AlbumsChanged
                | AppEvent::FoldersChanged
                | AppEvent::PhotosAdded { .. }
                | AppEvent::PhotosRemoved { .. } => refresh_sidebar = true,
                AppEvent::Error { context, message } => {
                    self.toast = Some(format!("{context}: {message}"));
                    changed = true;
                }
                _ => {}
            }
            if let Overlay::Export { vm, .. } = &mut self.overlay {
                vm.handle_event(&event);
                changed = true;
            }
        }
        if let Overlay::Export { vm, .. } = &mut self.overlay {
            vm.poll();
        }
        if viewer_stale && self.editor.is_none() {
            self.viewer_reload(cx);
        }
        if editor_stale {
            self.editor_changed(cx);
        }
        if refresh_sidebar {
            if let Err(e) = self.sidebar.refresh() {
                tracing::warn!(error = %e, "sidebar refresh failed");
            }
            changed = true;
        }
        if changed || self.viewer_needs_frame() {
            cx.notify();
        }
    }

    fn drain_instance(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(instance) = &self.instance else {
            return false;
        };
        let mut launches = Vec::new();
        while let Some(launch) = instance.poll() {
            launches.push(launch);
        }
        if launches.is_empty() {
            return false;
        }
        for launch in launches {
            if let Launch::View { paths, .. } = launch {
                self.open_external_paths(&paths, cx);
            }
        }
        true
    }

    /// The cached thumbnail for a grid cell at the current zoom, requesting it once if missing.
    pub(crate) fn thumb_for(&mut self, id: PhotoId) -> Option<PathBuf> {
        let size = thumb_size_for(self.vm.zoom());
        self.thumb_for_size(id, size)
    }

    /// The cached thumbnail of `id` at `size`, requesting it once if missing.
    pub(crate) fn thumb_for_size(&mut self, id: PhotoId, size: ThumbSize) -> Option<PathBuf> {
        if let Some(path) = self.thumbs.get(&(id, size)) {
            return Some(path.clone());
        }
        match self.app.thumbs().get_cached(id, size) {
            Ok(Some(path)) => {
                self.thumbs.insert((id, size), path.clone());
                Some(path)
            }
            Ok(None) => {
                if self.requested.insert((id, size)) {
                    drop(self.app.thumbs().request_visible(id, size));
                }
                None
            }
            Err(e) => {
                tracing::debug!(photo = %id, error = %e, "no thumbnail");
                None
            }
        }
    }

    // ---- intents ----

    /// Shows `message` in the toast.
    pub fn notify_message(&mut self, message: impl Into<String>, cx: &mut Context<Self>) {
        self.toast = Some(message.into());
        cx.notify();
    }

    pub(crate) fn report<T>(
        &mut self,
        result: lb_app::Result<T>,
        cx: &mut Context<Self>,
    ) -> Option<T> {
        match result {
            Ok(v) => Some(v),
            Err(e) => {
                self.toast = Some(e.user_message());
                cx.notify();
                None
            }
        }
    }

    /// Selects a sidebar row and shows its source in the grid.
    pub fn select_sidebar(&mut self, key: SidebarKey, cx: &mut Context<Self>) {
        if let Some(source) = self.sidebar.select(key) {
            self.vm.set_source(source);
            self.scroll.scroll_to_item(0, ScrollStrategy::Top);
            cx.notify();
        }
    }

    /// Creates an album and shows it.
    pub fn new_album(&mut self, cx: &mut Context<Self>) -> Option<AlbumId> {
        let created = self.sidebar.create_album();
        let (album, source) = self.report(created, cx)?;
        self.vm.set_source(source);
        cx.notify();
        Some(album.id)
    }

    /// A click on a grid cell.
    pub fn click_cell(&mut self, id: PhotoId, mods: Modifiers, cx: &mut Context<Self>) {
        self.vm.click(id, mods);
        cx.notify();
    }

    /// Moves keyboard focus in the grid and scrolls the new cell into view.
    pub fn move_focus(&mut self, key: NavKey, extend: bool, cx: &mut Context<Self>) {
        self.vm.move_focus(key, extend);
        self.reveal_focused();
        cx.notify();
    }

    pub(crate) fn reveal_focused(&mut self) {
        let Some(id) = self.vm.focused() else { return };
        let Some((group, index)) = self.vm.position_of(id) else {
            return;
        };
        if let Some(row) = self.vm.layout().row_of(group, index) {
            self.scroll.scroll_to_item(row, ScrollStrategy::Nearest);
        }
    }

    /// Toggles the star of the target photos (tray first, then selection).
    pub fn toggle_star_targets(&mut self, cx: &mut Context<Self>) {
        let ids = self.vm.target_ids();
        self.vm.toggle_star_on(&ids);
        cx.notify();
    }

    /// Rotates the target photos by `turns` quarter turns.
    pub fn rotate_targets(&mut self, turns: i8, cx: &mut Context<Self>) {
        let ids = self.vm.target_ids();
        self.vm.rotate_on(&ids, turns);
        cx.notify();
    }

    /// Sets the rating of the target photos.
    pub fn rate_targets(&mut self, rating: u8, cx: &mut Context<Self>) {
        let ids = self.vm.target_ids();
        self.vm.rate_on(&ids, rating);
        cx.notify();
    }

    /// Undoes the last organize action (SPEC §7.4).
    pub fn undo_organize(&mut self, cx: &mut Context<Self>) {
        let result = self.app.history().undo();
        self.report(result, cx);
        self.pump(cx);
    }

    /// Re-runs the query and rebuilds the sidebar.
    pub fn refresh_all(&mut self, cx: &mut Context<Self>) {
        let result = self.vm.refresh();
        self.report(result, cx);
        let result = self.sidebar.refresh();
        self.report(result, cx);
        cx.notify();
    }

    /// The grid width implied by the panel layout.
    pub(crate) fn grid_width(&self, window: &Window, cx: &App) -> f32 {
        let sizes = self.panels.read(cx).sizes();
        match sizes.get(1) {
            Some(w) if f32::from(*w) > 1.0 => f32::from(*w),
            _ => {
                let total = f32::from(window.viewport_size().width);
                let info = if self.info_open { INFO_WIDTH } else { 0.0 };
                (total - SIDEBAR_WIDTH - info).max(1.0)
            }
        }
    }

    /// Keeps the scroll anchor on the first visible photo so re-layouts keep the position.
    pub(crate) fn track_scroll_anchor(&mut self) {
        let offset = f32::from(self.scroll.offset().y);
        let top = (-offset).max(0.0);
        let row = self.vm.layout().visible_rows(top, 1.0, 0.0).start;
        if self.last_anchor_row != Some(row) {
            self.last_anchor_row = Some(row);
            if let Some(id) = self.vm.photo_at_scroll(top) {
                self.vm.set_scroll_anchor(Some(id));
            }
        }
    }

    /// Closes any overlay and returns keyboard focus to the grid.
    pub fn close_overlay(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.overlay = Overlay::None;
        window.focus(&self.focus, cx);
        cx.notify();
    }

    /// Whether the search box has keyboard focus.
    pub(crate) fn search_focused(&self, window: &Window, cx: &App) -> bool {
        self.search.focus_handle(cx).is_focused(window)
    }
}

impl Focusable for LightboxWindow {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus.clone()
    }
}

/// Applies a colour scheme to the whole application.
pub(crate) fn apply_theme(choice: ThemeChoice, window: Option<&mut Window>, cx: &mut App) {
    use gpui_kit::component::{Theme, ThemeMode};
    match choice {
        ThemeChoice::System => Theme::sync_system_appearance(window, cx),
        ThemeChoice::Light => Theme::change(ThemeMode::Light, window, cx),
        ThemeChoice::Dark => Theme::change(ThemeMode::Dark, window, cx),
    }
}

/// The thumbnail size that stays crisp at a cell size of `zoom` pixels.
pub fn thumb_size_for(zoom: u32) -> ThumbSize {
    match zoom {
        0..=160 => ThumbSize::S,
        161..=320 => ThumbSize::M,
        _ => ThumbSize::L,
    }
}

/// Element ids of the sidebar rows that are not photos or albums, for tests.
pub fn sidebar_id(key: SidebarKey) -> gpui_kit::ElementId {
    match key {
        SidebarKey::All => ids::SIDEBAR_ALL.into(),
        SidebarKey::Starred => ids::SIDEBAR_STARRED.into(),
        SidebarKey::Recent => ids::SIDEBAR_RECENT.into(),
        SidebarKey::Album(id) => ids::album_item(id),
        SidebarKey::Folder(id) => ids::folder_item(id),
        SidebarKey::Person(id) => ids::person_item(id),
        SidebarKey::Unnamed => ids::SIDEBAR_UNNAMED.into(),
        SidebarKey::Year(y) => {
            gpui_kit::ElementId::NamedInteger("year-heading".into(), y.map_or(0, |y| y as u64))
        }
    }
}
