use super::*;
use proptest::prelude::*;
use proptest::strategy::ValueTree;

fn sample_stack() -> EditStack {
    EditStack {
        version: 3,
        ops: vec![
            EditOp::RotateQuarter { turns: 1 },
            EditOp::Crop {
                x: 0.1,
                y: 0.2,
                w: 0.5,
                h: 0.25,
            },
            EditOp::Tuning {
                fill_light: 0.3,
                highlights: 0.0,
                shadows: 0.5,
                temperature: -0.25,
                neutral: Some([0.5, 0.6, 0.7]),
            },
            EditOp::AutoContrast,
        ],
    }
}

proptest! {
    #[test]
    fn edit_stack_serde_roundtrip(stack in strategies::edit_stack(8)) {
        let json = serde_json::to_string(&stack).unwrap();
        let back: EditStack = serde_json::from_str(&json).unwrap();
        prop_assert_eq!(&back, &stack);
        prop_assert_eq!(back.hash(), stack.hash());
    }

    #[test]
    fn valid_strategy_ops_validate_to_themselves(op in strategies::edit_op()) {
        let checked = op.validate().unwrap();
        prop_assert_eq!(checked.hash_form(), op.hash_form());
    }
}

impl EditOp {
    fn hash_form(&self) -> String {
        crate::edit::hash::canonical_ops_json(std::slice::from_ref(self))
    }
}

