//! State of the crop tool: aspect presets, handle dragging and keyboard nudging (SPEC §9.5).

use lb_core::{EditOp, NormRect};

/// Smallest crop edge as a fraction of the image.
const MIN_EDGE: f32 = 0.02;

/// The aspect-ratio presets of the crop tool.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AspectPreset {
    /// No constraint.
    Free,
    /// The ratio of the image being cropped.
    Original,
    /// 1:1.
    Square,
    /// 4:3.
    R4x3,
    /// 3:2.
    R3x2,
    /// 16:9.
    R16x9,
    /// 5:7.
    R5x7,
    /// 8:10.
    R8x10,
}

impl AspectPreset {
    /// Every preset in the order the tool lists them.
    pub const ALL: [Self; 8] = [
        Self::Free,
        Self::Original,
        Self::Square,
        Self::R4x3,
        Self::R3x2,
        Self::R16x9,
        Self::R5x7,
        Self::R8x10,
    ];

    /// Button label.
    pub fn label(self) -> &'static str {
        match self {
            Self::Free => "Free",
            Self::Original => "Original",
            Self::Square => "1:1",
            Self::R4x3 => "4:3",
            Self::R3x2 => "3:2",
            Self::R16x9 => "16:9",
            Self::R5x7 => "5:7",
            Self::R8x10 => "8:10",
        }
    }

    /// Stable key used in element ids.
    pub fn key(self) -> &'static str {
        match self {
            Self::Free => "free",
            Self::Original => "original",
            Self::Square => "1x1",
            Self::R4x3 => "4x3",
            Self::R3x2 => "3x2",
            Self::R16x9 => "16x9",
            Self::R5x7 => "5x7",
            Self::R8x10 => "8x10",
        }
    }

    /// Width divided by height in pixels, `None` for [`Self::Free`]. Presets other than
    /// `Square`/`Original` follow the image orientation (portrait images get 3:4, ...).
    pub fn ratio(self, image_w: f32, image_h: f32) -> Option<f32> {
        let landscape = image_w >= image_h;
        let oriented = |w: f32, h: f32| {
            if landscape { w / h } else { h / w }
        };
        match self {
            Self::Free => None,
            Self::Original => Some(image_w / image_h),
            Self::Square => Some(1.0),
            Self::R4x3 => Some(oriented(4.0, 3.0)),
            Self::R3x2 => Some(oriented(3.0, 2.0)),
            Self::R16x9 => Some(oriented(16.0, 9.0)),
            Self::R5x7 => Some(oriented(7.0, 5.0)),
            Self::R8x10 => Some(oriented(10.0, 8.0)),
        }
    }
}

/// The part of the crop rectangle a drag grabs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CropHandle {
    /// The inside: moves the whole rectangle.
    Move,
    /// Top-left corner.
    TopLeft,
    /// Top edge.
    Top,
    /// Top-right corner.
    TopRight,
    /// Right edge.
    Right,
    /// Bottom-right corner.
    BottomRight,
    /// Bottom edge.
    Bottom,
    /// Bottom-left corner.
    BottomLeft,
    /// Left edge.
    Left,
}

impl CropHandle {
    /// The eight resize handles.
    pub const RESIZE: [Self; 8] = [
        Self::TopLeft,
        Self::Top,
        Self::TopRight,
        Self::Right,
        Self::BottomRight,
        Self::Bottom,
        Self::BottomLeft,
        Self::Left,
    ];

    /// Stable key used in element ids.
    pub fn key(self) -> &'static str {
        match self {
            Self::Move => "move",
            Self::TopLeft => "top-left",
            Self::Top => "top",
            Self::TopRight => "top-right",
            Self::Right => "right",
            Self::BottomRight => "bottom-right",
            Self::Bottom => "bottom",
            Self::BottomLeft => "bottom-left",
            Self::Left => "left",
        }
    }

    /// Which sides the handle moves per axis: `-1` the low edge, `1` the high edge, `0` neither.
    fn sides(self) -> (i8, i8) {
        match self {
            Self::Move => (0, 0),
            Self::TopLeft => (-1, -1),
            Self::Top => (0, -1),
            Self::TopRight => (1, -1),
            Self::Right => (1, 0),
            Self::BottomRight => (1, 1),
            Self::Bottom => (0, 1),
            Self::BottomLeft => (-1, 1),
            Self::Left => (-1, 0),
        }
    }

    /// Anchor position inside a rectangle as fractions `(x, y)` of its size.
    pub fn anchor(self) -> (f32, f32) {
        match self {
            Self::Move => (0.5, 0.5),
            Self::TopLeft => (0.0, 0.0),
            Self::Top => (0.5, 0.0),
            Self::TopRight => (1.0, 0.0),
            Self::Right => (1.0, 0.5),
            Self::BottomRight => (1.0, 1.0),
            Self::Bottom => (0.5, 1.0),
            Self::BottomLeft => (0.0, 1.0),
            Self::Left => (0.0, 0.5),
        }
    }
}

