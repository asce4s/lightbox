//! Viewer transitions sampled from `Clock::elapsed()` (SPEC-VIEWER §V4).
//!
//! GPUI's `with_animation` is wall-clock time, so tests drive motion by passing
//! `now` explicitly. A new transition starts from the presented state when one
//! is already running, which keeps `Presented` continuous at the input instant.

use super::{RectF, ViewTransform};
use std::time::Duration;

/// `1 − (1 − t)³` (SPEC-VIEWER §V4.2).
pub fn ease_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(3)
}

/// Smoothstep cubic (SPEC-VIEWER §V4.2).
pub fn ease_in_out_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    if t < 0.5 {
        4.0 * t * t * t
    } else {
        1.0 - (-2.0 * t + 2.0).powi(3) / 2.0
    }
}

/// `1 − (1 − t)²`.
pub fn ease_out_quad(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    1.0 - (1.0 - t).powi(2)
}

/// `t³`.
pub fn ease_in_cubic(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * t
}

const OPEN_THUMB: Duration = Duration::from_millis(260);
const OPEN_PLAIN: Duration = Duration::from_millis(220);
const CLOSE_THUMB: Duration = Duration::from_millis(220);
const CLOSE_FADE: Duration = Duration::from_millis(160);
const NAV_SLIDE: Duration = Duration::from_millis(200);
const NAV_CROSS: Duration = Duration::from_millis(320);
const RAPID_CROSS: Duration = Duration::from_millis(120);
const RAPID_GAP: Duration = Duration::from_millis(150);
const ZOOM_DUR: Duration = Duration::from_millis(140);
const REFIT_DUR: Duration = Duration::from_millis(200);
const BOUNCE_DUR: Duration = Duration::from_millis(160);
const REDUCED_FADE: Duration = Duration::from_millis(120);
const CHROME_IDLE: Duration = Duration::from_millis(2_500);
const CHROME_OUT: Duration = Duration::from_millis(200);
const CHROME_IN: Duration = Duration::from_millis(120);
const NAV_SHIFT: f32 = 0.06;
const BOUNCE_PX: f32 = 12.0;

/// Resolved motion level. `auto` is decided by the caller from the OS preference.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MotionMode {
    /// Full geometric transitions.
    Full,
    /// 120 ms opacity crossfade at the final rect. Chrome fades stay.
    Reduced,
    /// Every duration is zero.
    Off,
}

/// One drawn layer (SPEC-VIEWER §V4.1).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LayerState {
    /// Stage rectangle.
    pub rect: RectF,
    /// 0 transparent, 1 opaque.
    pub opacity: f32,
    /// Corner radius in logical pixels.
    pub corner_radius: f32,
}

/// What the stage draws at one instant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Presented {
    /// The photo being shown.
    pub image: Option<LayerState>,
    /// The photo leaving during a navigate.
    pub outgoing: Option<LayerState>,
    /// Backdrop opacity, 0 to 1.
    pub backdrop_opacity: f32,
    /// Blend from the previous ambient texture to the current one.
    pub backdrop_crossfade: f32,
    /// Chrome opacity, the product of the transition and the auto-hide fade.
    pub chrome_opacity: f32,
}

impl Presented {
    fn closed() -> Self {
        Self {
            image: None,
            outgoing: None,
            backdrop_opacity: 0.0,
            backdrop_crossfade: 1.0,
            chrome_opacity: 0.0,
        }
    }

    fn open_still(rect: RectF) -> Self {
        Self {
            image: Some(layer(rect, 1.0, 0.0)),
            outgoing: None,
            backdrop_opacity: 1.0,
            backdrop_crossfade: 1.0,
            chrome_opacity: 1.0,
        }
    }
}

fn layer(rect: RectF, opacity: f32, radius: f32) -> LayerState {
    LayerState {
        rect,
        opacity,
        corner_radius: radius,
    }
}

fn scaled_about_center(fit: RectF, scale: f32) -> RectF {
    let w = fit.w * scale;
    let h = fit.h * scale;
    RectF {
        x: fit.x + (fit.w - w) * 0.5,
        y: fit.y + (fit.h - h) * 0.5,
        w,
        h,
    }
}

