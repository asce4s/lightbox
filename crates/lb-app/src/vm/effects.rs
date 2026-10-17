//! The twelve effect tiles of the Effects tab and their parameter panels (SPEC §9.5).

use lb_core::EditOp;

/// One slider of an effect's parameter panel.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ParamSpec {
    /// Stable key used in element ids.
    pub key: &'static str,
    /// Slider label.
    pub label: &'static str,
    /// Smallest value.
    pub min: f32,
    /// Largest value.
    pub max: f32,
    /// Value the panel opens with.
    pub default: f32,
}

const fn p(key: &'static str, label: &'static str, min: f32, max: f32, default: f32) -> ParamSpec {
    ParamSpec {
        key,
        label,
        min,
        max,
        default,
    }
}

/// An effect tile. `key` equals the snake_case op name.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EffectSpec {
    /// Op name and element-id suffix.
    pub key: &'static str,
    /// Tile caption.
    pub label: &'static str,
    /// Sliders; empty for effects that apply on click.
    pub params: &'static [ParamSpec],
}

/// All effects in tile order.
pub const EFFECTS: [EffectSpec; 12] = [
    EffectSpec {
        key: "sharpen",
        label: "Sharpen",
        params: &[p("amount", "Amount", 0.0, 2.0, 0.8)],
    },
    EffectSpec {
        key: "sepia",
        label: "Sepia",
        params: &[],
    },
    EffectSpec {
        key: "black_and_white",
        label: "B&W",
        params: &[],
    },
    EffectSpec {
        key: "warmify",
        label: "Warmify",
        params: &[],
    },
    EffectSpec {
        key: "film_grain",
        label: "Film Grain",
        params: &[p("amount", "Amount", 0.0, 1.0, 0.4)],
    },
    EffectSpec {
        key: "tint",
        label: "Tint",
        params: &[
            p("hue", "Hue", 0.0, 360.0, 30.0),
            p("preserve", "Keep colors", 0.0, 1.0, 0.5),
        ],
    },
    EffectSpec {
        key: "saturation",
        label: "Saturation",
        params: &[p("amount", "Amount", -1.0, 1.0, 0.4)],
    },
    EffectSpec {
        key: "soft_focus",
        label: "Soft Focus",
        params: &[
            p("radius", "Sharp area", 0.0, 1.0, 0.3),
            p("amount", "Blur", 0.0, 1.0, 0.6),
        ],
    },
    EffectSpec {
        key: "glow",
        label: "Glow",
        params: &[
            p("intensity", "Intensity", 0.0, 1.0, 0.5),
            p("radius", "Spread", 0.0, 5.0, 2.0),
        ],
    },
    EffectSpec {
        key: "focal_black_and_white",
        label: "Focal B&W",
        params: &[
            p("radius", "Color area", 0.0, 1.0, 0.3),
            p("sharpness", "Sharpness", 0.0, 1.0, 0.5),
        ],
    },
    EffectSpec {
        key: "graduated_tint",
        label: "Graduated Tint",
        params: &[
            p("hue", "Hue", 0.0, 360.0, 210.0),
            p("y", "Position", 0.0, 1.0, 0.4),
            p("feather", "Feather", 0.0, 1.0, 0.3),
            p("amount", "Strength", 0.0, 1.0, 0.6),
        ],
    },
    EffectSpec {
        key: "vignette",
        label: "Vignette",
        params: &[
            p("amount", "Amount", 0.0, 1.0, 0.5),
            p("radius", "Radius", 0.0, 1.0, 0.6),
        ],
    },
];

/// The effect with op name `key`.
pub fn effect(key: &str) -> Option<&'static EffectSpec> {
    EFFECTS.iter().find(|e| e.key == key)
}

/// A saturated colour for a hue in degrees, as sRGB `0..=1`.
pub fn hue_to_rgb(hue: f32) -> [f32; 3] {
    let h = (hue.rem_euclid(360.0)) / 60.0;
    let (s, v) = (0.7_f32, 0.9_f32);
    let c = v * s;
    let x = c * (1.0 - (h % 2.0 - 1.0).abs());
    let (r, g, b) = match h as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = v - c;
    [r + m, g + m, b + m]
}

