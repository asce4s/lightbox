//! Mapping of Picasa `filters=` chains to edit operations (SPEC §6.17).

use crate::rect::{decode_hex_u64, decode_rect64};
use lb_core::EditOp;

/// The edit operations a `filters=` value maps to, in order, and the filters that were skipped.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FilterOutcome {
    /// Operations in the order Picasa applied them.
    pub ops: Vec<EditOp>,
    /// Names of filters that are unknown or malformed.
    pub skipped: Vec<String>,
}

fn nums(args: &[&str]) -> Vec<Option<f32>> {
    args.iter().map(|a| a.trim().parse::<f32>().ok()).collect()
}

fn arg(values: &[Option<f32>], i: usize) -> Option<f32> {
    values.get(i).copied().flatten()
}

/// The sampled neutral colour of `finetune2`: `AARRGGBB`; opaque white means "none".
fn neutral(hex: &str) -> Option<[f32; 3]> {
    let v = decode_hex_u64(hex).filter(|_| hex.len() <= 8)?;
    let (r, g, b) = ((v >> 16) & 0xff, (v >> 8) & 0xff, v & 0xff);
    if (r, g, b) == (255, 255, 255) {
        return None;
    }
    Some([r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0])
}

fn map_one(name: &str, args: &[&str]) -> Option<Option<EditOp>> {
    let values = nums(args);
    let enabled = args.first().is_some_and(|a| a.trim() != "0");
    Some(match name {
        "crop64" => {
            let r = decode_rect64(args.get(1)?)?;
            Some(EditOp::Crop {
                x: r.x,
                y: r.y,
                w: r.w,
                h: r.h,
            })
        }
        "enhance" => enabled.then_some(EditOp::FeelingLucky),
        "autolight" => enabled.then_some(EditOp::AutoContrast),
        "autocolor" => enabled.then_some(EditOp::AutoColor),
        "finetune2" => {
            if !enabled {
                return Some(None);
            }
            Some(EditOp::Tuning {
                fill_light: arg(&values, 1)?,
                highlights: arg(&values, 2)?,
                shadows: arg(&values, 3)?,
                neutral: args.get(4).and_then(|h| neutral(h)),
                temperature: arg(&values, 5).unwrap_or(0.0),
            })
        }
        "bw" => enabled.then_some(EditOp::BlackAndWhite),
        "sepia" => enabled.then_some(EditOp::Sepia),
        "warm" => enabled.then_some(EditOp::Warmify),
        "sat" => enabled.then(|| EditOp::Saturation {
            amount: arg(&values, 1).unwrap_or(0.0),
        }),
        "fill" => enabled.then(|| EditOp::FillLight {
            amount: arg(&values, 1).unwrap_or(0.0),
        }),
        "radblur" => enabled.then(|| EditOp::SoftFocus {
            cx: arg(&values, 1).unwrap_or(0.5),
            cy: arg(&values, 2).unwrap_or(0.5),
            radius: arg(&values, 3).unwrap_or(0.4),
            amount: arg(&values, 4).unwrap_or(0.5),
        }),
        "glow2" => enabled.then(|| EditOp::Glow {
            intensity: arg(&values, 1).unwrap_or(0.5),
            radius: arg(&values, 2).unwrap_or(0.5),
        }),
        "ansel" => enabled.then(|| EditOp::FocalBlackAndWhite {
            cx: arg(&values, 1).unwrap_or(0.5),
            cy: arg(&values, 2).unwrap_or(0.5),
            radius: arg(&values, 3).unwrap_or(0.35),
            sharpness: arg(&values, 4).unwrap_or(0.5),
        }),
        _ => return None,
    })
}

/// Maps a `filters=` value such as `crop64=1,30a2...;enhance=1;` to operations. Filters that are
/// not understood are skipped and listed in [`FilterOutcome::skipped`]; operations are validated
/// (clamped) by the importer, not here.
pub fn map_filters(value: &str) -> FilterOutcome {
    let mut out = FilterOutcome::default();
    for item in value.split(';').map(str::trim).filter(|s| !s.is_empty()) {
        let (name, rest) = item.split_once('=').unwrap_or((item, ""));
        let args: Vec<&str> = rest.split(',').collect();
        match map_one(name.trim(), &args) {
            Some(Some(op)) => out.ops.push(op),
            Some(None) => {}
            None => out.skipped.push(name.trim().to_owned()),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn crop64_maps_to_crop() {
        let out = map_filters("crop64=1,30a245ce99b7dbb2;");
        assert_eq!(out.skipped, Vec::<String>::new());
        let [EditOp::Crop { x, y, w, h }] = out.ops.as_slice() else {
            panic!("{:?}", out.ops);
        };
        assert!((x - 0x30a2 as f32 / 65535.0).abs() < 1e-6);
        assert!((y - 0x45ce as f32 / 65535.0).abs() < 1e-6);
        assert!((x + w - 0x99b7 as f32 / 65535.0).abs() < 1e-6);
        assert!((y + h - 0xdbb2 as f32 / 65535.0).abs() < 1e-6);
        let bad = map_filters("crop64=1,00000000;crop64=1");
        assert!(bad.ops.is_empty());
        assert_eq!(bad.skipped, ["crop64", "crop64"]);
    }

    #[test]
    fn finetune2_maps_to_tuning() {
        let out = map_filters("finetune2=1,0.100000,0.200000,0.190000,fff7f5f3,-0.500000;");
        let [
            EditOp::Tuning {
                fill_light,
                highlights,
                shadows,
                temperature,
                neutral,
            },
        ] = out.ops.as_slice()
        else {
            panic!("{:?}", out.ops);
        };
        assert_eq!(
            (*fill_light, *highlights, *shadows, *temperature),
            (0.1, 0.2, 0.19, -0.5)
        );
        let n = neutral.unwrap();
        assert!((n[0] - 247.0 / 255.0).abs() < 1e-6);
        assert!((n[1] - 245.0 / 255.0).abs() < 1e-6);
        assert!((n[2] - 243.0 / 255.0).abs() < 1e-6);
        let white = map_filters("finetune2=1,0,0,0,ffffffff,0");
        assert!(matches!(
            white.ops.as_slice(),
            [EditOp::Tuning { neutral: None, .. }]
        ));
        assert!(map_filters("finetune2=1,0,0").ops.is_empty());
    }

    #[test]
    fn every_documented_filter_maps_in_order() {
        let out = map_filters(
            "bw=1;sepia=1;warm=1;sat=1,0.500000;fill=1,0.300000;radblur=1,0.5,0.5,0.4,0.5;glow2=1,0.5,0.5;ansel=1;madeupfilter=1,2,3;enhance=1;autolight=1;autocolor=1;",
        );
        assert_eq!(out.skipped, ["madeupfilter"]);
        let names: Vec<&str> = out.ops.iter().map(EditOp::name).collect();
        assert_eq!(
            names,
            [
                "black_and_white",
                "sepia",
                "warmify",
                "saturation",
                "fill_light",
                "soft_focus",
                "glow",
                "focal_black_and_white",
                "feeling_lucky",
                "auto_contrast",
                "auto_color"
            ]
        );
        assert_eq!(out.ops[3], EditOp::Saturation { amount: 0.5 });
        assert_eq!(out.ops[4], EditOp::FillLight { amount: 0.3 });
    }

    #[test]
    fn disabled_filters_are_ignored() {
        let out = map_filters("bw=0;enhance=0;");
        assert!(out.ops.is_empty() && out.skipped.is_empty());
        assert_eq!(map_filters("").ops.len(), 0);
    }
}
