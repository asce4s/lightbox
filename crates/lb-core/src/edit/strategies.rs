//! `proptest` strategies for edit types. Available to tests via the `test-strategies` feature.

use crate::edit::{EditOp, EditStack, NormRect, RetouchSpot};
use proptest::prelude::*;

fn unit() -> impl Strategy<Value = f32> {
    0.0_f32..=1.0
}

fn color() -> impl Strategy<Value = [f32; 3]> {
    [unit(), unit(), unit()]
}

fn rect() -> impl Strategy<Value = NormRect> {
    (0.0_f32..0.9, 0.0_f32..0.9, 0.05_f32..0.5, 0.05_f32..0.5).prop_map(|(x, y, w, h)| NormRect {
        x,
        y,
        w: w.min(1.0 - x),
        h: h.min(1.0 - y),
    })
}

fn spot() -> impl Strategy<Value = RetouchSpot> {
    (unit(), unit(), unit(), unit(), 0.005_f32..0.2).prop_map(
        |(src_x, src_y, dst_x, dst_y, radius)| RetouchSpot {
            src_x,
            src_y,
            dst_x,
            dst_y,
            radius,
        },
    )
}

/// Any operation with every parameter inside its valid range.
pub fn edit_op() -> impl Strategy<Value = EditOp> {
    let geometry = prop_oneof![
        rect().prop_map(|r| EditOp::Crop {
            x: r.x,
            y: r.y,
            w: r.w,
            h: r.h
        }),
        prop_oneof![Just(-2_i8), Just(-1), Just(1), Just(2)]
            .prop_map(|turns| EditOp::RotateQuarter { turns }),
        (-45.0_f32..=45.0).prop_map(|degrees| EditOp::Straighten { degrees }),
        prop::collection::vec(rect(), 1..4).prop_map(|regions| EditOp::RedEye { regions }),
        prop::collection::vec(spot(), 1..4).prop_map(|spots| EditOp::Retouch { spots }),
    ];
    let auto = prop_oneof![
        Just(EditOp::AutoContrast),
        Just(EditOp::AutoColor),
        Just(EditOp::FeelingLucky),
        unit().prop_map(|amount| EditOp::FillLight { amount }),
        (
            unit(),
            unit(),
            unit(),
            -1.0_f32..=1.0,
            prop::option::of(color())
        )
            .prop_map(|(fill_light, highlights, shadows, temperature, neutral)| {
                EditOp::Tuning {
                    fill_light,
                    highlights,
                    shadows,
                    temperature,
                    neutral,
                }
            }),
    ];
    let effects = prop_oneof![
        (0.0_f32..=2.0).prop_map(|amount| EditOp::Sharpen { amount }),
        Just(EditOp::Sepia),
        Just(EditOp::BlackAndWhite),
        Just(EditOp::Warmify),
        unit().prop_map(|amount| EditOp::FilmGrain { amount }),
        (color(), unit()).prop_map(|(color, preserve)| EditOp::Tint { color, preserve }),
        (-1.0_f32..=1.0).prop_map(|amount| EditOp::Saturation { amount }),
        (unit(), unit(), 0.0_f32..=1.0, unit()).prop_map(|(cx, cy, radius, amount)| {
            EditOp::SoftFocus {
                cx,
                cy,
                radius,
                amount,
            }
        }),
        (unit(), 0.0_f32..=5.0).prop_map(|(intensity, radius)| EditOp::Glow { intensity, radius }),
        (unit(), unit(), 0.0_f32..=1.0, unit()).prop_map(|(cx, cy, radius, sharpness)| {
            EditOp::FocalBlackAndWhite {
                cx,
                cy,
                radius,
                sharpness,
            }
        }),
        (color(), unit(), unit(), unit()).prop_map(|(color, y, feather, amount)| {
            EditOp::GraduatedTint {
                color,
                y,
                feather,
                amount,
            }
        }),
        (unit(), unit()).prop_map(|(amount, radius)| EditOp::Vignette { amount, radius }),
    ];
    prop_oneof![geometry, auto, effects]
}

/// A stack of up to `max_len` valid operations.
pub fn edit_stack(max_len: usize) -> impl Strategy<Value = EditStack> {
    (0_u32..1000, prop::collection::vec(edit_op(), 0..=max_len))
        .prop_map(|(version, ops)| EditStack { version, ops })
}
