//! Validation and clamping of edit parameters (SPEC §6.6.1).

use crate::edit::{EditOp, NormRect, RetouchSpot};

/// Why an edit operation was rejected.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum EditError {
    /// A parameter is NaN or infinite.
    #[error("{op}: `{field}` is not a finite number")]
    NotFinite {
        /// Operation name.
        op: &'static str,
        /// Parameter name.
        field: &'static str,
    },
    /// A rectangle has no area or lies outside the image.
    #[error("{op}: {reason}")]
    BadRect {
        /// Operation name.
        op: &'static str,
        /// What is wrong.
        reason: &'static str,
    },
    /// `RotateQuarter` turns must be ±1 or ±2.
    #[error("rotate_quarter: turns must be -2, -1, 1 or 2 (got {0})")]
    BadTurns(i8),
    /// Too many regions or spots in one op.
    #[error("{op}: at most {max} entries are allowed (got {got})")]
    TooMany {
        /// Operation name.
        op: &'static str,
        /// The limit.
        max: usize,
        /// What was supplied.
        got: usize,
    },
    /// A list-valued op has no entries.
    #[error("{0}: needs at least one entry")]
    Empty(&'static str),
}

/// Most regions or spots one op may carry.
const MAX_ENTRIES: usize = 64;
/// Slack for float error in crop bounds.
const RECT_EPS: f32 = 1e-4;

fn finite(op: &'static str, field: &'static str, v: f32) -> Result<f32, EditError> {
    if v.is_finite() {
        Ok(v)
    } else {
        Err(EditError::NotFinite { op, field })
    }
}

fn clamped(
    op: &'static str,
    field: &'static str,
    v: f32,
    lo: f32,
    hi: f32,
) -> Result<f32, EditError> {
    finite(op, field, v).map(|v| v.clamp(lo, hi))
}

fn color(op: &'static str, field: &'static str, c: [f32; 3]) -> Result<[f32; 3], EditError> {
    Ok([
        clamped(op, field, c[0], 0.0, 1.0)?,
        clamped(op, field, c[1], 0.0, 1.0)?,
        clamped(op, field, c[2], 0.0, 1.0)?,
    ])
}

fn rect(
    op: &'static str,
    (x, y, w, h): (f32, f32, f32, f32),
) -> Result<(f32, f32, f32, f32), EditError> {
    let (x, y) = (finite(op, "x", x)?, finite(op, "y", y)?);
    let (w, h) = (finite(op, "w", w)?, finite(op, "h", h)?);
    if w <= 0.0 || h <= 0.0 {
        return Err(EditError::BadRect {
            op,
            reason: "width and height must be positive",
        });
    }
    if x < -RECT_EPS || y < -RECT_EPS || x + w > 1.0 + RECT_EPS || y + h > 1.0 + RECT_EPS {
        return Err(EditError::BadRect {
            op,
            reason: "rectangle must lie within [0, 1]",
        });
    }
    let (x, y) = (x.clamp(0.0, 1.0), y.clamp(0.0, 1.0));
    Ok((x, y, w.min(1.0 - x), h.min(1.0 - y)))
}

