//! State of the red-eye tool: rectangles dragged over the displayed image (SPEC §9.5).

use lb_core::{EditOp, NormRect};

/// Smallest rectangle edge, as a fraction of the image, that counts as an eye.
const MIN_EDGE: f32 = 0.005;

/// Where the image is drawn on screen, in pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DisplayRect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl DisplayRect {
    /// The rectangle that shows an `image_w` x `image_h` image fitted and centred in `area`.
    pub fn fit(image_w: f32, image_h: f32, area_w: f32, area_h: f32) -> Self {
        let scale = (area_w / image_w.max(1.0)).min(area_h / image_h.max(1.0));
        let (w, h) = (image_w * scale, image_h * scale);
        Self {
            x: (area_w - w) / 2.0,
            y: (area_h - h) / 2.0,
            w,
            h,
        }
    }

    /// A screen point as normalized image coordinates, clamped to the image.
    pub fn normalize(&self, px: f32, py: f32) -> (f32, f32) {
        (
            ((px - self.x) / self.w.max(1.0)).clamp(0.0, 1.0),
            ((py - self.y) / self.h.max(1.0)).clamp(0.0, 1.0),
        )
    }
}

/// The red-eye tool's view-model.
#[derive(Debug, Clone, Default)]
pub struct RedEyeVm {
    regions: Vec<NormRect>,
    dragging: Option<(f32, f32)>,
    current: Option<NormRect>,
}

impl RedEyeVm {
    /// An empty tool.
    pub fn new() -> Self {
        Self::default()
    }

    /// Starts a rectangle at a screen point.
    pub fn begin(&mut self, display: DisplayRect, px: f32, py: f32) {
        self.dragging = Some(display.normalize(px, py));
        self.current = None;
    }

    /// Updates the rectangle being dragged.
    pub fn update(&mut self, display: DisplayRect, px: f32, py: f32) {
        let Some((x0, y0)) = self.dragging else {
            return;
        };
        let (x1, y1) = display.normalize(px, py);
        self.current = Some(NormRect {
            x: x0.min(x1),
            y: y0.min(y1),
            w: (x1 - x0).abs(),
            h: (y1 - y0).abs(),
        });
    }

    /// Finishes the drag, keeping the rectangle when it is big enough. Returns whether one was
    /// added.
    pub fn end(&mut self) -> bool {
        self.dragging = None;
        match self.current.take() {
            Some(r) if r.w >= MIN_EDGE && r.h >= MIN_EDGE => {
                self.regions.push(r);
                true
            }
            _ => false,
        }
    }

    /// Adds the rectangle spanned by two screen points in one step.
    pub fn add_drag(&mut self, display: DisplayRect, from: (f32, f32), to: (f32, f32)) -> bool {
        self.begin(display, from.0, from.1);
        self.update(display, to.0, to.1);
        self.end()
    }

    /// Rectangles committed so far, normalized to the image.
    pub fn regions(&self) -> &[NormRect] {
        &self.regions
    }

    /// The rectangle being dragged.
    pub fn in_progress(&self) -> Option<NormRect> {
        self.current
    }

    /// Removes the last rectangle.
    pub fn remove_last(&mut self) {
        self.regions.pop();
    }

    /// Removes all rectangles.
    pub fn clear(&mut self) {
        self.regions.clear();
        self.current = None;
        self.dragging = None;
    }

    /// The op to commit, `None` without rectangles.
    pub fn op(&self) -> Option<EditOp> {
        (!self.regions.is_empty()).then(|| EditOp::RedEye {
            regions: self.regions.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vm_redeye_region_normalized() {
        let display = DisplayRect::fit(4000.0, 3000.0, 800.0, 800.0);
        assert!((display.w - 800.0).abs() < 1e-3);
        assert!((display.h - 600.0).abs() < 1e-3);
        assert!((display.y - 100.0).abs() < 1e-3);

        let mut vm = RedEyeVm::new();
        assert!(vm.op().is_none());
        assert!(vm.add_drag(display, (400.0, 400.0), (480.0, 460.0)));
        let r = vm.regions()[0];
        assert!((r.x - 0.5).abs() < 1e-4, "{r:?}");
        assert!((r.y - 0.5).abs() < 1e-4, "{r:?}");
        assert!((r.w - 0.1).abs() < 1e-4, "{r:?}");
        assert!((r.h - 0.1).abs() < 1e-4, "{r:?}");

        assert!(vm.add_drag(display, (50.0, 50.0), (-100.0, 2000.0)));
        let clamped = vm.regions()[1];
        assert!(clamped.x >= 0.0 && clamped.y >= 0.0);
        assert!(clamped.x + clamped.w <= 1.0 && clamped.y + clamped.h <= 1.0);

        assert!(!vm.add_drag(display, (10.0, 10.0), (10.0, 10.0)));
        assert_eq!(vm.regions().len(), 2);
        assert!(matches!(vm.op(), Some(EditOp::RedEye { regions }) if regions.len() == 2));
        vm.remove_last();
        vm.clear();
        assert!(vm.op().is_none());
    }
}
