//! M11.1: editor proxy render budget of SPEC §11 (2048 px proxy, Tuning + Sharpen).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use image::DynamicImage;
use lb_core::{EditOp, EditStack};
use lb_imaging::{RenderCtx, RenderTarget, render};
use lb_testkit::perf::{check, median_of, scale};
use std::time::Duration;

#[test]
fn editor_proxy_render_under_budget() {
    let edge = u32::try_from(scale(2048, 512)).unwrap();
    let src = DynamicImage::ImageRgb8(image::RgbImage::from_fn(edge, edge * 3 / 4, |x, y| {
        image::Rgb([(x / 8) as u8, (y / 6) as u8, ((x + y) / 10) as u8])
    }));
    let stack = EditStack {
        version: 1,
        ops: vec![
            EditOp::Tuning {
                fill_light: 0.3,
                highlights: 0.2,
                shadows: 0.2,
                temperature: 0.1,
                neutral: None,
            },
            EditOp::Sharpen { amount: 0.8 },
        ],
    };
    let ctx = RenderCtx::new("perf", RenderTarget::Proxy { max_edge: edge });
    let median = median_of(5, || {
        let out = render(&src, &stack, &ctx);
        assert_eq!(out.width(), edge);
    });
    check(
        "editor_proxy_render_2048",
        median,
        Duration::from_millis(120),
    );
}
