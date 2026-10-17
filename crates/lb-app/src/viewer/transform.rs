//! Fit rect, zoom-at-anchor and pan clamp (SPEC-VIEWER §V3.3, §V5).
//!
//! Positive scroll `y` is a wheel-down (zoom out, or next photo).

use std::time::Duration;

/// Top bar height in logical pixels.
pub const TOP_BAR_HEIGHT: f32 = 48.0;
/// Floating toolbar height.
pub const TOOLBAR_HEIGHT: f32 = 56.0;
/// Gap between the toolbar and the photo or the filmstrip.
pub const TOOLBAR_GAP: f32 = 12.0;
/// Filmstrip height. Matches the M8 strip.
pub const FILMSTRIP_HEIGHT: f32 = 84.0;
/// Gap under the filmstrip.
const FILMSTRIP_GAP: f32 = 8.0;
/// Info panel width.
pub const INFO_PANEL_WIDTH: f32 = 280.0;
/// Largest zoom factor (existing `ZOOM_LIMITS.1`).
pub const ZOOM_MAX: f32 = 16.0;
/// One mouse-wheel notch.
pub const ZOOM_STEP: f32 = 1.25;
/// A press that travels farther than this is a drag, not a click.
pub const DRAG_THRESHOLD_PX: f32 = 4.0;
/// Horizontal swipe distance that turns the page.
pub const SWIPE_THRESHOLD: f32 = 120.0;
/// Pixel-wheel zoom: `factor = exp(-dy / PIXEL_ZOOM_DIVISOR)`.
pub const PIXEL_ZOOM_DIVISOR: f32 = 300.0;
/// Navigate mode accepts one notch per this interval.
pub const NAV_DEBOUNCE: Duration = Duration::from_millis(120);

/// A rectangle in logical pixels (stage / window space).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RectF {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

impl RectF {
    /// Centre point.
    pub fn center(self) -> (f32, f32) {
        (self.x + self.w * 0.5, self.y + self.h * 0.5)
    }

    /// Whether `p` lies inside, edges included.
    pub fn contains(self, p: (f32, f32)) -> bool {
        p.0 >= self.x && p.1 >= self.y && p.0 <= self.x + self.w && p.1 <= self.y + self.h
    }
}

/// Distances from the stage edges to the fit area.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Insets {
    /// Top inset.
    pub top: f32,
    /// Right inset.
    pub right: f32,
    /// Bottom inset.
    pub bottom: f32,
    /// Left inset.
    pub left: f32,
}

/// Chrome insets for a stage of size `(w, h)` (SPEC-VIEWER §V3.3).
///
/// The fit area does not change when the chrome auto-hides.
pub fn stage_insets(stage: (f32, f32), show_filmstrip: bool, info_open: bool) -> Insets {
    let m = (0.03 * stage.0.min(stage.1)).round().max(24.0);
    let film = if show_filmstrip {
        FILMSTRIP_HEIGHT
    } else {
        0.0
    };
    Insets {
        top: TOP_BAR_HEIGHT,
        bottom: TOOLBAR_HEIGHT + TOOLBAR_GAP + film + FILMSTRIP_GAP,
        left: m,
        right: m + if info_open { INFO_PANEL_WIDTH } else { 0.0 },
    }
}

/// The rectangle inside `stage` after `insets`. Empty stages become a 1 px area.
pub fn fit_area(stage: (f32, f32), insets: Insets) -> RectF {
    let w = (stage.0 - insets.left - insets.right).max(1.0);
    let h = (stage.1 - insets.top - insets.bottom).max(1.0);
    RectF {
        x: insets.left,
        y: insets.top,
        w,
        h,
    }
}

