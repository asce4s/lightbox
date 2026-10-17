//! Render API tests (SPEC §6.6.4).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use image::DynamicImage;
use lb_core::{EditOp, EditStack};
use lb_imaging::{RenderCtx, RenderTarget, convert, render, render_cancellable};
use lb_media::resize_exact;
use lb_testkit::compare_images;
use std::sync::atomic::AtomicBool;

fn big_source() -> DynamicImage {
    DynamicImage::ImageRgba8(lb_testkit::fixtures::pattern::render(
        lb_testkit::fixtures::manifest::Pattern::Scene,
        5,
        1600,
        1200,
    ))
}

fn stack(ops: Vec<EditOp>) -> EditStack {
    EditStack { version: 1, ops }
}

fn rich_stack() -> EditStack {
    stack(vec![
        EditOp::Straighten { degrees: 3.0 },
        EditOp::Crop {
            x: 0.05,
            y: 0.1,
            w: 0.9,
            h: 0.8,
        },
        EditOp::FeelingLucky,
        EditOp::Tuning {
            fill_light: 0.3,
            highlights: 0.2,
            shadows: 0.1,
            temperature: 0.2,
            neutral: None,
        },
        EditOp::Saturation { amount: 0.3 },
        EditOp::SoftFocus {
            cx: 0.5,
            cy: 0.5,
            radius: 0.25,
            amount: 0.8,
        },
        EditOp::Glow {
            intensity: 0.5,
            radius: 1.0,
        },
        EditOp::Tint {
            color: [1.0, 0.8, 0.6],
            preserve: 0.6,
        },
        EditOp::Vignette {
            amount: 0.5,
            radius: 0.4,
        },
    ])
}

#[test]
fn proxy_and_full_render_similar() {
    let src = big_source();
    let stack = rich_stack();
    let full = render(&src, &stack, &RenderCtx::new("qk", RenderTarget::Full));
    let proxy = render(
        &src,
        &stack,
        &RenderCtx::new("qk", RenderTarget::Proxy { max_edge: 800 }),
    );
    assert!(proxy.width().max(proxy.height()) <= 800);
    assert!(full.width() > proxy.width());

    let full8 = DynamicImage::ImageRgba8(convert::to_rgba8(&full));
    let down = resize_exact(&full8, proxy.width(), proxy.height()).to_rgba8();
    let proxy8 = convert::to_rgba8(&proxy);
    let diff = compare_images(&proxy8, &down).expect("same dimensions");
    assert!(
        diff.psnr >= 35.0,
        "proxy differs from the downscaled full render: PSNR {:.2} dB",
        diff.psnr
    );
}

#[test]
fn render_is_bit_identical_across_runs() {
    let src = big_source();
    let mut ops = rich_stack().ops;
    ops.push(EditOp::FilmGrain { amount: 0.6 });
    ops.push(EditOp::Sharpen { amount: 1.0 });
    let stack = stack(ops);
    let ctx = RenderCtx::new("0123456789abcdef", RenderTarget::Proxy { max_edge: 600 });
    let first = render(&src, &stack, &ctx);
    for _ in 0..3 {
        assert!(
            render(&src, &stack, &ctx).as_raw() == first.as_raw(),
            "renders must be bit-identical"
        );
    }
    let other = RenderCtx::new("fedcba9876543210", ctx.target);
    assert!(
        render(&src, &stack, &other).as_raw() != first.as_raw(),
        "grain depends on the quick key"
    );
}

#[test]
fn empty_stack_renders_the_source() {
    let src = DynamicImage::ImageRgba8(lb_testkit::fixtures::pattern::render(
        lb_testkit::fixtures::manifest::Pattern::Patches,
        1,
        64,
        48,
    ));
    let out = render(
        &src,
        &EditStack::new(),
        &RenderCtx::new("k", RenderTarget::Full),
    );
    assert_eq!(convert::to_rgba8(&out), src.to_rgba8());
}

#[test]
fn proxy_never_upscales_and_keeps_aspect() {
    let src = DynamicImage::ImageRgba8(lb_testkit::fixtures::pattern::render(
        lb_testkit::fixtures::manifest::Pattern::Gradient,
        1,
        300,
        200,
    ));
    let ctx = |max_edge| RenderCtx::new("k", RenderTarget::Proxy { max_edge });
    let big = render(&src, &EditStack::new(), &ctx(2048));
    assert_eq!(big.dimensions(), (300, 200));
    let small = render(&src, &EditStack::new(), &ctx(150));
    assert_eq!(small.dimensions(), (150, 100));
}

#[test]
fn invalid_ops_are_skipped_and_valid_ones_still_apply() {
    let src = big_source();
    let good = stack(vec![EditOp::Sepia]);
    let with_bad = stack(vec![
        EditOp::Crop {
            x: 0.0,
            y: 0.0,
            w: 0.0,
            h: 0.5,
        },
        EditOp::Sepia,
        EditOp::Sharpen { amount: f32::NAN },
    ]);
    let ctx = RenderCtx::new("k", RenderTarget::Proxy { max_edge: 200 });
    assert!(render(&src, &good, &ctx).as_raw() == render(&src, &with_bad, &ctx).as_raw());
}

#[test]
fn cancellation_stops_a_render() {
    let src = big_source();
    let cancel = AtomicBool::new(true);
    let result = render_cancellable(
        &src,
        &rich_stack(),
        &RenderCtx::new("k", RenderTarget::Full),
        &cancel,
    );
    assert!(result.is_err());
    let cancel = AtomicBool::new(false);
    assert!(
        render_cancellable(
            &src,
            &stack(vec![EditOp::Sepia]),
            &RenderCtx::new("k", RenderTarget::Proxy { max_edge: 100 }),
            &cancel
        )
        .is_ok()
    );
}
