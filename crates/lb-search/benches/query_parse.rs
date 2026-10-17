//! Query parser throughput (SPEC §11).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use criterion::{Criterion, criterion_group, criterion_main};
use std::hint::black_box;

fn bench(c: &mut Criterion) {
    let queries = [
        "beach",
        "beach rating:>=3 year:2021",
        "\"golden gate\" -tag:work camera:canon iso:>=400 is:starred",
    ];
    for q in queries {
        c.bench_function(&format!("parse/{q}"), |b| {
            b.iter(|| lb_search::parse(black_box(q)));
        });
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