fn shift_x(rect: RectF, dx: f32) -> RectF {
    RectF {
        x: rect.x + dx,
        ..rect
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_rect(a: RectF, b: RectF, t: f32) -> RectF {
    RectF {
        x: lerp(a.x, b.x, t),
        y: lerp(a.y, b.y, t),
        w: lerp(a.w, b.w, t),
        h: lerp(a.h, b.h, t),
    }
}

fn unit(elapsed: Duration, dur: Duration) -> f32 {
    if dur.is_zero() {
        return 1.0;
    }
    (elapsed.as_secs_f32() / dur.as_secs_f32()).clamp(0.0, 1.0)
}

fn window_linear(elapsed: Duration, start: Duration, end: Duration) -> f32 {
    if elapsed <= start {
        0.0
    } else {
        unit(elapsed.saturating_sub(start), end.saturating_sub(start))
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Profile {
    OpenThumb,
    OpenPlain,
    CloseThumb,
    CloseFade,
    Navigate,
    Rapid,
    Zoom(ZoomAnim),
    Refit,
    Bounce(i32),
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct ZoomAnim {
    image: (f32, f32),
    anchor: (f32, f32),
    from_scale: f32,
    to_scale: f32,
    from_offset: (f32, f32),
}

#[derive(Debug, Clone)]
struct Running {
    started: Duration,
    until: Duration,
    from: Presented,
    end: Presented,
    profile: Profile,
}

#[derive(Debug, Clone)]
struct ChromeIdle {
    last_activity: Duration,
    /// Fade toward visible: `(started, opacity at start)`.
    fade_in: Option<(Duration, f32)>,
}

impl ChromeIdle {
    fn factor(&self, now: Duration) -> f32 {
        if let Some((started, from)) = self.fade_in {
            let u = unit(now.saturating_sub(started), CHROME_IN);
            if u < 1.0 {
                return lerp(from, 1.0, u);
            }
        }
        let idle = now.saturating_sub(self.last_activity);
        if idle <= CHROME_IDLE {
            return 1.0;
        }
        1.0 - unit(idle - CHROME_IDLE, CHROME_OUT)
    }

    fn animating(&self, now: Duration) -> bool {
        if let Some((started, _)) = self.fade_in
            && now < started + CHROME_IN
        {
            return true;
        }
        let idle = now.saturating_sub(self.last_activity);
        idle > CHROME_IDLE && idle < CHROME_IDLE + CHROME_OUT
    }

    fn activity(&mut self, now: Duration) {
        let current = self.factor(now);
        self.last_activity = now;
        if current < 0.999 {
            self.fade_in = Some((now, current));
        } else {
            self.fade_in = None;
        }
    }
}

/// The viewer's motion state.
#[derive(Debug, Clone)]
pub struct Motion {
    mode: MotionMode,
    stage_w: f32,
    settled: Presented,
    running: Option<Running>,
    chrome: ChromeIdle,
    last_navigate: Option<Duration>,
}

impl Motion {
    /// A closed viewer.
    pub fn new(mode: MotionMode, stage: (f32, f32)) -> Self {
        Self {
            mode,
            stage_w: stage.0,
            settled: Presented::closed(),
            running: None,
            chrome: ChromeIdle {
                last_activity: Duration::ZERO,
                fade_in: None,
            },
            last_navigate: None,
        }
    }

    /// Changes the motion level. The current frame is kept.
    pub fn set_mode(&mut self, mode: MotionMode) {
        self.mode = mode;
    }

    /// Stage width used by the navigate slide (`0.06 · W`).
    pub fn set_stage(&mut self, stage: (f32, f32)) {
        self.stage_w = stage.0;
    }

    /// Places an open photo with no animation. Tests and settled navigation start here.
    pub fn settle_open(&mut self, fit: RectF) {
        self.running = None;
        self.settled = Presented::open_still(fit);
    }

    /// Overlay open from a thumbnail rect.
    pub fn open_from_thumbnail(&mut self, now: Duration, source: RectF, fit: RectF) {
        let end = Presented::open_still(fit);
        let from = if self.mode == MotionMode::Reduced {
            Presented {
                image: Some(layer(fit, 0.0, 0.0)),
                backdrop_opacity: 0.0,
                chrome_opacity: 0.0,
                ..end
            }
        } else {
            Presented {
                image: Some(layer(source, 1.0, 6.0)),
                backdrop_opacity: 0.0,
                chrome_opacity: 0.0,
                ..end
            }
        };
        self.begin(now, Profile::OpenThumb, from, end, OPEN_THUMB);
    }

    /// Open when no thumbnail rect is visible (standalone, or the cell is off screen).
    pub fn open_plain(&mut self, now: Duration, fit: RectF) {
        let end = Presented::open_still(fit);
        let from = if self.mode == MotionMode::Reduced {
            Presented {
                image: Some(layer(fit, 0.0, 0.0)),
                backdrop_opacity: 0.0,
                chrome_opacity: 0.0,
                ..end
            }
        } else {
            Presented {
                image: Some(layer(scaled_about_center(fit, 0.92), 0.0, 0.0)),
                backdrop_opacity: 0.0,
                chrome_opacity: 0.0,
                ..end
            }
        };
        self.begin(now, Profile::OpenPlain, from, end, OPEN_PLAIN);
    }

    /// Close back onto a visible cell.
    pub fn close_to_thumbnail(&mut self, now: Duration, cell: RectF) {
        let current = self.sample_transition(now);
        let from = if self.mode == MotionMode::Reduced {
            Presented {
                image: Some(layer(cell, current.image.map_or(1.0, |l| l.opacity), 6.0)),
                outgoing: None,
                ..current
            }
        } else {
            Presented {
                outgoing: None,
                ..current
            }
        };
        let end = Presented {
            image: Some(layer(cell, 1.0, 6.0)),
            outgoing: None,
            backdrop_opacity: 0.0,
            backdrop_crossfade: current.backdrop_crossfade,
            chrome_opacity: 0.0,
        };
        self.begin(now, Profile::CloseThumb, from, end, CLOSE_THUMB);
    }

    /// Close when the cell cannot be measured.
    pub fn close_fade(&mut self, now: Duration) {
        let current = self.sample_transition(now);
        let base = current.image.map(|l| l.rect).unwrap_or(RectF {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0,
        });
        let end_rect = scaled_about_center(base, 0.96);
        let from = if self.mode == MotionMode::Reduced {
            Presented {
                image: current
                    .image
                    .map(|l| layer(end_rect, l.opacity, l.corner_radius)),
                outgoing: None,
                ..current
            }
        } else {
            Presented {
                outgoing: None,
                ..current
            }
        };
        let end = Presented {
            image: current.image.map(|l| layer(end_rect, 0.0, l.corner_radius)),
            outgoing: None,
            backdrop_opacity: 0.0,
            backdrop_crossfade: current.backdrop_crossfade,
            chrome_opacity: 0.0,
        };
        self.begin(now, Profile::CloseFade, from, end, CLOSE_FADE);
    }

    /// Next (`dir > 0`) or previous. A second call inside 150 ms skips the slide.
    pub fn navigate(&mut self, now: Duration, dir: i32, fit: RectF) {
        let rapid = self
            .last_navigate
            .is_some_and(|t| now.saturating_sub(t) < RAPID_GAP);
        self.last_navigate = Some(now);
        let current = self.sample_transition(now);
        let dir = if dir < 0 { -1 } else { 1 };
        let dx = dir as f32 * NAV_SHIFT * self.stage_w;
        let chrome = current.chrome_opacity;
        if rapid || self.mode == MotionMode::Reduced {
            let from = Presented {
                image: Some(layer(fit, 1.0, 0.0)),
                outgoing: None,
                backdrop_opacity: 1.0,
                backdrop_crossfade: 0.0,
                chrome_opacity: chrome,
            };
            let end = Presented {
                backdrop_crossfade: 1.0,
                ..from
            };
            self.begin(now, Profile::Rapid, from, end, RAPID_CROSS);
            return;
        }
        let outgoing_rect = current.image.map_or(fit, |l| l.rect);
        let from = Presented {
            image: Some(layer(shift_x(fit, dx), 0.0, 0.0)),
            outgoing: Some(layer(outgoing_rect, 1.0, 0.0)),
            backdrop_opacity: 1.0,
            backdrop_crossfade: 0.0,
            chrome_opacity: chrome,
        };
        let end = Presented {
            image: Some(layer(fit, 1.0, 0.0)),
            outgoing: Some(layer(shift_x(outgoing_rect, -dx), 0.0, 0.0)),
            backdrop_opacity: 1.0,
            backdrop_crossfade: 1.0,
            chrome_opacity: chrome,
        };
        self.begin(now, Profile::Navigate, from, end, NAV_CROSS);
    }

    /// Animate scale in log space about `anchor` (SPEC-VIEWER §V5.2, zoom row).
    pub fn zoom_to(&mut self, now: Duration, image: (f32, f32), anchor: (f32, f32), to_scale: f32) {
        let current = self.sample_transition(now);
        let (from_scale, from_offset) = current.image.map_or((1.0, (0.0, 0.0)), |l| {
            let scale = if image.0 > 0.0 {
                l.rect.w / image.0
            } else {
                1.0
            };
            (scale, (l.rect.x, l.rect.y))
        });
        let to_scale = to_scale.max(1.0e-3);
        let transform = ViewTransform {
            scale: from_scale,
            offset: from_offset,
        }
        .zoom_at(anchor, to_scale);
        let end_rect = RectF {
            x: transform.offset.0,
            y: transform.offset.1,
            w: image.0 * to_scale,
            h: image.1 * to_scale,
        };
        let end = Presented {
            image: current
                .image
                .map(|l| layer(end_rect, l.opacity, l.corner_radius)),
            ..current
        };
        let zoom = ZoomAnim {
            image,
            anchor,
            from_scale,
            to_scale,
            from_offset,
        };
        self.begin(now, Profile::Zoom(zoom), current, end, ZOOM_DUR);
    }

    /// Fit-rect change (info panel or resize) while the zoom mode is Fit.
    pub fn refit(&mut self, now: Duration, fit: RectF) {
        let current = self.sample_transition(now);
        let end = Presented {
            image: current
                .image
                .map(|l| layer(fit, l.opacity, l.corner_radius)),
            ..current
        };
        self.begin(now, Profile::Refit, current, end, REFIT_DUR);
    }

    /// End-of-filmstrip bounce. `dir > 0` is "next" at the last item.
    pub fn bounce(&mut self, now: Duration, dir: i32) {
        let current = self.sample_transition(now);
        let dir = if dir < 0 { -1 } else { 1 };
        self.begin(now, Profile::Bounce(dir), current, current, BOUNCE_DUR);
    }

    /// Pointer moved: chrome returns, and the idle timer restarts.
    pub fn pointer_moved(&mut self, now: Duration) {
        self.chrome.activity(now);
    }

    /// A key was pressed. Same effect as [`Self::pointer_moved`].
    pub fn key_pressed(&mut self, now: Duration) {
        self.chrome.activity(now);
    }

    /// The frame at `now`.
    pub fn presented(&self, now: Duration) -> Presented {
        let mut frame = self.sample_transition(now);
        frame.chrome_opacity *= self.chrome.factor(now);
        frame.chrome_opacity = frame.chrome_opacity.clamp(0.0, 1.0);
        frame
    }

    /// Whether the view should request another animation frame.
    pub fn is_animating(&self, now: Duration) -> bool {
        self.running
            .as_ref()
            .is_some_and(|r| now < r.started + r.until)
            || self.chrome.animating(now)
    }

    fn begin(
        &mut self,
        now: Duration,
        profile: Profile,
        canonical_from: Presented,
        end: Presented,
        until: Duration,
    ) {
        self.chrome.activity(now);
        if self.mode == MotionMode::Off {
            self.running = None;
            self.settled = end;
            return;
        }
        let interrupting = self
            .running
            .as_ref()
            .is_some_and(|r| now < r.started + r.until);
        let from = if interrupting && !matches!(profile, Profile::Rapid) {
            self.sample_transition(now)
        } else {
            canonical_from
        };
        if until.is_zero() {
            self.running = None;
            self.settled = end;
            return;
        }
        self.running = Some(Running {
            started: now,
            until,
            from,
            end,
            profile,
        });
    }

    fn sample_transition(&self, now: Duration) -> Presented {
        let Some(running) = &self.running else {
            return self.settled;
        };
        if now <= running.started {
            return running.from;
        }
        if now >= running.started + running.until {
            return running.end;
        }
        let elapsed = now.saturating_sub(running.started);
        if self.mode == MotionMode::Reduced {
            return sample_reduced(running, elapsed);
        }
        match running.profile {
            Profile::Zoom(_) => sample_zoom(running, elapsed),
            Profile::Bounce(_) => sample_bounce(running, elapsed),
            _ => sample_curves(running, elapsed),
        }
    }
}

fn sample_reduced(running: &Running, elapsed: Duration) -> Presented {
    let mut frame = sample_curves(running, elapsed);
    let u = unit(elapsed, REDUCED_FADE);
    if let (Some(end), Some(img)) = (running.end.image, frame.image.as_mut()) {
        img.rect = end.rect;
        img.corner_radius = end.corner_radius;
        let from_op = running.from.image.map_or(0.0, |l| l.opacity);
        img.opacity = lerp(from_op, end.opacity, u);
    }
    if let Some(end) = running.end.outgoing {
        frame.outgoing = Some(layer(
            end.rect,
            lerp(
                running.from.outgoing.map_or(0.0, |l| l.opacity),
                end.opacity,
                u,
            ),
            end.corner_radius,
        ));
    } else {
        frame.outgoing = None;
    }
    frame
}

fn sample_curves(running: &Running, elapsed: Duration) -> Presented {
    let (u_geom, u_op, u_back, u_chrome, u_cross) = factors(running.profile, elapsed);
    Presented {
        image: blend_layer(running.from.image, running.end.image, u_geom, u_op),
        outgoing: blend_layer(running.from.outgoing, running.end.outgoing, u_geom, u_op),
        backdrop_opacity: lerp(
            running.from.backdrop_opacity,
            running.end.backdrop_opacity,
            u_back,
        ),
        backdrop_crossfade: lerp(
            running.from.backdrop_crossfade,
            running.end.backdrop_crossfade,
            u_cross,
        ),
        chrome_opacity: lerp(
            running.from.chrome_opacity,
            running.end.chrome_opacity,
            u_chrome,
        ),
    }
}

fn factors(profile: Profile, elapsed: Duration) -> (f32, f32, f32, f32, f32) {
    match profile {
        Profile::OpenThumb => (
            ease_out_cubic(unit(elapsed, OPEN_THUMB)),
            ease_out_cubic(unit(elapsed, OPEN_THUMB)),
            ease_out_quad(unit(elapsed, Duration::from_millis(200))),
            window_linear(elapsed, Duration::from_millis(160), OPEN_THUMB),
            1.0,
        ),
        Profile::OpenPlain => (
            ease_out_cubic(unit(elapsed, OPEN_PLAIN)),
            ease_out_cubic(unit(elapsed, OPEN_PLAIN)),
            ease_out_quad(unit(elapsed, Duration::from_millis(180))),
            window_linear(elapsed, Duration::from_millis(120), OPEN_PLAIN),
            1.0,
        ),
        Profile::CloseThumb => (
            ease_in_out_cubic(unit(elapsed, CLOSE_THUMB)),
            ease_in_out_cubic(unit(elapsed, CLOSE_THUMB)),
            unit(elapsed, CLOSE_THUMB),
            unit(elapsed, Duration::from_millis(80)),
            1.0,
        ),
        Profile::CloseFade => (
            ease_in_cubic(unit(elapsed, CLOSE_FADE)),
            ease_in_cubic(unit(elapsed, CLOSE_FADE)),
            unit(elapsed, CLOSE_FADE),
            unit(elapsed, Duration::from_millis(80)),
            1.0,
        ),
        Profile::Navigate => {
            let slide = ease_out_cubic(unit(elapsed, NAV_SLIDE));
            (slide, slide, 0.0, 0.0, unit(elapsed, NAV_CROSS))
        }
        Profile::Rapid => (0.0, 0.0, 0.0, 0.0, unit(elapsed, RAPID_CROSS)),
        Profile::Refit => {
            let u = ease_out_cubic(unit(elapsed, REFIT_DUR));
            (u, 0.0, 0.0, 0.0, 0.0)
        }
        Profile::Zoom(_) | Profile::Bounce(_) => (0.0, 0.0, 0.0, 0.0, 0.0),
    }
}

fn blend_layer(
    from: Option<LayerState>,
    to: Option<LayerState>,
    u_geom: f32,
    u_op: f32,
) -> Option<LayerState> {
    match (from, to) {
        (None, None) => None,
        (Some(a), Some(b)) => Some(LayerState {
            rect: lerp_rect(a.rect, b.rect, u_geom),
            opacity: lerp(a.opacity, b.opacity, u_op),
            corner_radius: lerp(a.corner_radius, b.corner_radius, u_geom),
        }),
        (Some(a), None) => Some(LayerState {
            opacity: lerp(a.opacity, 0.0, u_op),
            ..a
        }),
        (None, Some(b)) => Some(LayerState {
            rect: b.rect,
            opacity: lerp(0.0, b.opacity, u_op),
            corner_radius: b.corner_radius,
        }),
    }
}

fn sample_zoom(running: &Running, elapsed: Duration) -> Presented {
    let Profile::Zoom(zoom) = running.profile else {
        return running.from;
    };
    let u = ease_out_quad(unit(elapsed, ZOOM_DUR));
    let s0 = zoom.from_scale.max(1.0e-4);
    let s1 = zoom.to_scale.max(1.0e-4);
    let scale = (s0.ln() + (s1.ln() - s0.ln()) * u).exp();
    let transform = ViewTransform {
        scale: s0,
        offset: zoom.from_offset,
    }
    .zoom_at(zoom.anchor, scale);
    let mut frame = running.from;
    if let Some(img) = frame.image.as_mut() {
        img.rect = RectF {
            x: transform.offset.0,
            y: transform.offset.1,
            w: zoom.image.0 * scale,
            h: zoom.image.1 * scale,
        };
    }
    frame
}

fn sample_bounce(running: &Running, elapsed: Duration) -> Presented {
    let u = unit(elapsed, BOUNCE_DUR);
    let bump = if u < 0.5 {
        ease_out_quad(u * 2.0)
    } else {
        ease_out_quad((1.0 - u) * 2.0)
    };
    let Profile::Bounce(dir) = running.profile else {
        return running.from;
    };
    let dx = if dir < 0 { BOUNCE_PX } else { -BOUNCE_PX } * bump;
    let mut frame = running.from;
    if let Some(img) = frame.image.as_mut() {
        img.rect = shift_x(img.rect, dx);
    }
    frame
}

#[cfg(test)]
mod tests {
    use super::*;

    fn stage() -> (f32, f32) {
        (1000.0, 800.0)
    }

    fn fit() -> RectF {
        RectF {
            x: 100.0,
            y: 80.0,
            w: 400.0,
            h: 300.0,
        }
    }

    fn source() -> RectF {
        RectF {
            x: 10.0,
            y: 20.0,
            w: 80.0,
            h: 60.0,
        }
    }

    fn cell() -> RectF {
        RectF {
            x: 12.0,
            y: 40.0,
            w: 64.0,
            h: 48.0,
        }
    }

    fn approx(a: f32, b: f32) {
        assert!((a - b).abs() < 1.0e-3, "{a} vs {b}");
    }

    fn approx_rect(a: RectF, b: RectF) {
        approx(a.x, b.x);
        approx(a.y, b.y);
        approx(a.w, b.w);
        approx(a.h, b.h);
    }

    fn image(frame: &Presented) -> LayerState {
        frame.image.expect("image")
    }

    #[test]
    fn motion_easing_endpoints_and_monotonic() {
        let fns = [
            ease_out_cubic,
            ease_in_out_cubic,
            ease_out_quad,
            ease_in_cubic,
        ];
        for ease in fns {
            approx(ease(0.0), 0.0);
            approx(ease(1.0), 1.0);
            let mut prev = 0.0;
            for i in 0..=64 {
                let y = ease(i as f32 / 64.0);
                assert!(y + 1.0e-6 >= prev, "{y} < {prev}");
                prev = y;
            }
        }
    }

    #[test]
    fn viewer_open_from_thumbnail_interpolates_rect() {
        let mut m = Motion::new(MotionMode::Full, stage());
        m.open_from_thumbnail(Duration::ZERO, source(), fit());
        let start = image(&m.presented(Duration::ZERO));
        approx_rect(start.rect, source());
        approx(start.opacity, 1.0);
        approx(start.corner_radius, 6.0);
        approx(m.presented(Duration::ZERO).backdrop_opacity, 0.0);
        approx(m.presented(Duration::ZERO).chrome_opacity, 0.0);

        let mid = Duration::from_millis(130);
        let u = ease_out_cubic(0.5);
        let frame = m.presented(mid);
        approx_rect(image(&frame).rect, lerp_rect(source(), fit(), u));
        approx(image(&frame).corner_radius, lerp(6.0, 0.0, u));
        approx(frame.backdrop_opacity, ease_out_quad(130.0 / 200.0));
        approx(frame.chrome_opacity, 0.0);

        let end = m.presented(OPEN_THUMB);
        approx_rect(image(&end).rect, fit());
        approx(image(&end).corner_radius, 0.0);
        approx(end.backdrop_opacity, 1.0);
        approx(end.chrome_opacity, 1.0);
        let chrome_on = m.presented(Duration::from_millis(210));
        approx(chrome_on.chrome_opacity, (210.0 - 160.0) / 100.0);
    }

    #[test]
    fn viewer_open_without_source_scales_and_fades() {
        let mut m = Motion::new(MotionMode::Full, stage());
        m.open_plain(Duration::ZERO, fit());
        let start = image(&m.presented(Duration::ZERO));
        approx_rect(start.rect, scaled_about_center(fit(), 0.92));
        approx(start.opacity, 0.0);
        let mid = Duration::from_millis(110);
        let u = ease_out_cubic(0.5);
        let frame = m.presented(mid);
        approx_rect(
            image(&frame).rect,
            lerp_rect(scaled_about_center(fit(), 0.92), fit(), u),
        );
        approx(image(&frame).opacity, u);
        approx(frame.backdrop_opacity, ease_out_quad(110.0 / 180.0));
        approx(frame.chrome_opacity, 0.0);
        let end = m.presented(OPEN_PLAIN);
        approx_rect(image(&end).rect, fit());
        approx(image(&end).opacity, 1.0);
        approx(end.backdrop_opacity, 1.0);
        approx(end.chrome_opacity, 1.0);
    }

    #[test]
    fn viewer_close_returns_to_visible_thumbnail_rect() {
        let mut m = Motion::new(MotionMode::Full, stage());
        m.settle_open(fit());
        let t0 = Duration::from_millis(300);
        m.close_to_thumbnail(t0, cell());
        approx_rect(image(&m.presented(t0)).rect, fit());
        let mid = t0 + Duration::from_millis(110);
        let u = ease_in_out_cubic(0.5);
        let frame = m.presented(mid);
        approx_rect(image(&frame).rect, lerp_rect(fit(), cell(), u));
        approx(image(&frame).corner_radius, lerp(0.0, 6.0, u));
        approx(frame.backdrop_opacity, 0.5);
        approx(frame.chrome_opacity, 0.0);
        let end = m.presented(t0 + CLOSE_THUMB);
        approx_rect(image(&end).rect, cell());
        approx(image(&end).corner_radius, 6.0);
        approx(end.backdrop_opacity, 0.0);
        approx(end.chrome_opacity, 0.0);
    }

    #[test]
    fn viewer_close_fades_when_thumbnail_offscreen() {
        let mut m = Motion::new(MotionMode::Full, stage());
        m.settle_open(fit());
        let t0 = Duration::from_millis(300);
        m.close_fade(t0);
        let start = image(&m.presented(t0));
        approx_rect(start.rect, fit());
        approx(start.opacity, 1.0);
        let mid = t0 + Duration::from_millis(80);
        let u = ease_in_cubic(0.5);
        let frame = m.presented(mid);
        approx_rect(
            image(&frame).rect,
            lerp_rect(fit(), scaled_about_center(fit(), 0.96), u),
        );
        approx(image(&frame).opacity, lerp(1.0, 0.0, u));
        let end = m.presented(t0 + CLOSE_FADE);
        approx_rect(image(&end).rect, scaled_about_center(fit(), 0.96));
        approx(image(&end).opacity, 0.0);
        approx(end.backdrop_opacity, 0.0);
    }

    #[test]
    fn viewer_navigate_slides_and_crossfades_ambient() {
        let mut m = Motion::new(MotionMode::Full, stage());
        m.settle_open(fit());
        let t0 = Duration::from_millis(50);
        m.navigate(t0, 1, fit());
        let dx = NAV_SHIFT * stage().0;
        let start = m.presented(t0);
        approx_rect(image(&start).rect, shift_x(fit(), dx));
        approx(image(&start).opacity, 0.0);
        let outgoing = start.outgoing.expect("outgoing");
        approx_rect(outgoing.rect, fit());
        approx(outgoing.opacity, 1.0);
        approx(start.backdrop_crossfade, 0.0);
        approx(start.chrome_opacity, 1.0);

        let mid = t0 + Duration::from_millis(100);
        let u = ease_out_cubic(0.5);
        let frame = m.presented(mid);
        approx_rect(image(&frame).rect, lerp_rect(shift_x(fit(), dx), fit(), u));
        approx(image(&frame).opacity, u);
        let out = frame.outgoing.expect("outgoing");
        approx_rect(out.rect, lerp_rect(fit(), shift_x(fit(), -dx), u));
        approx(out.opacity, 1.0 - u);
        approx(frame.backdrop_crossfade, 100.0 / 320.0);
        approx(frame.chrome_opacity, 1.0);

        let slide_done = m.presented(t0 + NAV_SLIDE);
        approx_rect(image(&slide_done).rect, fit());
        approx(image(&slide_done).opacity, 1.0);
        approx(slide_done.backdrop_crossfade, 200.0 / 320.0);
        let end = m.presented(t0 + NAV_CROSS);
        approx(end.backdrop_crossfade, 1.0);
        approx(end.outgoing.expect("outgoing").opacity, 0.0);
    }

    #[test]
    fn viewer_rapid_navigation_skips_slide() {
        let mut m = Motion::new(MotionMode::Full, stage());
        m.settle_open(fit());
        let t0 = Duration::from_millis(20);
        m.navigate(t0, 1, fit());
        let t1 = t0 + Duration::from_millis(100);
        let next = RectF {
            x: 120.0,
            y: 90.0,
            w: 360.0,
            h: 280.0,
        };
        m.navigate(t1, 1, next);
        let frame = m.presented(t1);
        approx_rect(image(&frame).rect, next);
        approx(image(&frame).opacity, 1.0);
        assert!(frame.outgoing.is_none(), "rapid navigation does not slide");
        approx(frame.backdrop_crossfade, 0.0);
        let mid = m.presented(t1 + Duration::from_millis(60));
        approx_rect(image(&mid).rect, next);
        approx(mid.backdrop_crossfade, 0.5);
        approx(m.presented(t1 + RAPID_CROSS).backdrop_crossfade, 1.0);
    }

    #[test]
    fn viewer_reduced_motion_uses_crossfade() {
        let mut m = Motion::new(MotionMode::Reduced, stage());
        m.open_from_thumbnail(Duration::ZERO, source(), fit());
        let start = image(&m.presented(Duration::ZERO));
        approx_rect(start.rect, fit());
        approx(start.opacity, 0.0);
        let mid = image(&m.presented(Duration::from_millis(60)));
        approx_rect(mid.rect, fit());
        approx(mid.opacity, 0.5);
        let end = image(&m.presented(REDUCED_FADE));
        approx_rect(end.rect, fit());
        approx(end.opacity, 1.0);
        approx(m.presented(Duration::ZERO).chrome_opacity, 0.0);
        approx(m.presented(OPEN_THUMB).chrome_opacity, 1.0);
    }

    #[test]
    fn viewer_motion_off_is_instant() {
        let mut m = Motion::new(MotionMode::Off, stage());
        m.open_from_thumbnail(Duration::ZERO, source(), fit());
        let frame = m.presented(Duration::ZERO);
        approx_rect(image(&frame).rect, fit());
        approx(image(&frame).opacity, 1.0);
        approx(frame.backdrop_opacity, 1.0);
        approx(frame.chrome_opacity, 1.0);
        assert!(!m.is_animating(Duration::ZERO));
        m.close_fade(Duration::from_millis(10));
        let closed = m.presented(Duration::from_millis(10));
        approx(image(&closed).opacity, 0.0);
        approx(closed.backdrop_opacity, 0.0);
        assert!(!m.is_animating(Duration::from_millis(10)));
    }

    #[test]
    fn viewer_chrome_autohides_after_idle_and_returns_on_move() {
        let mut m = Motion::new(MotionMode::Full, stage());
        m.open_from_thumbnail(Duration::ZERO, source(), fit());
        approx(m.presented(OPEN_THUMB).chrome_opacity, 1.0);
        approx(m.presented(CHROME_IDLE).chrome_opacity, 1.0);
        let mid = CHROME_IDLE + Duration::from_millis(100);
        approx(m.presented(mid).chrome_opacity, 0.5);
        let hidden = CHROME_IDLE + CHROME_OUT;
        approx(m.presented(hidden).chrome_opacity, 0.0);
        m.pointer_moved(hidden);
        approx(m.presented(hidden).chrome_opacity, 0.0);
        approx(
            m.presented(hidden + Duration::from_millis(60))
                .chrome_opacity,
            0.5,
        );
        approx(m.presented(hidden + CHROME_IN).chrome_opacity, 1.0);
        m.key_pressed(hidden + Duration::from_secs(10));
        let later = hidden + Duration::from_secs(10) + CHROME_IDLE + Duration::from_millis(100);
        approx(m.presented(later).chrome_opacity, 0.5);
    }

    #[test]
    fn viewer_not_animating_when_idle() {
        let mut m = Motion::new(MotionMode::Full, stage());
        m.open_from_thumbnail(Duration::ZERO, source(), fit());
        assert!(m.is_animating(Duration::from_millis(100)));
        assert!(!m.is_animating(OPEN_THUMB));
        assert!(!m.is_animating(Duration::from_millis(1_000)));
        assert!(m.is_animating(CHROME_IDLE + Duration::from_millis(100)));
        assert!(!m.is_animating(CHROME_IDLE + CHROME_OUT));
    }

    #[test]
    fn viewer_retarget_is_continuous() {
        use proptest::prelude::*;
        proptest!(|(interrupt_ms in 0u64..259, kind in 0u8..5)| {
            let mut motion = Motion::new(MotionMode::Full, stage());
            motion.open_from_thumbnail(Duration::ZERO, source(), fit());
            let t = Duration::from_millis(interrupt_ms);
            let before = motion.presented(t);
            match kind {
                0 => motion.close_to_thumbnail(t, cell()),
                1 => motion.close_fade(t),
                2 => motion.navigate(t, 1, fit()),
                3 => motion.zoom_to(t, (400.0, 300.0), (200.0, 150.0), 2.0),
                _ => motion.refit(t, RectF { x: 40.0, y: 40.0, w: 500.0, h: 400.0 }),
            }
            let after = motion.presented(t);
            prop_assert_eq!(before, after);
        });
    }
}
