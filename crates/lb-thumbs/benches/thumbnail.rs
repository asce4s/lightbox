//! Thumbnail generation from a JPEG (SPEC §11).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use criterion::{Criterion, criterion_group, criterion_main};
use lb_media::quick_key;
use lb_thumbs::{ThumbCache, ThumbRequest, ThumbSize};

fn bench(c: &mut Criterion) {
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("photo.jpg");
    image::RgbImage::from_fn(2000, 1500, |x, y| {
        image::Rgb([(x / 8) as u8, (y / 6) as u8, ((x ^ y) / 4) as u8])
    })
    .save(&src)
    .unwrap();
    let qk = quick_key(&src).unwrap();
    for size in [ThumbSize::S, ThumbSize::M] {
        let mut n = 0u32;
        c.bench_function(&format!("thumbnail/{size:?}"), |b| {
            b.iter(|| {
                n += 1;
                let cache = ThumbCache::open(&tmp.path().join(format!("c{n}"))).unwrap();
                cache
                    .get_or_create(&ThumbRequest::new(&src, &qk, size))
                    .unwrap()
            });
        });
    }
}

criterion_group!(benches, bench);
criterion_main!(benches);
