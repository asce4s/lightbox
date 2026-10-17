//! State of the single-photo viewer (SPEC §9.4, SPEC-VIEWER §V4).

use crate::error::Result;
use crate::events::AppEvent;
use crate::lightbox::Lightbox;
use crate::viewer::{
    FolderRefresh, FolderWatch, Motion, MotionMode, NavSet, Presented, RectF, ViewerEntry,
    entry_error, entry_from_id,
};
use lb_core::{Photo, PhotoId};
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Smallest and largest zoom factor.
pub const ZOOM_LIMITS: (f32, f32) = (0.05, 16.0);
const ZOOM_STEP: f32 = 1.25;

/// How the photo is scaled in the viewer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ZoomMode {
    /// Fit the window.
    Fit,
    /// One image pixel per screen pixel (`1`).
    Actual,
    /// An explicit factor from `+`/`-` or the wheel.
    Factor(f32),
}

impl ZoomMode {
    /// The scale given the fit scale of the current window.
    pub fn scale(self, fit: f32) -> f32 {
        match self {
            Self::Fit => fit,
            Self::Actual => 1.0,
            Self::Factor(f) => f,
        }
    }
}

/// The viewer's view-model: where in the list, zoom, pan and panels.
pub struct ViewerVm {
    lb: Lightbox,
    ids: Vec<PhotoId>,
    index: usize,
    nav: NavSet,
    watch: Option<FolderWatch>,
    zoom: ZoomMode,
    pan: (f32, f32),
    show_info: bool,
    motion: Motion,
}

impl ViewerVm {
    /// Opens `start` within `ids` (the current view). `None` when `start` is not in the list.
    pub fn open(lb: &Lightbox, ids: Vec<PhotoId>, start: PhotoId) -> Option<Self> {
        let index = ids.iter().position(|i| *i == start)?;
        let entries: Vec<_> = ids.iter().map(|id| entry_from_id(lb, *id)).collect();
        let nav = NavSet::library(entries, index, ids);
        let mut vm = Self {
            lb: lb.clone(),
            ids: Vec::new(),
            index: 0,
            nav,
            watch: None,
            zoom: ZoomMode::Fit,
            pan: (0.0, 0.0),
            show_info: false,
            motion: Motion::new(MotionMode::Full, (0.0, 0.0)),
        };
        vm.sync();
        Some(vm)
    }

    /// Opens files or one folder from outside the grid (SPEC-VIEWER §V2.2).
    pub fn open_paths(lb: &Lightbox, paths: &[impl AsRef<Path>]) -> Result<Self> {
        let paths: Vec<PathBuf> = paths.iter().map(|p| p.as_ref().to_path_buf()).collect();
        let nav = NavSet::open_paths(lb, &paths)?;
        let watch = nav
            .source_dir()
            .and_then(|dir| FolderWatch::start(dir).ok());
        let mut vm = Self {
            lb: lb.clone(),
            ids: Vec::new(),
            index: 0,
            nav,
            watch,
            zoom: ZoomMode::Fit,
            pan: (0.0, 0.0),
            show_info: false,
            motion: Motion::new(MotionMode::Full, (0.0, 0.0)),
        };
        vm.sync();
        Ok(vm)
    }

    /// The entry on screen.
    #[must_use]
    pub fn current_entry(&self) -> &ViewerEntry {
        self.nav.current()
    }

    /// Every entry in navigation order.
    #[must_use]
    pub fn entries(&self) -> &[ViewerEntry] {
        self.nav.entries()
    }

    /// Catalog id of the current entry, when it is an indexed readable photo.
    #[must_use]
    pub fn current_photo(&self) -> Option<PhotoId> {
        self.nav.current().photo
    }

