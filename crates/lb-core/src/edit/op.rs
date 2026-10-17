//! Edit operation types (SPEC §6.6.1).

use serde::{Deserialize, Serialize};

/// An axis-aligned rectangle in coordinates normalized to the image (`0..=1`).
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub struct NormRect {
    /// Left edge.
    pub x: f32,
    /// Top edge.
    pub y: f32,
    /// Width.
    pub w: f32,
    /// Height.
    pub h: f32,
}

/// One clone-heal spot: the disk at `(src_x, src_y)` is copied onto `(dst_x, dst_y)`.
/// Centres are normalized to width/height; `radius` is normalized to the long edge.
#[derive(Serialize, Deserialize, Clone, Copy, PartialEq, Debug)]
pub struct RetouchSpot {
    /// Source centre, x.
    pub src_x: f32,
    /// Source centre, y.
    pub src_y: f32,
    /// Target centre, x.
    pub dst_x: f32,
    /// Target centre, y.
    pub dst_y: f32,
    /// Disk radius as a fraction of the long edge.
    pub radius: f32,
}

/// A single non-destructive edit. Operations apply in stack order (SPEC §6.6.1, §6.6.3).
#[derive(Serialize, Deserialize, Clone, PartialEq, Debug)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum EditOp {
    /// Crop to a rectangle normalized to the image entering this op.
    Crop {
        /// Left edge.
        x: f32,
        /// Top edge.
        y: f32,
        /// Width.
        w: f32,
        /// Height.
        h: f32,
    },
    /// Rotate by quarter turns (clockwise positive): ±1, ±2.
    RotateQuarter {
        /// Number of quarter turns.
        turns: i8,
    },
    /// Rotate content about the centre, then crop away the empty corners. Degrees in `[-45, 45]`.
    Straighten {
        /// Clockwise degrees.
        degrees: f32,
    },
    /// Remove red eye inside the given ellipses.
    RedEye {
        /// Bounding rectangles of the eyes.
        regions: Vec<NormRect>,
    },
    /// Stretch luma to the 0.5–99.5 percentile range.
    AutoContrast,
    /// Per-channel stretch and mid-tone balance.
    AutoColor,
    /// `AutoContrast` then `AutoColor`.
    FeelingLucky,
    /// Clone-heal spots (P1).
    Retouch {
        /// The spots, applied in order.
        spots: Vec<RetouchSpot>,
    },
    /// Brighten shadows.
    FillLight {
        /// Strength `[0, 1]`.
        amount: f32,
    },
    /// The Tuning tab's sliders as one op.
    Tuning {
        /// Fill light `[0, 1]`.
        fill_light: f32,
        /// Highlights recovery `[0, 1]`.
        highlights: f32,
        /// Shadows `[0, 1]`.
        shadows: f32,
        /// Colour temperature `[-1, 1]`.
        temperature: f32,
        /// Sampled neutral colour (sRGB), if any.
        #[serde(default)]
        neutral: Option<[f32; 3]>,
    },
    /// Unsharp mask.
    Sharpen {
        /// Strength `[0, 2]`.
        amount: f32,
    },
    /// Sepia tone.
    Sepia,
    /// Black and white.
    BlackAndWhite,
    /// Warm tone plus a little saturation.
    Warmify,
    /// Seeded film grain.
    FilmGrain {
        /// Strength `[0, 1]`.
        amount: f32,
    },
    /// Tint towards a colour.
    Tint {
        /// Tint colour (sRGB).
        color: [f32; 3],
        /// How much of the original colour to keep `[0, 1]`.
        preserve: f32,
    },
    /// Saturation change.
    Saturation {
        /// Amount `[-1, 1]`.
        amount: f32,
    },
    /// Blur outside a focus circle.
    SoftFocus {
        /// Focus centre x (normalized).
        cx: f32,
        /// Focus centre y (normalized).
        cy: f32,
        /// Radius of the sharp area, normalized to the long edge.
        radius: f32,
        /// Blur strength `[0, 1]`.
        amount: f32,
    },
    /// Highlight bloom.
    Glow {
        /// Strength `[0, 1]`.
        intensity: f32,
        /// Spread `[0, 5]`.
        radius: f32,
    },
    /// Keep colour only inside a circle.
    FocalBlackAndWhite {
        /// Centre x (normalized).
        cx: f32,
        /// Centre y (normalized).
        cy: f32,
        /// Radius of the coloured area, normalized to the long edge.
        radius: f32,
        /// Edge sharpness `[0, 1]`.
        sharpness: f32,
    },
    /// Colour gradient from the top down to `y`.
    GraduatedTint {
        /// Tint colour (sRGB).
        color: [f32; 3],
        /// Row where the tint has half faded, normalized `[0, 1]`.
        y: f32,
        /// Half-width of the fade `[0, 1]`.
        feather: f32,
        /// Strength `[0, 1]`.
        amount: f32,
    },
    /// Darken the corners (P1).
    Vignette {
        /// Strength `[0, 1]`.
        amount: f32,
        /// Radius where darkening starts `[0, 1]`.
        radius: f32,
    },
}

impl EditOp {
    /// The snake_case operation name used in JSON and by the CLI.
    pub fn name(&self) -> &'static str {
        match self {
            Self::Crop { .. } => "crop",
            Self::RotateQuarter { .. } => "rotate_quarter",
            Self::Straighten { .. } => "straighten",
            Self::RedEye { .. } => "red_eye",
            Self::AutoContrast => "auto_contrast",
            Self::AutoColor => "auto_color",
            Self::FeelingLucky => "feeling_lucky",
            Self::Retouch { .. } => "retouch",
            Self::FillLight { .. } => "fill_light",
            Self::Tuning { .. } => "tuning",
            Self::Sharpen { .. } => "sharpen",
            Self::Sepia => "sepia",
            Self::BlackAndWhite => "black_and_white",
            Self::Warmify => "warmify",
            Self::FilmGrain { .. } => "film_grain",
            Self::Tint { .. } => "tint",
            Self::Saturation { .. } => "saturation",
            Self::SoftFocus { .. } => "soft_focus",
            Self::Glow { .. } => "glow",
            Self::FocalBlackAndWhite { .. } => "focal_black_and_white",
            Self::GraduatedTint { .. } => "graduated_tint",
            Self::Vignette { .. } => "vignette",
        }
    }
}
