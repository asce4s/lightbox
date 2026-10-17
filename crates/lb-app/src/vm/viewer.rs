//! State of the single-photo viewer (SPEC §9.4).

use crate::error::Result;
use crate::events::AppEvent;
use crate::lightbox::Lightbox;
use lb_core::{Photo, PhotoId};

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
    zoom: ZoomMode,
    pan: (f32, f32),
    show_info: bool,
}

impl ViewerVm {
    /// Opens `start` within `ids` (the current view). `None` when `start` is not in the list.
    pub fn open(lb: &Lightbox, ids: Vec<PhotoId>, start: PhotoId) -> Option<Self> {
        let index = ids.iter().position(|i| *i == start)?;
        Some(Self {
            lb: lb.clone(),
            ids,
            index,
            zoom: ZoomMode::Fit,
            pan: (0.0, 0.0),
            show_info: false,
        })
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
        if index == self.index || index >= self.ids.len() {
            return false;
        }
        self.index = index;
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
            let current = self.current();
            self.ids.retain(|i| !ids.contains(i));
            if self.ids.is_empty() {
                return false;
            }
            self.index = self
                .ids
                .iter()
                .position(|i| *i == current)
                .unwrap_or_else(|| self.index.min(self.ids.len() - 1));
        }
        true
    }
}