/// Scale that fits `image` in `area`. Small photos stay at 1 unless `enlarge_small`.
pub fn fit_scale(image: (f32, f32), area: RectF, enlarge_small: bool) -> f32 {
    if image.0 <= 0.0 || image.1 <= 0.0 || area.w <= 0.0 || area.h <= 0.0 {
        return 1.0;
    }
    let mut s = (area.w / image.0).min(area.h / image.1);
    if !enlarge_small {
        s = s.min(1.0);
    }
    s
}

/// Image scaled into `area`, centred, then rounded to whole logical pixels.
pub fn fit_rect(image: (f32, f32), area: RectF, enlarge_small: bool) -> RectF {
    let s = fit_scale(image, area, enlarge_small);
    let ideal_w = image.0 * s;
    let ideal_h = image.1 * s;
    let mut w = ideal_w.round().max(1.0);
    let mut h = ideal_h.round().max(1.0);
    if w > area.w {
        w = area.w.max(1.0);
    }
    if h > area.h {
        h = area.h.max(1.0);
    }
    let mut x = area.x + ((area.w - w) * 0.5).round();
    let mut y = area.y + ((area.h - h) * 0.5).round();
    x = x.clamp(area.x, area.x + area.w - w);
    y = y.clamp(area.y, area.y + area.h - h);
    RectF { x, y, w, h }
}

/// Maps an image pixel `p` to the stage point `offset + p · scale`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ViewTransform {
    /// Image pixels per stage pixel.
    pub scale: f32,
    /// Stage position of image pixel `(0, 0)`.
    pub offset: (f32, f32),
}

impl ViewTransform {
    /// The transform whose image rectangle is `rect`.
    pub fn from_fit(image: (f32, f32), rect: RectF) -> Self {
        let scale = if image.0 > 0.0 { rect.w / image.0 } else { 1.0 };
        Self {
            scale,
            offset: (rect.x, rect.y),
        }
    }

    /// Image pixel to stage point.
    pub fn apply(self, p: (f32, f32)) -> (f32, f32) {
        (
            self.offset.0 + p.0 * self.scale,
            self.offset.1 + p.1 * self.scale,
        )
    }

    /// Stage point to image pixel.
    pub fn unapply(self, stage: (f32, f32)) -> (f32, f32) {
        let s = if self.scale == 0.0 { 1.0 } else { self.scale };
        ((stage.0 - self.offset.0) / s, (stage.1 - self.offset.1) / s)
    }

    /// Zoom to `scale` keeping the image point under `anchor` fixed (SPEC-VIEWER §V5.2).
    pub fn zoom_at(self, anchor: (f32, f32), scale: f32) -> Self {
        let s = if self.scale == 0.0 { 1.0 } else { self.scale };
        let k = scale / s;
        Self {
            scale,
            offset: (
                anchor.0 - (anchor.0 - self.offset.0) * k,
                anchor.1 - (anchor.1 - self.offset.1) * k,
            ),
        }
    }
}

/// How the photo is scaled. `Fit` is the centred fit rect.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ZoomMode {
    /// Fit the fit-area.
    Fit,
    /// One image pixel per stage pixel.
    Actual,
    /// An explicit factor.
    Factor(f32),
}

/// A zoom result: the mode label and the transform that realises it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct FramedView {
    /// Fit, 100 % or an explicit factor.
    pub mode: ZoomMode,
    /// Mapping in use.
    pub transform: ViewTransform,
}

/// Multiply `current.scale` by `factor` about `anchor`.
///
/// At or below the fit scale the view snaps back to the centred fit rect.
pub fn zoom_about(
    current: ViewTransform,
    image: (f32, f32),
    area: RectF,
    anchor: (f32, f32),
    factor: f32,
    enlarge_small: bool,
) -> FramedView {
    let s_fit = fit_scale(image, area, enlarge_small);
    let fitted = ViewTransform::from_fit(image, fit_rect(image, area, enlarge_small));
    let target = current.scale * factor;
    if !target.is_finite() || target <= s_fit * (1.0 + 1.0e-4) {
        return FramedView {
            mode: ZoomMode::Fit,
            transform: fitted,
        };
    }
    let scale = target.min(ZOOM_MAX);
    FramedView {
        mode: ZoomMode::Factor(scale),
        transform: current.zoom_at(anchor, scale),
    }
}

