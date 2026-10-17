//! M11.1: thumbnail budget of SPEC §11 (S thumbnail from a 12 MP JPEG, DCT-scaled decode).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use lb_media::quick_key;
use lb_testkit::perf::{check, median_of, scale};
use lb_thumbs::{ThumbCache, ThumbRequest, ThumbSize};
use std::time::Duration;

#[test]
fn thumbnail_s_from_12mp_under_budget() {
    let (w, h) = (
        u32::try_from(scale(4000, 800)).unwrap(),
        u32::try_from(scale(3000, 600)).unwrap(),
    );
    let tmp = tempfile::tempdir().unwrap();
    let src = tmp.path().join("big.jpg");
    image::RgbImage::from_fn(w, h, |x, y| {
        image::Rgb([(x / 16) as u8, (y / 12) as u8, ((x ^ y) / 8) as u8])
    })
    .save(&src)
    .unwrap();
    let qk = quick_key(&src).unwrap();

    let runs = 5;
    let mut n = 0;
    let median = median_of(runs, || {
        let cache = ThumbCache::open(&tmp.path().join(format!("cache{n}"))).unwrap();
        n += 1;
        let thumb = cache
            .get_or_create(&ThumbRequest::new(&src, &qk, ThumbSize::S))
            .unwrap();
        assert!(thumb.path.is_file());
    });
    check("thumbnail_s_12mp", median, Duration::from_millis(60));
}
