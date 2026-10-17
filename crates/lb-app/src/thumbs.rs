//! Thumbnail requests with the current edit stack applied (SPEC §6.4).

use crate::error::{AppError, Result};
use crate::events::AppEvent;
use crate::lightbox::Lightbox;
use image::DynamicImage;
use lb_core::{EditStack, PhotoId};
use lb_imaging::convert::to_rgba8;
use lb_imaging::{RenderCtx, RenderTarget, render};
use lb_jobs::{Job, JobCtx, JobError, JobHandle, JobResult, Priority};
use lb_thumbs::{EditFn, ThumbRequest, ThumbSize};
use std::path::PathBuf;
use std::sync::Arc;

/// Produces and looks up cached thumbnails.
#[derive(Clone)]
pub struct ThumbService {
    lb: Lightbox,
}

impl Lightbox {
    /// Thumbnail generation and lookup.
    pub fn thumbs(&self) -> ThumbService {
        ThumbService { lb: self.clone() }
    }
}

/// Handle to a queued thumbnail render.
pub struct ThumbTicket {
    handle: JobHandle,
}

impl ThumbTicket {
    /// Asks the queue to drop the request if it has not started.
    pub fn cancel(&self) {
        self.handle.cancel();
    }

    /// Whether the render ended (successfully or not).
    pub fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }

    /// Waits up to `timeout` for the render and returns its outcome.
    pub fn wait(&self, timeout: std::time::Duration) -> Option<JobResult> {
        self.handle.wait(timeout)
    }
}

struct Resolved {
    path: PathBuf,
    quick_key: String,
    edit_hash: String,
    edit_fn: Option<Arc<EditFn>>,
}

fn edit_fn(stack: EditStack, quick_key: String) -> Arc<EditFn> {
    Arc::new(move |img: DynamicImage| {
        let ctx = RenderCtx::new(&quick_key, RenderTarget::Full);
        DynamicImage::ImageRgba8(to_rgba8(&render(&img, &stack, &ctx)))
    })
}

impl ThumbService {
    fn resolve(&self, id: PhotoId) -> Result<Resolved> {
        let photo = self
            .lb
            .catalog()
            .get_photo(id)?
            .ok_or(AppError::NotFound("photo"))?;
        let stack = self.lb.edits().stack(id)?;
        let edit_hash = stack.cache_hash();
        let edit_fn = (!stack.is_empty()).then(|| edit_fn(stack, photo.quick_key.clone()));
        Ok(Resolved {
            path: photo.path,
            quick_key: photo.quick_key,
            edit_hash,
            edit_fn,
        })
    }

    fn request<'a>(r: &'a Resolved, size: ThumbSize) -> ThumbRequest<'a> {
        let req = ThumbRequest::new(&r.path, &r.quick_key, size);
        match &r.edit_fn {
            Some(f) => req.with_edits(&r.edit_hash, f.as_ref()),
            None => req,
        }
    }

    /// The cached thumbnail file for `id` at `size`, if it already exists for the current edits.
    pub fn get_cached(&self, id: PhotoId, size: ThumbSize) -> Result<Option<PathBuf>> {
        let r = self.resolve(id)?;
        Ok(self
            .lb
            .inner
            .thumbs
            .lookup(&Self::request(&r, size))
            .map(|t| t.path))
    }

    /// The cache key `id` currently maps to at `size` (changes when the edit stack changes).
    pub fn cache_key(&self, id: PhotoId, size: ThumbSize) -> Result<String> {
        let r = self.resolve(id)?;
        Ok(Self::request(&r, size).key())
    }

    /// Renders (or fetches) the thumbnail synchronously and returns its file path.
    pub fn generate(&self, id: PhotoId, size: ThumbSize) -> Result<PathBuf> {
        let r = self.resolve(id)?;
        let thumb = self
            .lb
            .inner
            .thumbs
            .get_or_create(&Self::request(&r, size))?;
        Ok(thumb.path)
    }

    /// Queues a render at `priority`; emits [`AppEvent::ThumbReady`] when the file exists.
    /// Identical pending requests share one job.
    pub fn request_thumb(&self, id: PhotoId, size: ThumbSize, priority: Priority) -> ThumbTicket {
        let version = self
            .lb
            .catalog()
            .get_photo(id)
            .ok()
            .flatten()
            .map_or(0, |p| p.edit_version);
        let job = ThumbJob {
            lb: self.lb.clone(),
            id,
            size,
            version,
        };
        ThumbTicket {
            handle: self.lb.jobs().submit(job, priority),
        }
    }

    /// Queues Background renders of `size` for every photo that is not cached yet.
    pub fn warm(&self, size: ThumbSize) -> Result<Vec<ThumbTicket>> {
        let mut tickets = Vec::new();
        for id in self.lb.catalog().all_photo_ids()? {
            if self.get_cached(id, size)?.is_none() {
                tickets.push(self.request_thumb(id, size, Priority::Background));
            }
        }
        Ok(tickets)
    }
}

struct ThumbJob {
    lb: Lightbox,
    id: PhotoId,
    size: ThumbSize,
    version: u32,
}

impl Job for ThumbJob {
    fn kind(&self) -> &'static str {
        "thumbnail"
    }

    fn coalesce_key(&self) -> Option<String> {
        Some(format!(
            "thumb:{}:{:?}:{}",
            self.id, self.size, self.version
        ))
    }

    fn run(self: Box<Self>, ctx: &JobCtx) -> JobResult {
        if ctx.is_cancelled() {
            return Err(JobError::Cancelled);
        }
        let service = self.lb.thumbs();
        match service.generate(self.id, self.size) {
            Ok(_) => {
                let version = self
                    .lb
                    .catalog()
                    .get_photo(self.id)
                    .ok()
                    .flatten()
                    .map_or(self.version, |p| p.edit_version);
                self.lb.emit(AppEvent::ThumbReady {
                    id: self.id,
                    size: self.size,
                    version,
                });
                Ok(())
            }
            Err(e) => Err(JobError::Failed(e.to_string())),
        }
    }
}
