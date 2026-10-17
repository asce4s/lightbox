//! Background preview rendering shared by the viewer and the editor (SPEC §9.4, §9.5).

use crate::window::LightboxWindow;
use gpui_kit::Context;
use lb_app::PreviewFile;
use lb_core::{EditStack, PhotoId};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

/// Which view asked for a render.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub(crate) enum StageKind {
    Viewer,
    Editor,
    Tile { effect: String, base: String },
}

/// The newest rendered frame and the frame being rendered. The shown frame is kept until a newer
/// one arrives, so the picture never goes blank while a slider moves.
pub(crate) struct Stage {
    shown: Option<(String, PreviewFile)>,
    requested: Option<String>,
    cancel: Arc<AtomicBool>,
}

impl Stage {
    pub(crate) fn new() -> Self {
        Self {
            shown: None,
            requested: None,
            cancel: Arc::new(AtomicBool::new(false)),
        }
    }

    /// Asks for the frame named `key`. Returns the cancel flag of the new render, or `None` when
    /// that frame is already shown or on its way.
    pub(crate) fn want(&mut self, key: &str) -> Option<Arc<AtomicBool>> {
        if self.requested.as_deref() == Some(key) {
            return None;
        }
        self.cancel.store(true, Ordering::Relaxed);
        self.cancel = Arc::new(AtomicBool::new(false));
        self.requested = Some(key.to_owned());
        Some(self.cancel.clone())
    }

    /// Stores a finished render; stale ones are dropped. Returns whether it is now shown.
    pub(crate) fn done(&mut self, key: &str, file: PreviewFile) -> bool {
        if self.requested.as_deref() != Some(key) {
            return false;
        }
        self.shown = Some((key.to_owned(), file));
        true
    }

    /// Forgets a failed request so it can be retried.
    pub(crate) fn failed(&mut self, key: &str) {
        if self.requested.as_deref() == Some(key) {
            self.requested = None;
        }
    }

    /// The frame to draw, whatever it is for.
    pub(crate) fn shown(&self) -> Option<&PreviewFile> {
        self.shown.as_ref().map(|(_, f)| f)
    }

    /// The shown frame if its key satisfies `pred`.
    pub(crate) fn shown_if(&self, pred: impl Fn(&str) -> bool) -> Option<&PreviewFile> {
        match &self.shown {
            Some((k, f)) if pred(k) => Some(f),
            _ => None,
        }
    }

    /// The frame to draw only if it is the one asked for last.
    pub(crate) fn current(&self) -> Option<&PreviewFile> {
        match (&self.shown, &self.requested) {
            (Some((k, f)), Some(r)) if k == r => Some(f),
            _ => None,
        }
    }

    /// Whether the asked-for frame has not arrived yet.
    pub(crate) fn busy(&self) -> bool {
        self.requested.is_some() && self.current().is_none()
    }

    /// Stops any render in flight.
    pub(crate) fn cancel(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
        self.requested = None;
    }
}

/// The identity of a rendered frame.
pub(crate) fn frame_key(id: PhotoId, stack: &EditStack, max_edge: u32) -> String {
    format!("{}:{}:{max_edge}", id.get(), stack.hash())
}

/// One render to run in the background.
pub(crate) struct RenderJob {
    pub(crate) kind: StageKind,
    pub(crate) key: String,
    pub(crate) id: PhotoId,
    pub(crate) stack: EditStack,
    pub(crate) max_edge: u32,
    pub(crate) cancel: Arc<AtomicBool>,
}

impl LightboxWindow {
    /// Renders the job's stack in the background and delivers it to the view named by its kind.
    pub(crate) fn spawn_render(&mut self, job: RenderJob, cx: &mut Context<Self>) {
        let RenderJob {
            kind,
            key,
            id,
            stack,
            max_edge,
            cancel,
        } = job;
        let app = self.app.clone();
        cx.spawn(async move |this, cx| {
            let result = cx
                .background_executor()
                .spawn(async move { app.edits().preview_file(id, &stack, max_edge, &cancel) })
                .await;
            this.update(cx, |this, cx| {
                this.finish_render(kind, &key, result, cx);
            })
            .ok();
        })
        .detach();
    }

    fn finish_render(
        &mut self,
        kind: StageKind,
        key: &str,
        result: lb_app::Result<PreviewFile>,
        cx: &mut Context<Self>,
    ) {
        match (&kind, result) {
            (StageKind::Viewer, Ok(file)) => {
                if let Some(v) = &mut self.viewer {
                    v.stage.done(key, file);
                }
            }
            (StageKind::Editor, Ok(file)) => {
                if let Some(e) = &mut self.editor {
                    e.stage.done(key, file);
                }
            }
            (StageKind::Tile { effect, base }, Ok(file)) => {
                if let Some(e) = &mut self.editor
                    && &e.tiles_base == base
                {
                    e.tiles.insert(effect.clone(), file);
                }
            }
            (_, Err(lb_app::AppError::Cancelled)) => {}
            (kind, Err(e)) => {
                tracing::warn!(error = %e, "preview failed");
                match kind {
                    StageKind::Viewer => {
                        if let Some(v) = &mut self.viewer {
                            v.stage.failed(key);
                        }
                    }
                    StageKind::Editor => {
                        if let Some(ed) = &mut self.editor {
                            ed.stage.failed(key);
                        }
                    }
                    StageKind::Tile { .. } => {}
                }
            }
        }
        cx.notify();
    }
}
