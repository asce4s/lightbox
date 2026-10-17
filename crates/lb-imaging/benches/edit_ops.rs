//! Edit operation cost on a 1024 px proxy (SPEC §11).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use criterion::{Criterion, criterion_group, criterion_main};
use image::DynamicImage;
use lb_core::{EditOp, EditStack};
use lb_imaging::{RenderCtx, RenderTarget, render};
use std::hint::black_box;

fn bench(c: &mut Criterion) {
    let src = DynamicImage::ImageRgb8(image::RgbImage::from_fn(1024, 768, |x, y| {
        image::Rgb([(x / 4) as u8, (y / 3) as u8, ((x + y) / 5) as u8])
    }));
    let ctx = RenderCtx::new("bench", RenderTarget::Proxy { max_edge: 1024 });
    let ops = [
        (
            "tuning",
            EditOp::Tuning {
                fill_light: 0.3,
                highlights: 0.2,
                shadows: 0.2,
                temperature: 0.1,
                neutral: None,
            },
        ),
        ("sharpen", EditOp::Sharpen { amount: 0.8 }),
        ("sepia", EditOp::Sepia),
        ("black_and_white", EditOp::BlackAndWhite),
        ("rotate", EditOp::RotateQuarter { turns: 1 }),
    ];
    for (name, op) in ops {
        let stack = EditStack {
            version: 1,
            ops: vec![op],
        };
        c.bench_function(&format!("render/{name}"), |b| {
            b.iter(|| render(black_box(&src), &stack, &ctx));
        });
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