/// The crop tool's view-model. The rectangle is normalized to the image entering the crop op.
#[derive(Debug, Clone)]
pub struct CropVm {
    image_w: f32,
    image_h: f32,
    rect: NormRect,
    aspect: AspectPreset,
}

impl CropVm {
    /// A crop covering the whole `image_w` x `image_h` pixel image.
    pub fn new(image_w: u32, image_h: u32) -> Self {
        Self {
            image_w: image_w.max(1) as f32,
            image_h: image_h.max(1) as f32,
            rect: full(),
            aspect: AspectPreset::Free,
        }
    }

    /// The current rectangle.
    pub fn rect(&self) -> NormRect {
        self.rect
    }

    /// The selected preset.
    pub fn aspect(&self) -> AspectPreset {
        self.aspect
    }

    /// Image size in pixels.
    pub fn image_size(&self) -> (f32, f32) {
        (self.image_w, self.image_h)
    }

    /// Width divided by height of the crop in pixels.
    pub fn pixel_ratio(&self) -> f32 {
        (self.rect.w * self.image_w) / (self.rect.h * self.image_h)
    }

    /// Crop size in pixels.
    pub fn pixel_size(&self) -> (u32, u32) {
        (
            (self.rect.w * self.image_w).round() as u32,
            (self.rect.h * self.image_h).round() as u32,
        )
    }

    /// Whether the rectangle covers the whole image.
    pub fn is_full(&self) -> bool {
        let r = self.rect;
        r.x <= 0.0 && r.y <= 0.0 && r.w >= 1.0 && r.h >= 1.0
    }

    /// Selects a preset. A locked ratio shrinks the current rectangle around its centre to the
    /// largest rectangle of that ratio that fits inside it.
    pub fn set_aspect(&mut self, aspect: AspectPreset) {
        self.aspect = aspect;
        let Some(ratio) = aspect.ratio(self.image_w, self.image_h) else {
            return;
        };
        let r = self.rect;
        let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let (pw, ph) = (r.w * self.image_w, r.h * self.image_h);
        let (nw, nh) = if pw / ph > ratio {
            (ph * ratio, ph)
        } else {
            (pw, pw / ratio)
        };
        let (mut w, mut h) = (nw / self.image_w, nh / self.image_h);
        let grow = (MIN_EDGE / w).max(MIN_EDGE / h).max(1.0);
        w *= grow;
        h *= grow;
        self.rect = clamp_inside(NormRect {
            x: cx - w / 2.0,
            y: cy - h / 2.0,
            w,
            h,
        });
    }

    /// Resets to the whole image, keeping the preset.
    pub fn reset(&mut self) {
        self.rect = full();
        self.set_aspect(self.aspect);
    }

    /// Drags `handle` by `(dx, dy)`, in fractions of the image. With a locked ratio the rectangle
    /// keeps it: corners keep the opposite corner fixed, edges keep the opposite edge and the
    /// centre of the other axis.
    pub fn drag(&mut self, handle: CropHandle, dx: f32, dy: f32) {
        if handle == CropHandle::Move {
            let r = self.rect;
            self.rect = NormRect {
                x: (r.x + dx).clamp(0.0, 1.0 - r.w),
                y: (r.y + dy).clamp(0.0, 1.0 - r.h),
                ..r
            };
            return;
        }
        let r = self.rect;
        let (mut l, mut t, mut rt, mut b) = (r.x, r.y, r.x + r.w, r.y + r.h);
        let (ax, ay) = handle.sides();
        match ax {
            -1 => l = (l + dx).clamp(0.0, rt - MIN_EDGE),
            1 => rt = (rt + dx).clamp(l + MIN_EDGE, 1.0),
            _ => {}
        }
        match ay {
            -1 => t = (t + dy).clamp(0.0, b - MIN_EDGE),
            1 => b = (b + dy).clamp(t + MIN_EDGE, 1.0),
            _ => {}
        }
        let Some(ratio) = self.aspect.ratio(self.image_w, self.image_h) else {
            self.rect = NormRect {
                x: l,
                y: t,
                w: rt - l,
                h: b - t,
            };
            return;
        };
        let (iw, ih) = (self.image_w, self.image_h);
        let width_from_height = |h: f32| h * ih * ratio / iw;
        let height_from_width = |w: f32| w * iw / ratio / ih;
        let (mut w, mut h) = (rt - l, b - t);
        if ax == 0 {
            h = b - t;
            w = width_from_height(h);
        } else if ay == 0 {
            w = rt - l;
            h = height_from_width(w);
        } else if (w - r.w).abs() >= (h - r.h).abs() {
            h = height_from_width(w);
        } else {
            w = width_from_height(h);
        }
        let (mid_x, mid_y) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
        let max_w = match ax {
            -1 => r.x + r.w,
            1 => 1.0 - r.x,
            _ => 2.0 * mid_x.min(1.0 - mid_x),
        };
        let max_h = match ay {
            -1 => r.y + r.h,
            1 => 1.0 - r.y,
            _ => 2.0 * mid_y.min(1.0 - mid_y),
        };
        let shrink = (max_w / w).min(max_h / h).min(1.0);
        w *= shrink;
        h *= shrink;
        let min_scale = (MIN_EDGE / w).max(MIN_EDGE / h).max(1.0);
        w *= min_scale;
        h *= min_scale;
        let x = match ax {
            -1 => r.x + r.w - w,
            1 => r.x,
            _ => mid_x - w / 2.0,
        };
        let y = match ay {
            -1 => r.y + r.h - h,
            1 => r.y,
            _ => mid_y - h / 2.0,
        };
        self.rect = clamp_inside(NormRect { x, y, w, h });
    }