#[test]
fn edit_op_json_shape_is_tagged_snake_case() {
    let json = serde_json::to_value(EditOp::BlackAndWhite).unwrap();
    assert_eq!(json, serde_json::json!({"op": "black_and_white"}));
    let json = serde_json::to_value(EditOp::RotateQuarter { turns: -1 }).unwrap();
    assert_eq!(
        json,
        serde_json::json!({"op": "rotate_quarter", "turns": -1})
    );
    let op: EditOp = serde_json::from_str(
        r#"{"op":"tuning","fill_light":0.1,"highlights":0.2,"shadows":0.3,"temperature":0.4}"#,
    )
    .unwrap();
    assert!(matches!(op, EditOp::Tuning { neutral: None, .. }));
    assert!(serde_json::from_str::<EditOp>(r#"{"op":"nope"}"#).is_err());
}

#[test]
fn stack_hash_stable_across_runs() {
    let stack = sample_stack();
    let json = hash::canonical_ops_json(&stack.ops);
    assert_eq!(
        json,
        concat!(
            r#"[{"op":"rotate_quarter","turns":1},"#,
            r#"{"h":0.250000,"op":"crop","w":0.500000,"x":0.100000,"y":0.200000},"#,
            r#"{"fill_light":0.300000,"highlights":0.000000,"neutral":[0.500000,0.600000,0.700000],"op":"tuning","shadows":0.500000,"temperature":-0.250000},"#,
            r#"{"op":"auto_contrast"}]"#
        )
    );
    assert_eq!(
        stack.hash(),
        blake3::hash(json.as_bytes()).to_hex().to_string()
    );
    assert_eq!(
        stack.hash(),
        "2d543ea7b4135a0dcddc96bd400d02a14db3088a44f77f0d95e810bbf3934c25",
        "pinned: the hash format is part of the thumbnail cache key"
    );

    let mut bumped = stack.clone();
    bumped.version += 10;
    assert_eq!(bumped.hash(), stack.hash(), "version is not hashed");

    let mut jittered = stack.clone();
    if let EditOp::Crop { x, .. } = &mut jittered.ops[1] {
        *x += 1e-8;
    }
    assert_eq!(jittered.hash(), stack.hash(), "6-decimal canonical floats");

    let mut changed = stack.clone();
    if let EditOp::Crop { x, .. } = &mut changed.ops[1] {
        *x += 0.001;
    }
    assert_ne!(changed.hash(), stack.hash());

    let mut reordered = stack.clone();
    reordered.ops.swap(0, 1);
    assert_ne!(reordered.hash(), stack.hash(), "order matters");

    assert_eq!(EditStack::new().cache_hash(), "");
    assert_eq!(stack.cache_hash(), stack.hash());
}

#[test]
fn negative_zero_hashes_like_zero() {
    let a = [EditOp::Straighten { degrees: 0.0 }];
    let b = [EditOp::Straighten { degrees: -0.0 }];
    assert_eq!(hash::canonical_ops_json(&a), hash::canonical_ops_json(&b));
}

#[test]
fn edit_op_validation_table() {
    use EditOp::*;
    let nan = f32::NAN;
    let ok = |op: EditOp| op.validate().unwrap();
    let err = |op: EditOp| op.validate().unwrap_err();

    // Clamping.
    assert_eq!(
        ok(Straighten { degrees: 90.0 }),
        Straighten { degrees: 45.0 }
    );
    assert_eq!(
        ok(Straighten { degrees: -90.0 }),
        Straighten { degrees: -45.0 }
    );
    assert_eq!(ok(FillLight { amount: 2.0 }), FillLight { amount: 1.0 });
    assert_eq!(ok(FillLight { amount: -1.0 }), FillLight { amount: 0.0 });
    assert_eq!(ok(Sharpen { amount: 9.0 }), Sharpen { amount: 2.0 });
    assert_eq!(ok(Saturation { amount: -3.0 }), Saturation { amount: -1.0 });
    assert_eq!(ok(FilmGrain { amount: 7.0 }), FilmGrain { amount: 1.0 });
    assert_eq!(
        ok(Tuning {
            fill_light: 5.0,
            highlights: -1.0,
            shadows: 0.5,
            temperature: 3.0,
            neutral: Some([2.0, -1.0, 0.5]),
        }),
        Tuning {
            fill_light: 1.0,
            highlights: 0.0,
            shadows: 0.5,
            temperature: 1.0,
            neutral: Some([1.0, 0.0, 0.5]),
        }
    );
    assert_eq!(
        ok(Tint {
            color: [1.5, 0.5, 0.5],
            preserve: -1.0
        }),
        Tint {
            color: [1.0, 0.5, 0.5],
            preserve: 0.0
        }
    );
    assert_eq!(
        ok(Vignette {
            amount: 2.0,
            radius: 2.0
        }),
        Vignette {
            amount: 1.0,
            radius: 1.0
        }
    );
    assert_eq!(
        ok(Glow {
            intensity: 2.0,
            radius: 50.0
        }),
        Glow {
            intensity: 1.0,
            radius: 5.0
        }
    );

    // Valid values are untouched.
    assert_eq!(ok(AutoColor), AutoColor);
    assert_eq!(ok(RotateQuarter { turns: -2 }), RotateQuarter { turns: -2 });
    let crop = Crop {
        x: 0.25,
        y: 0.25,
        w: 0.5,
        h: 0.5,
    };
    assert_eq!(ok(crop.clone()), crop);
    assert_eq!(
        ok(Crop {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0
        }),
        Crop {
            x: 0.0,
            y: 0.0,
            w: 1.0,
            h: 1.0
        }
    );

    // Rejections.
    for turns in [0, 3, -3, 4, i8::MAX] {
        assert_eq!(err(RotateQuarter { turns }), EditError::BadTurns(turns));
    }
    for (x, y, w, h) in [
        (0.0, 0.0, 0.0, 0.5),
        (0.0, 0.0, 0.5, 0.0),
        (0.0, 0.0, -0.5, 0.5),
        (-0.1, 0.0, 0.5, 0.5),
        (0.0, -0.1, 0.5, 0.5),
        (0.6, 0.0, 0.5, 0.5),
        (0.0, 0.6, 0.5, 0.5),
        (0.0, 0.0, 1.5, 1.5),
    ] {
        assert!(
            matches!(err(Crop { x, y, w, h }), EditError::BadRect { .. }),
            "{x} {y} {w} {h}"
        );
    }
    assert!(matches!(
        err(Crop {
            x: nan,
            y: 0.0,
            w: 0.5,
            h: 0.5
        }),
        EditError::NotFinite {
            op: "crop",
            field: "x"
        }
    ));
    assert!(matches!(
        err(Straighten {
            degrees: f32::INFINITY
        }),
        EditError::NotFinite { .. }
    ));
    assert!(matches!(
        err(Sharpen { amount: nan }),
        EditError::NotFinite { .. }
    ));
    assert!(matches!(
        err(Tint {
            color: [0.0, nan, 0.0],
            preserve: 0.5
        }),
        EditError::NotFinite { .. }
    ));
    assert_eq!(err(RedEye { regions: vec![] }), EditError::Empty("red_eye"));
    assert_eq!(err(Retouch { spots: vec![] }), EditError::Empty("retouch"));
    let region = NormRect {
        x: 0.1,
        y: 0.1,
        w: 0.1,
        h: 0.1,
    };
    assert!(matches!(
        err(RedEye {
            regions: vec![region; 65]
        }),
        EditError::TooMany {
            max: 64,
            got: 65,
            ..
        }
    ));
    assert!(matches!(
        err(RedEye {
            regions: vec![NormRect { w: 0.0, ..region }]
        }),
        EditError::BadRect { .. }
    ));
    assert_eq!(
        ok(Retouch {
            spots: vec![RetouchSpot {
                src_x: 2.0,
                src_y: 0.5,
                dst_x: 0.5,
                dst_y: 0.5,
                radius: 9.0
            }]
        }),
        Retouch {
            spots: vec![RetouchSpot {
                src_x: 1.0,
                src_y: 0.5,
                dst_x: 0.5,
                dst_y: 0.5,
                radius: 0.5
            }]
        }
    );
}

#[test]
fn every_op_has_a_distinct_name_matching_its_tag() {
    use std::collections::HashSet;
    let mut runner = proptest::test_runner::TestRunner::deterministic();
    let mut names = HashSet::new();
    for _ in 0..400 {
        let op = strategies::edit_op()
            .new_tree(&mut runner)
            .unwrap()
            .current();
        let tag = serde_json::to_value(&op).unwrap()["op"]
            .as_str()
            .unwrap()
            .to_owned();
        assert_eq!(tag, op.name());
        names.insert(tag);
    }
    assert_eq!(names.len(), 22, "strategy covers every op: {names:?}");
}