/// Keep a zoomed image covering the stage, or centre it in the fit area when it is smaller.
pub fn clamp_pan(
    transform: ViewTransform,
    image: (f32, f32),
    stage: (f32, f32),
    area: RectF,
) -> ViewTransform {
    let iw = image.0 * transform.scale;
    let ih = image.1 * transform.scale;
    let offset = (
        clamp_axis(transform.offset.0, iw, stage.0, area.x, area.w),
        clamp_axis(transform.offset.1, ih, stage.1, area.y, area.h),
    );
    ViewTransform {
        scale: transform.scale,
        offset,
    }
}

fn clamp_axis(offset: f32, size: f32, stage: f32, area_origin: f32, area_size: f32) -> f32 {
    if size <= stage {
        area_origin + (area_size - size) * 0.5
    } else {
        offset.clamp(stage - size, 0.0)
    }
}

/// `true` when the pointer travelled more than [`DRAG_THRESHOLD_PX`].
pub fn is_drag(start: (f32, f32), end: (f32, f32)) -> bool {
    let dx = end.0 - start.0;
    let dy = end.1 - start.1;
    dx * dx + dy * dy > DRAG_THRESHOLD_PX * DRAG_THRESHOLD_PX
}

/// What the mouse wheel does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum WheelMode {
    /// Zoom at the cursor.
    Zoom,
    /// Turn the page.
    Navigate,
}

/// A scroll tick. Positive `y` is wheel-down.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ScrollDelta {
    /// Notch deltas. Fractions accumulate.
    Lines {
        /// Horizontal notches.
        x: f32,
        /// Vertical notches.
        y: f32,
    },
    /// Pixel deltas (trackpad).
    Pixels {
        /// Horizontal pixels.
        x: f32,
        /// Vertical pixels.
        y: f32,
    },
}

/// Modifiers that change how a scroll is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScrollMods {
    /// Ctrl or Cmd is held: always zoom.
    pub ctrl_or_cmd: bool,
    /// The photo is zoomed past fit, so a swipe must not turn the page.
    pub zoomed_past_fit: bool,
}

/// One outcome of a scroll or swipe.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Gesture {
    /// Multiply the current scale by `factor` about the cursor.
    Zoom {
        /// Scale multiplier.
        factor: f32,
    },
    /// `+1` is the next photo, `-1` the previous.
    Navigate(i8),
}

/// Remembers fractional wheel notches and an in-progress swipe.
#[derive(Debug, Clone, Copy, Default)]
pub struct GestureState {
    line_y: f32,
    swipe_x: f32,
    last_navigate: Option<Duration>,
}

impl GestureState {
    /// Fold one scroll event into this state.
    pub fn on_scroll(
        &mut self,
        mode: WheelMode,
        delta: ScrollDelta,
        mods: ScrollMods,
        now: Duration,
    ) -> Option<Gesture> {
        match delta {
            ScrollDelta::Lines { y, .. } => self.on_lines(mode, y, mods, now),
            ScrollDelta::Pixels { x, y } => self.on_pixels(mode, x, y, mods, now),
        }
    }

    fn on_lines(
        &mut self,
        mode: WheelMode,
        y: f32,
        mods: ScrollMods,
        now: Duration,
    ) -> Option<Gesture> {
        if !y.is_finite() || y == 0.0 {
            return None;
        }
        self.swipe_x = 0.0;
        self.line_y += y;
        let steps = self.line_y.trunc();
        if steps == 0.0 {
            return None;
        }
        self.line_y -= steps;
        if mods.ctrl_or_cmd || mode == WheelMode::Zoom {
            let n = steps.clamp(-64.0, 64.0) as i32;
            return Some(Gesture::Zoom {
                factor: ZOOM_STEP.powi(-n),
            });
        }
        let dir: i8 = if steps > 0.0 { 1 } else { -1 };
        if !self.navigate_ready(now) {
            return None;
        }
        self.last_navigate = Some(now);
        Some(Gesture::Navigate(dir))
    }