    /// Arrow keys: moves the rectangle by whole image pixels.
    pub fn nudge(&mut self, dx_px: i32, dy_px: i32) {
        self.drag(
            CropHandle::Move,
            dx_px as f32 / self.image_w,
            dy_px as f32 / self.image_h,
        );
    }

    /// The op to commit, `None` when the crop covers the whole image.
    pub fn op(&self) -> Option<EditOp> {
        (!self.is_full()).then_some(EditOp::Crop {
            x: self.rect.x,
            y: self.rect.y,
            w: self.rect.w,
            h: self.rect.h,
        })
    }
}

fn full() -> NormRect {
    NormRect {
        x: 0.0,
        y: 0.0,
        w: 1.0,
        h: 1.0,
    }
}

fn clamp_inside(r: NormRect) -> NormRect {
    let w = r.w.clamp(MIN_EDGE, 1.0);
    let h = r.h.clamp(MIN_EDGE, 1.0);
    NormRect {
        x: r.x.clamp(0.0, 1.0 - w),
        y: r.y.clamp(0.0, 1.0 - h),
        w,
        h,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_ratio(vm: &CropVm, expected: f32) {
        let got = vm.pixel_ratio();
        assert!(
            (got - expected).abs() < 0.01,
            "pixel ratio {got}, expected {expected}, rect {:?}",
            vm.rect()
        );
    }

    #[test]
    fn vm_crop_aspect_lock() {
        let mut vm = CropVm::new(4000, 3000);
        vm.set_aspect(AspectPreset::Square);
        assert_ratio(&vm, 1.0);
        assert!(vm.rect().w <= 1.0 && vm.rect().h <= 1.0);

        for handle in CropHandle::RESIZE {
            for (dx, dy) in [(0.1, 0.05), (-0.08, -0.12), (0.2, -0.3)] {
                vm.drag(handle, dx, dy);
                assert_ratio(&vm, 1.0);
                let r = vm.rect();
                assert!(r.x >= -1e-4 && r.y >= -1e-4);
                assert!(r.x + r.w <= 1.0001 && r.y + r.h <= 1.0001, "{r:?}");
            }
        }

        vm.set_aspect(AspectPreset::R16x9);
        assert_ratio(&vm, 16.0 / 9.0);
        vm.drag(CropHandle::BottomRight, -0.1, 0.0);
        assert_ratio(&vm, 16.0 / 9.0);
        vm.drag(CropHandle::Top, 0.0, 0.05);
        assert_ratio(&vm, 16.0 / 9.0);

        vm.set_aspect(AspectPreset::Free);
        vm.drag(CropHandle::Right, -0.1, 0.0);
        let free = vm.pixel_ratio();
        assert!((free - 16.0 / 9.0).abs() > 0.05);
    }

    #[test]
    fn crop_presets_follow_orientation() {
        let mut vm = CropVm::new(3000, 4000);
        vm.set_aspect(AspectPreset::R3x2);
        assert_ratio(&vm, 2.0 / 3.0);
        vm.set_aspect(AspectPreset::Original);
        assert_ratio(&vm, 0.75);
    }

    #[test]
    fn crop_move_nudge_and_op() {
        let mut vm = CropVm::new(1000, 500);
        assert!(vm.op().is_none());
        vm.set_aspect(AspectPreset::Square);
        let before = vm.rect();
        vm.nudge(10, 0);
        assert!((vm.rect().x - before.x - 0.01).abs() < 1e-4);
        vm.drag(CropHandle::Move, 5.0, 5.0);
        let r = vm.rect();
        assert!(r.x + r.w <= 1.0001 && r.y + r.h <= 1.0001);
        assert!(matches!(vm.op(), Some(EditOp::Crop { .. })));
        vm.reset();
        assert_ratio(&vm, 1.0);
    }
}
