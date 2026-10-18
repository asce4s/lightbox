//! Headless timings for `lightbox view --perf-json` (SPEC-VIEWER §V12).
//!
//! The report is wall-clock time from `origin` (the process start). It does not open a window:
//! the first frame is the motion sample, and the image times are decoder work.

use std::path::Path;
use std::time::Instant;

use lb_media::decode;
use serde::Serialize;

use crate::error::{AppError, Result};
use crate::lightbox::Lightbox;
use crate::viewer::{fit_area, fit_rect, stage_insets};

/// Long edge of the on-screen proxy. Matches the viewer's fit proxy.
const SCREEN_PROXY_EDGE: u32 = 2560;

/// Milestones from process start, in milliseconds.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct ViewerPerf {
    /// Library is open and viewer work is about to start.
    pub process_start_ms: u128,
    /// First backdrop frame (motion sample, no decode).
    pub first_frame_ms: u128,
    /// First decoded pixels (the screen proxy when the file has no earlier preview).
    pub first_image_ms: u128,
    /// Screen proxy decoded.
    pub l2_ms: u128,
}

impl ViewerPerf {
    /// One JSON object with the four §V12 fields.
    #[must_use]
    pub fn to_json(self) -> String {
        match serde_json::to_string(&self) {
            Ok(json) => json,
            Err(error) => {
                tracing::warn!(%error, "viewer perf report could not be encoded");
                "{}".to_owned()
            }
        }
    }
}

/// Measures one file. `origin` is the instant the process started.
pub fn capture_viewer_perf(lb: &Lightbox, path: &Path, origin: Instant) -> Result<ViewerPerf> {
    let process_start_ms = elapsed_ms(origin);
    let mut vm = crate::vm::ViewerVm::open_paths(lb, &[path])?;
    let stage = (1920.0_f32, 1080.0);
    vm.set_stage(stage);
    let area = fit_area(stage, stage_insets(stage, true, false));
    let fit = fit_rect((3000.0, 2000.0), area, false);
    vm.begin_open(lb.clock().elapsed(), None, fit);
    let _frame = vm.presented(lb.clock().elapsed());
    let first_frame_ms = elapsed_ms(origin);

    let image = decode(path, Some(SCREEN_PROXY_EDGE)).map_err(|error| {
        AppError::Invalid(format!("could not decode {}: {error}", path.display()))
    })?;
    let l2_ms = elapsed_ms(origin);
    let _ = (image.width(), image.height());
    Ok(ViewerPerf {
        process_start_ms,
        first_frame_ms,
        first_image_ms: l2_ms,
        l2_ms,
    })
}

fn elapsed_ms(origin: Instant) -> u128 {
    origin.elapsed().as_millis()
}