    /// One-line reason the current path cannot be shown.
    #[must_use]
    pub fn current_error(&self) -> Option<&'static str> {
        entry_error(&self.current_entry().path)
    }

    /// Re-reads the folder after a create or delete. Library lists stay as they are.
    pub fn refresh(&mut self) -> Result<FolderRefresh> {
        let change = self.nav.refresh(&self.lb)?;
        self.sync();
        Ok(change)
    }

    /// Applies a directory-watch event, if one is waiting.
    pub fn poll_folder(&mut self) -> Result<FolderRefresh> {
        let pending = self.watch.as_ref().is_some_and(FolderWatch::poll);
        if pending {
            self.refresh()
        } else {
            Ok(FolderRefresh::Kept)
        }
    }

    fn sync(&mut self) {
        self.ids = self
            .nav
            .entries()
            .iter()
            .map(|e| e.photo.unwrap_or(PhotoId::new(0)))
            .collect();
        self.index = self.nav.index();
    }

    /// The frame at `now` (`Clock::elapsed()`). Geometry is sampled, not animated by GPUI.
    pub fn presented(&self, now: Duration) -> Presented {
        self.motion.presented(now)
    }

    /// Whether the view should ask for another frame.
    pub fn is_animating(&self, now: Duration) -> bool {
        self.motion.is_animating(now)
    }

    /// Motion state. The UI samples [`Self::presented`] from this.
    pub fn motion_mut(&mut self) -> &mut Motion {
        &mut self.motion
    }

    /// Remembers the stage size used by navigate slides.
    pub fn set_stage(&mut self, stage: (f32, f32)) {
        self.motion.set_stage(stage);
    }

    /// Starts the overlay open from a thumbnail rect.
    pub fn begin_open(&mut self, now: Duration, source: Option<RectF>, fit: RectF) {
        match source {
            Some(source) => self.motion.open_from_thumbnail(now, source, fit),
            None => self.motion.open_plain(now, fit),
        }
    }

    /// The photo being shown.
    pub fn current(&self) -> PhotoId {
        self.ids[self.index]
    }

    /// Position in the filmstrip.
    pub fn index(&self) -> usize {
        self.index
    }

    /// The filmstrip.
    pub fn filmstrip(&self) -> &[PhotoId] {
        &self.ids
    }

    /// Whether [`Self::go_next`] would move.
    pub fn has_next(&self) -> bool {
        self.index + 1 < self.ids.len()
    }

    /// Whether [`Self::go_prev`] would move.
    pub fn has_prev(&self) -> bool {
        self.index > 0
    }

    fn go(&mut self, index: usize) -> bool {
        if !self.nav.go(index) {
            return false;
        }
        self.sync();
        self.zoom = ZoomMode::Fit;
        self.pan = (0.0, 0.0);
        true
    }

    /// Right arrow.
    pub fn go_next(&mut self) -> bool {
        self.go(self.index + 1)
    }

    /// Left arrow.
    pub fn go_prev(&mut self) -> bool {
        self.index.checked_sub(1).is_some_and(|i| self.go(i))
    }

    /// Home.
    pub fn home(&mut self) -> bool {
        self.go(0)
    }

    /// End.
    pub fn end(&mut self) -> bool {
        self.go(self.ids.len() - 1)
    }

    /// Jumps to `id` (a click in the filmstrip).
    pub fn goto(&mut self, id: PhotoId) -> bool {
        self.ids
            .iter()
            .position(|i| *i == id)
            .is_some_and(|i| self.go(i))
    }

    /// Current zoom mode.
    pub fn zoom(&self) -> ZoomMode {
        self.zoom
    }

    /// Pan offset in screen pixels.
    pub fn pan(&self) -> (f32, f32) {
        self.pan
    }

    /// Drags the image by `(dx, dy)`.
    pub fn pan_by(&mut self, dx: f32, dy: f32) {
        self.pan = (self.pan.0 + dx, self.pan.1 + dy);
    }

    fn factor(&self, fit: f32) -> f32 {
        self.zoom.scale(fit)
    }

    /// `+` or wheel up, relative to the fit scale of the window.
    pub fn zoom_in(&mut self, fit: f32) {
        let f = (self.factor(fit) * ZOOM_STEP).clamp(ZOOM_LIMITS.0, ZOOM_LIMITS.1);
        self.zoom = ZoomMode::Factor(f);
    }

    /// `-` or wheel down.
    pub fn zoom_out(&mut self, fit: f32) {
        let f = (self.factor(fit) / ZOOM_STEP).clamp(ZOOM_LIMITS.0, ZOOM_LIMITS.1);
        self.zoom = ZoomMode::Factor(f);
        if f <= fit {
            self.pan = (0.0, 0.0);
        }
    }

    /// Double-click: fit becomes 100% and back.
    pub fn toggle_fit_actual(&mut self) {
        self.zoom = match self.zoom {
            ZoomMode::Fit => ZoomMode::Actual,
            _ => ZoomMode::Fit,
        };
        self.pan = (0.0, 0.0);
    }

    /// `I`.
    pub fn toggle_info(&mut self) {
        self.show_info = !self.show_info;
    }

    /// Whether the info panel is open.
    pub fn info_visible(&self) -> bool {
        self.show_info
    }

    /// The current photo's row.
    pub fn photo(&self) -> Result<Option<Photo>> {
        self.lb.photos().get(self.current())
    }

    /// Esc: the photo the library grid should focus.
    pub fn close(self) -> PhotoId {
        self.current()
    }

    /// Drops removed photos from the filmstrip. Returns `false` when none remain, so the caller
    /// closes the viewer.
    pub fn handle_event(&mut self, event: &AppEvent) -> bool {
        if let AppEvent::PhotosRemoved { ids } = event {
            let current = self.nav.current().path.clone();
            self.nav.retain_photos(ids);
            if self.nav.entries().is_empty() {
                self.sync();
                return false;
            }
            let index = self
                .nav
                .entries()
                .iter()
                .position(|e| e.path == current)
                .unwrap_or_else(|| self.nav.index().min(self.nav.entries().len() - 1));
            self.nav.set_index(index);
            self.sync();
        }
        true
    }
}