impl EditOp {
    /// Returns the operation with out-of-range parameters clamped, or an error when the op is
    /// unusable: non-finite numbers, a crop or region with no area or outside `[0, 1]`, bad
    /// quarter-turn counts, or empty/oversized region lists.
    pub fn validate(&self) -> Result<EditOp, EditError> {
        let name = self.name();
        Ok(match self {
            Self::Crop { x, y, w, h } => {
                let (x, y, w, h) = rect(name, (*x, *y, *w, *h))?;
                Self::Crop { x, y, w, h }
            }
            Self::RotateQuarter { turns } => {
                if !matches!(turns, -2 | -1 | 1 | 2) {
                    return Err(EditError::BadTurns(*turns));
                }
                self.clone()
            }
            Self::Straighten { degrees } => Self::Straighten {
                degrees: clamped(name, "degrees", *degrees, -45.0, 45.0)?,
            },
            Self::RedEye { regions } => {
                list_len(name, regions.len())?;
                let regions = regions
                    .iter()
                    .map(|r| {
                        rect(name, (r.x, r.y, r.w, r.h)).map(|(x, y, w, h)| NormRect { x, y, w, h })
                    })
                    .collect::<Result<_, _>>()?;
                Self::RedEye { regions }
            }
            Self::AutoContrast | Self::AutoColor | Self::FeelingLucky => self.clone(),
            Self::Sepia | Self::BlackAndWhite | Self::Warmify => self.clone(),
            Self::Retouch { spots } => {
                list_len(name, spots.len())?;
                let spots = spots
                    .iter()
                    .map(|s| spot(name, s))
                    .collect::<Result<_, _>>()?;
                Self::Retouch { spots }
            }
            Self::FillLight { amount } => Self::FillLight {
                amount: clamped(name, "amount", *amount, 0.0, 1.0)?,
            },
            Self::Tuning {
                fill_light,
                highlights,
                shadows,
                temperature,
                neutral,
            } => Self::Tuning {
                fill_light: clamped(name, "fill_light", *fill_light, 0.0, 1.0)?,
                highlights: clamped(name, "highlights", *highlights, 0.0, 1.0)?,
                shadows: clamped(name, "shadows", *shadows, 0.0, 1.0)?,
                temperature: clamped(name, "temperature", *temperature, -1.0, 1.0)?,
                neutral: neutral.map(|n| color(name, "neutral", n)).transpose()?,
            },
            Self::Sharpen { amount } => Self::Sharpen {
                amount: clamped(name, "amount", *amount, 0.0, 2.0)?,
            },
            Self::FilmGrain { amount } => Self::FilmGrain {
                amount: clamped(name, "amount", *amount, 0.0, 1.0)?,
            },
            Self::Tint { color: c, preserve } => Self::Tint {
                color: color(name, "color", *c)?,
                preserve: clamped(name, "preserve", *preserve, 0.0, 1.0)?,
            },
            Self::Saturation { amount } => Self::Saturation {
                amount: clamped(name, "amount", *amount, -1.0, 1.0)?,
            },
            Self::SoftFocus {
                cx,
                cy,
                radius,
                amount,
            } => Self::SoftFocus {
                cx: clamped(name, "cx", *cx, 0.0, 1.0)?,
                cy: clamped(name, "cy", *cy, 0.0, 1.0)?,
                radius: clamped(name, "radius", *radius, 0.0, 2.0)?,
                amount: clamped(name, "amount", *amount, 0.0, 1.0)?,
            },
            Self::Glow { intensity, radius } => Self::Glow {
                intensity: clamped(name, "intensity", *intensity, 0.0, 1.0)?,
                radius: clamped(name, "radius", *radius, 0.0, 5.0)?,
            },
            Self::FocalBlackAndWhite {
                cx,
                cy,
                radius,
                sharpness,
            } => Self::FocalBlackAndWhite {
                cx: clamped(name, "cx", *cx, 0.0, 1.0)?,
                cy: clamped(name, "cy", *cy, 0.0, 1.0)?,
                radius: clamped(name, "radius", *radius, 0.0, 2.0)?,
                sharpness: clamped(name, "sharpness", *sharpness, 0.0, 1.0)?,
            },
            Self::GraduatedTint {
                color: c,
                y,
                feather,
                amount,
            } => Self::GraduatedTint {
                color: color(name, "color", *c)?,
                y: clamped(name, "y", *y, 0.0, 1.0)?,
                feather: clamped(name, "feather", *feather, 0.0, 1.0)?,
                amount: clamped(name, "amount", *amount, 0.0, 1.0)?,
            },
            Self::Vignette { amount, radius } => Self::Vignette {
                amount: clamped(name, "amount", *amount, 0.0, 1.0)?,
                radius: clamped(name, "radius", *radius, 0.0, 1.0)?,
            },
        })
    }
}

fn list_len(op: &'static str, len: usize) -> Result<(), EditError> {
    if len == 0 {
        Err(EditError::Empty(op))
    } else if len > MAX_ENTRIES {
        Err(EditError::TooMany {
            op,
            max: MAX_ENTRIES,
            got: len,
        })
    } else {
        Ok(())
    }
}

fn spot(op: &'static str, s: &RetouchSpot) -> Result<RetouchSpot, EditError> {
    Ok(RetouchSpot {
        src_x: clamped(op, "src_x", s.src_x, 0.0, 1.0)?,
        src_y: clamped(op, "src_y", s.src_y, 0.0, 1.0)?,
        dst_x: clamped(op, "dst_x", s.dst_x, 0.0, 1.0)?,
        dst_y: clamped(op, "dst_y", s.dst_y, 0.0, 1.0)?,
        radius: clamped(op, "radius", s.radius, 0.001, 0.5)?,
    })
}