    fn on_pixels(
        &mut self,
        mode: WheelMode,
        x: f32,
        y: f32,
        mods: ScrollMods,
        _now: Duration,
    ) -> Option<Gesture> {
        if !x.is_finite() || !y.is_finite() {
            return None;
        }
        let horizontal = x.abs() > 2.0 * y.abs() && x != 0.0;
        if horizontal && !mods.ctrl_or_cmd {
            self.line_y = 0.0;
            if mods.zoomed_past_fit {
                self.swipe_x = 0.0;
                return None;
            }
            self.swipe_x += x;
            if self.swipe_x.abs() >= SWIPE_THRESHOLD {
                let dir: i8 = if self.swipe_x.is_sign_negative() {
                    1
                } else {
                    -1
                };
                self.swipe_x = 0.0;
                return Some(Gesture::Navigate(dir));
            }
            return None;
        }
        self.swipe_x = 0.0;
        if y == 0.0 {
            return None;
        }
        if mods.ctrl_or_cmd || mode == WheelMode::Zoom {
            return Some(Gesture::Zoom {
                factor: (-y / PIXEL_ZOOM_DIVISOR).exp(),
            });
        }
        None
    }

    fn navigate_ready(&self, now: Duration) -> bool {
        self.last_navigate
            .is_none_or(|t| now.saturating_sub(t) >= NAV_DEBOUNCE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() <= 1.0
    }

    proptest! {
        #[test]
        fn viewer_fit_rect_respects_insets_and_aspect(
            stage_w in 640.0f32..3840.0,
            stage_h in 480.0f32..2160.0,
            filmstrip in proptest::bool::ANY,
            info in proptest::bool::ANY,
            img_w in 1.0f32..8000.0,
            img_h in 1.0f32..8000.0,
            enlarge in proptest::bool::ANY,
        ) {
            let stage = (stage_w, stage_h);
            let insets = stage_insets(stage, filmstrip, info);
            let m = (0.03 * stage_w.min(stage_h)).round().max(24.0);
            let bottom = 56.0 + 12.0 + if filmstrip { 84.0 } else { 0.0 } + 8.0;
            let right = m + if info { 280.0 } else { 0.0 };
            prop_assert!((insets.top - 48.0).abs() < 1e-3);
            prop_assert!((insets.left - m).abs() < 1e-3);
            prop_assert!((insets.bottom - bottom).abs() < 1e-3);
            prop_assert!((insets.right - right).abs() < 1e-3);
            let area = fit_area(stage, insets);
            prop_assert!(area.x >= insets.left - 1e-3);
            prop_assert!(area.y >= insets.top - 1e-3);
            prop_assert!(area.x + area.w <= stage_w - insets.right + 1.0);
            prop_assert!(area.y + area.h <= stage_h - insets.bottom + 1.0);

            let rect = fit_rect((img_w, img_h), area, enlarge);
            prop_assert!(rect.x + 1e-2 >= area.x);
            prop_assert!(rect.y + 1e-2 >= area.y);
            prop_assert!(rect.x + rect.w <= area.x + area.w + 1e-2);
            prop_assert!(rect.y + rect.h <= area.y + area.h + 1e-2);

            let s = fit_scale((img_w, img_h), area, enlarge);
            prop_assert!(close(rect.w, img_w * s), "w {} vs {}", rect.w, img_w * s);
            prop_assert!(close(rect.h, img_h * s), "h {} vs {}", rect.h, img_h * s);

            let left = rect.x - area.x;
            let right = area.x + area.w - (rect.x + rect.w);
            prop_assert!(close(left, right), "horiz {left} vs {right}");
            let top = rect.y - area.y;
            let bottom = area.y + area.h - (rect.y + rect.h);
            prop_assert!(close(top, bottom), "vert {top} vs {bottom}");
        }
    }

    #[test]
    fn viewer_small_image_not_enlarged() {
        let area = RectF {
            x: 10.0,
            y: 20.0,
            w: 800.0,
            h: 600.0,
        };
        let rect = fit_rect((32.0, 32.0), area, false);
        assert_eq!((rect.w, rect.h), (32.0, 32.0));
        assert!((rect.x - (10.0 + 384.0)).abs() <= 1.0);
        assert!((rect.y - (20.0 + 284.0)).abs() <= 1.0);
        assert!((fit_scale((32.0, 32.0), area, false) - 1.0).abs() < 1e-6);
    }

    #[test]
    fn viewer_enlarge_small_setting() {
        let area = RectF {
            x: 0.0,
            y: 0.0,
            w: 800.0,
            h: 600.0,
        };
        let big = fit_rect((32.0, 24.0), area, true);
        assert_eq!((big.w, big.h), (800.0, 600.0));
        let small = fit_rect((32.0, 24.0), area, false);
        assert_eq!((small.w, small.h), (32.0, 24.0));
    }

    proptest! {
        #[test]
        fn viewer_zoom_anchor_point_stays_fixed(
            ox in -500.0f32..1500.0,
            oy in -500.0f32..1500.0,
            scale in 0.05f32..8.0,
            factor in 0.25f32..4.0,
            ax in 0.0f32..1600.0,
            ay in 0.0f32..1200.0,
        ) {
            let scale2 = (scale * factor).clamp(0.05, ZOOM_MAX);
            let t = ViewTransform { scale, offset: (ox, oy) };
            let image_pt = t.unapply((ax, ay));
            let next = t.zoom_at((ax, ay), scale2);
            let back = next.apply(image_pt);
            prop_assert!((back.0 - ax).abs() < 1e-3, "{} vs {ax}", back.0);
            prop_assert!((back.1 - ay).abs() < 1e-3, "{} vs {ay}", back.1);
        }
    }

    #[test]
    fn viewer_zoom_out_snaps_to_fit() {
        let image = (4000.0, 3000.0);
        let area = RectF {
            x: 24.0,
            y: 48.0,
            w: 800.0,
            h: 600.0,
        };
        let fit = fit_rect(image, area, false);
        let base = ViewTransform::from_fit(image, fit);
        let anchor = area.center();
        let zoomed = zoom_about(base, image, area, anchor, ZOOM_STEP, false);
        assert!(
            matches!(zoomed.mode, ZoomMode::Factor(f) if (f - base.scale * ZOOM_STEP).abs() < 1e-4),
            "{zoomed:?}"
        );
        let back = zoom_about(
            zoomed.transform,
            image,
            area,
            anchor,
            1.0 / ZOOM_STEP,
            false,
        );
        assert_eq!(back.mode, ZoomMode::Fit);
        assert!((back.transform.scale - base.scale).abs() < 1e-4);
        assert!((back.transform.offset.0 - base.offset.0).abs() < 1e-2);
        assert!((back.transform.offset.1 - base.offset.1).abs() < 1e-2);
        let again = zoom_about(back.transform, image, area, anchor, 1.0 / ZOOM_STEP, false);
        assert_eq!(again.mode, ZoomMode::Fit);
        assert!((again.transform.offset.0 - base.offset.0).abs() < 1e-2);
    }

    #[test]
    fn viewer_pan_clamped_to_edges() {
        let stage = (1000.0, 800.0);
        let area = RectF {
            x: 40.0,
            y: 50.0,
            w: 900.0,
            h: 700.0,
        };
        let image = (2000.0, 1000.0);
        let pushed = clamp_pan(
            ViewTransform {
                scale: 1.0,
                offset: (500.0, 400.0),
            },
            image,
            stage,
            area,
        );
        assert!((pushed.offset.0 - 0.0).abs() < 1e-3);
        assert!((pushed.offset.1 - 0.0).abs() < 1e-3);
        let pulled = clamp_pan(
            ViewTransform {
                scale: 1.0,
                offset: (-5000.0, -5000.0),
            },
            image,
            stage,
            area,
        );
        assert!((pulled.offset.0 - (stage.0 - image.0)).abs() < 1e-3);
        assert!((pulled.offset.1 - (stage.1 - image.1)).abs() < 1e-3);

        let small = (100.0, 80.0);
        let centred = clamp_pan(
            ViewTransform {
                scale: 1.0,
                offset: (1.0, 2.0),
            },
            small,
            stage,
            area,
        );
        assert!((centred.offset.0 - (area.x + (area.w - small.0) * 0.5)).abs() < 1e-3);
        assert!((centred.offset.1 - (area.y + (area.h - small.1) * 0.5)).abs() < 1e-3);
    }

    #[test]
    fn viewer_drag_threshold_is_not_click() {
        assert!(!is_drag((0.0, 0.0), (0.0, 0.0)));
        assert!(
            !is_drag((0.0, 0.0), (4.0, 0.0)),
            "exactly 4 px is still a click"
        );
        assert!(is_drag((0.0, 0.0), (4.1, 0.0)));
        assert!(!is_drag((0.0, 0.0), (2.8, 2.8)));
        assert!(is_drag((10.0, 10.0), (13.0, 14.0)));
    }

    #[test]
    fn viewer_wheel_lines_and_pixels_mapping() {
        let mut g = GestureState::default();
        let mods = ScrollMods {
            ctrl_or_cmd: false,
            zoomed_past_fit: false,
        };
        let t0 = Duration::ZERO;
        assert_eq!(
            g.on_scroll(
                WheelMode::Zoom,
                ScrollDelta::Lines { x: 0.0, y: 1.0 },
                mods,
                t0
            ),
            Some(Gesture::Zoom {
                factor: 1.0 / ZOOM_STEP
            })
        );
        assert_eq!(
            g.on_scroll(
                WheelMode::Zoom,
                ScrollDelta::Lines { x: 0.0, y: -1.0 },
                mods,
                t0
            ),
            Some(Gesture::Zoom { factor: ZOOM_STEP })
        );

        let mut frac = GestureState::default();
        assert_eq!(
            frac.on_scroll(
                WheelMode::Zoom,
                ScrollDelta::Lines { x: 0.0, y: -0.4 },
                mods,
                t0
            ),
            None
        );
        assert_eq!(
            frac.on_scroll(
                WheelMode::Zoom,
                ScrollDelta::Lines { x: 0.0, y: -0.4 },
                mods,
                t0
            ),
            None
        );
        assert_eq!(
            frac.on_scroll(
                WheelMode::Zoom,
                ScrollDelta::Lines { x: 0.0, y: -0.4 },
                mods,
                t0
            ),
            Some(Gesture::Zoom { factor: ZOOM_STEP })
        );

        let mut px = GestureState::default();
        let got = px
            .on_scroll(
                WheelMode::Zoom,
                ScrollDelta::Pixels { x: 0.0, y: 300.0 },
                mods,
                t0,
            )
            .unwrap();
        match got {
            Gesture::Zoom { factor } => assert!((factor - (-1.0f32).exp()).abs() < 1e-5),
            other => panic!("{other:?}"),
        }
        let got = px
            .on_scroll(
                WheelMode::Zoom,
                ScrollDelta::Pixels { x: 0.0, y: -300.0 },
                mods,
                t0,
            )
            .unwrap();
        match got {
            Gesture::Zoom { factor } => assert!((factor - 1.0f32.exp()).abs() < 1e-5),
            other => panic!("{other:?}"),
        }

        let mut forced = GestureState::default();
        let zoom = forced.on_scroll(
            WheelMode::Navigate,
            ScrollDelta::Lines { x: 0.0, y: -1.0 },
            ScrollMods {
                ctrl_or_cmd: true,
                zoomed_past_fit: false,
            },
            t0,
        );
        assert_eq!(zoom, Some(Gesture::Zoom { factor: ZOOM_STEP }));
    }

    #[test]
    fn viewer_wheel_navigate_setting() {
        let mods = ScrollMods {
            ctrl_or_cmd: false,
            zoomed_past_fit: false,
        };
        let mut g = GestureState::default();
        let t0 = Duration::from_millis(1_000);
        assert_eq!(
            g.on_scroll(
                WheelMode::Navigate,
                ScrollDelta::Lines { x: 0.0, y: 1.0 },
                mods,
                t0
            ),
            Some(Gesture::Navigate(1))
        );
        assert_eq!(
            g.on_scroll(
                WheelMode::Navigate,
                ScrollDelta::Lines { x: 0.0, y: 1.0 },
                mods,
                t0 + Duration::from_millis(119)
            ),
            None,
            "120 ms debounce"
        );
        assert_eq!(
            g.on_scroll(
                WheelMode::Navigate,
                ScrollDelta::Lines { x: 0.0, y: -1.0 },
                mods,
                t0 + NAV_DEBOUNCE
            ),
            Some(Gesture::Navigate(-1))
        );

        let mut frac = GestureState::default();
        assert_eq!(
            frac.on_scroll(
                WheelMode::Navigate,
                ScrollDelta::Lines { x: 0.0, y: 0.6 },
                mods,
                t0
            ),
            None
        );
        assert_eq!(
            frac.on_scroll(
                WheelMode::Navigate,
                ScrollDelta::Lines { x: 0.0, y: 0.6 },
                mods,
                t0
            ),
            Some(Gesture::Navigate(1))
        );
    }

    #[test]
    fn viewer_swipe_navigates_when_not_zoomed() {
        let t0 = Duration::ZERO;
        let mut g = GestureState::default();
        let fit = ScrollMods {
            ctrl_or_cmd: false,
            zoomed_past_fit: false,
        };
        assert_eq!(
            g.on_scroll(
                WheelMode::Zoom,
                ScrollDelta::Pixels { x: -60.0, y: 0.0 },
                fit,
                t0
            ),
            None
        );
        assert_eq!(
            g.on_scroll(
                WheelMode::Zoom,
                ScrollDelta::Pixels { x: -60.0, y: 0.0 },
                fit,
                t0
            ),
            Some(Gesture::Navigate(1)),
            "finger left moves to the next photo"
        );

        let mut back = GestureState::default();
        assert_eq!(
            back.on_scroll(
                WheelMode::Navigate,
                ScrollDelta::Pixels { x: 120.0, y: 10.0 },
                fit,
                t0
            ),
            Some(Gesture::Navigate(-1))
        );

        let mut zoomed = GestureState::default();
        let past = ScrollMods {
            ctrl_or_cmd: false,
            zoomed_past_fit: true,
        };
        assert_eq!(
            zoomed.on_scroll(
                WheelMode::Zoom,
                ScrollDelta::Pixels { x: -200.0, y: 0.0 },
                past,
                t0
            ),
            None,
            "a zoomed photo does not turn the page"
        );

        let mut vertical = GestureState::default();
        let vertical_gesture = vertical.on_scroll(
            WheelMode::Zoom,
            ScrollDelta::Pixels { x: -30.0, y: -40.0 },
            fit,
            t0,
        );
        assert!(
            matches!(vertical_gesture, Some(Gesture::Zoom { .. })),
            "a vertical-dominant scroll zooms, got {vertical_gesture:?}"
        );
    }
}