impl EffectSpec {
    /// Whether the effect has a parameter panel.
    pub fn is_parametrized(&self) -> bool {
        !self.params.is_empty()
    }

    /// The default value of every parameter.
    pub fn defaults(&self) -> Vec<f32> {
        self.params.iter().map(|p| p.default).collect()
    }

    /// The op for parameter `values` (in `params` order; missing values use the default).
    pub fn build(&self, values: &[f32]) -> EditOp {
        let v = |i: usize| {
            values
                .get(i)
                .copied()
                .or_else(|| self.params.get(i).map(|p| p.default))
                .unwrap_or(0.0)
        };
        match self.key {
            "sharpen" => EditOp::Sharpen { amount: v(0) },
            "sepia" => EditOp::Sepia,
            "black_and_white" => EditOp::BlackAndWhite,
            "film_grain" => EditOp::FilmGrain { amount: v(0) },
            "tint" => EditOp::Tint {
                color: hue_to_rgb(v(0)),
                preserve: v(1),
            },
            "saturation" => EditOp::Saturation { amount: v(0) },
            "soft_focus" => EditOp::SoftFocus {
                cx: 0.5,
                cy: 0.5,
                radius: v(0),
                amount: v(1),
            },
            "glow" => EditOp::Glow {
                intensity: v(0),
                radius: v(1),
            },
            "focal_black_and_white" => EditOp::FocalBlackAndWhite {
                cx: 0.5,
                cy: 0.5,
                radius: v(0),
                sharpness: v(1),
            },
            "graduated_tint" => EditOp::GraduatedTint {
                color: hue_to_rgb(v(0)),
                y: v(1),
                feather: v(2),
                amount: v(3),
            },
            "vignette" => EditOp::Vignette {
                amount: v(0),
                radius: v(1),
            },
            _ => EditOp::Warmify,
        }
    }
}

/// An open parameter panel.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectPanelVm {
    spec: &'static EffectSpec,
    values: Vec<f32>,
}

impl EffectPanelVm {
    /// Opens the panel of `key` with default values.
    pub fn open(key: &str) -> Option<Self> {
        let spec = effect(key)?;
        Some(Self {
            spec,
            values: spec.defaults(),
        })
    }

    /// The effect.
    pub fn spec(&self) -> &'static EffectSpec {
        self.spec
    }

    /// Current parameter values.
    pub fn values(&self) -> &[f32] {
        &self.values
    }

    /// Sets parameter `index`, clamped to its range.
    pub fn set(&mut self, index: usize, value: f32) {
        if let (Some(spec), Some(slot)) = (self.spec.params.get(index), self.values.get_mut(index))
        {
            *slot = value.clamp(spec.min, spec.max);
        }
    }

    /// The op Apply would store.
    pub fn op(&self) -> EditOp {
        self.spec.build(&self.values)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn effect_table_matches_op_names_and_validates() {
        assert_eq!(EFFECTS.len(), 12);
        for e in EFFECTS {
            let op = e.build(&e.defaults());
            assert_eq!(op.name(), e.key, "{}", e.label);
            assert_eq!(
                op.validate().ok(),
                Some(op.clone()),
                "{} defaults valid",
                e.key
            );
            let mut panel = EffectPanelVm::open(e.key).unwrap_or_else(|| unreachable!());
            for (i, spec) in e.params.iter().enumerate() {
                panel.set(i, spec.max + 100.0);
                assert!(panel.values()[i] <= spec.max);
            }
            assert!(panel.op().validate().is_ok());
        }
    }

    #[test]
    fn hue_to_rgb_is_in_gamut() {
        for h in [0.0, 59.0, 120.0, 200.0, 300.0, 359.0, -30.0, 720.0] {
            assert!(hue_to_rgb(h).iter().all(|c| (0.0..=1.0).contains(c)), "{h}");
        }
    }
}
