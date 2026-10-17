//! Thumbnail acceptance tests (SPEC §6.4) over the generated fixtures.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use image::{DynamicImage, GenericImageView};
use lb_media::{Decoder, DefaultDecoder, MediaError, decode, quick_key, resize_exact};
use lb_testkit::fixtures;
use lb_thumbs::{ThumbCache, ThumbRequest, ThumbSize};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};

fn fixture(rel: &str) -> PathBuf {
    let dir = fixtures::default_generated_dir();
    assert!(
        dir.join(fixtures::MARKER).is_file(),
        "run `cargo xtask fixtures` first"
    );
    dir.join(rel)
}

struct CountingDecoder(Arc<AtomicUsize>);

impl Decoder for CountingDecoder {
    fn decode_raw(
        &self,
        path: &Path,
        format: lb_core::Format,
        max: Option<u32>,
    ) -> Result<DynamicImage, MediaError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        DefaultDecoder.decode_raw(path, format, max)
    }
}

fn counting_cache(dir: &Path) -> (ThumbCache, Arc<AtomicUsize>) {
    let counter = Arc::new(AtomicUsize::new(0));
    let cache =
        ThumbCache::with_decoder(dir, Arc::new(CountingDecoder(Arc::clone(&counter)))).unwrap();
    (cache, counter)
}

fn dims(path: &Path) -> (u32, u32) {
    image::open(path).unwrap().dimensions()
}

#[test]
fn thumbnail_respects_orientation() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = ThumbCache::open(tmp.path()).unwrap();
    for (rel, expect) in [
        ("basic/2022/city/city_03_o6.jpg", (120, 160)),
        ("basic/2022/city/city_04_o8.jpg", (120, 160)),
        ("basic/2022/city/city_02_o3.jpg", (160, 120)),
    ] {
        let path = fixture(rel);
        let qk = quick_key(&path).unwrap();
        let thumb = cache
            .get_or_create(&ThumbRequest::new(&path, &qk, ThumbSize::S))
            .unwrap();
        assert_eq!(dims(&thumb.path), expect, "{rel}");
    }

    let path = fixture("basic/2022/city/city_03_o6.jpg");
    let qk = quick_key(&path).unwrap();
    let thumb = cache
        .get_or_create(&ThumbRequest::new(&path, &qk, ThumbSize::M))
        .unwrap();
    let got = image::open(&thumb.path).unwrap().to_rgb8();
    let upright = decode(&path, None).unwrap();
    let want = resize_exact(&upright, got.width(), got.height()).to_rgb8();
    let mean_abs: f64 = got
        .as_raw()
        .iter()
        .zip(want.as_raw())
        .map(|(a, b)| f64::from(a.abs_diff(*b)))
        .sum::<f64>()
        / got.as_raw().len() as f64;
    assert!(
        mean_abs < 8.0,
        "thumbnail content does not match the upright image (mean abs diff {mean_abs:.2})"
    );
}

#[test]
fn thumbnail_long_edge_exact() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = ThumbCache::open(tmp.path()).unwrap();
    let path = fixture("basic/2021/beach/beach_01.jpg");
    let qk = quick_key(&path).unwrap();
    let source = decode(&path, None).unwrap();
    assert!(source.width().max(source.height()) > 320);
    for size in ThumbSize::ALL {
        let thumb = cache
            .get_or_create(&ThumbRequest::new(&path, &qk, size))
            .unwrap();
        let (w, h) = dims(&thumb.path);
        let expected_long = size.px().min(source.width().max(source.height()));
        assert_eq!(w.max(h), expected_long, "{size:?}");
        let ratio = f64::from(w) / f64::from(h);
        let src_ratio = f64::from(source.width()) / f64::from(source.height());
        assert!(
            (ratio - src_ratio).abs() < 0.03,
            "aspect ratio kept at {size:?}"
        );
    }
}

#[test]
fn thumbnail_reflects_edits() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = ThumbCache::open(tmp.path()).unwrap();
    let path = fixture("basic/2021/beach/beach_01.jpg");
    let qk = quick_key(&path).unwrap();
    let plain = cache
        .get_or_create(&ThumbRequest::new(&path, &qk, ThumbSize::S))
        .unwrap();
    let invert = |img: DynamicImage| {
        let mut img = img;
        img.invert();
        img
    };
    let edited = cache
        .get_or_create(
            &ThumbRequest::new(&path, &qk, ThumbSize::S).with_edits("invert-v1", &invert),
        )
        .unwrap();
    assert_ne!(plain.key, edited.key, "cache key changes after an edit");
    assert_ne!(plain.path, edited.path);
    assert_ne!(
        image::open(&plain.path).unwrap().to_rgb8().into_raw(),
        image::open(&edited.path).unwrap().to_rgb8().into_raw(),
        "image differs"
    );
    assert!(!edited.cache_hit);
    let again = cache
        .get_or_create(
            &ThumbRequest::new(&path, &qk, ThumbSize::S).with_edits("invert-v1", &invert),
        )
        .unwrap();
    assert!(again.cache_hit);
}

#[test]
fn thumbnail_cache_hit_does_not_decode() {
    let tmp = tempfile::tempdir().unwrap();
    let (cache, decodes) = counting_cache(tmp.path());
    let path = fixture("basic/2021/beach/beach_02.jpg");
    let qk = quick_key(&path).unwrap();
    let req = ThumbRequest::new(&path, &qk, ThumbSize::M);

    let first = cache.get_or_create(&req).unwrap();
    assert!(!first.cache_hit);
    assert_eq!(decodes.load(Ordering::SeqCst), 1);

    let second = cache.get_or_create(&req).unwrap();
    assert!(second.cache_hit);
    assert_eq!(second.path, first.path);
    assert_eq!(decodes.load(Ordering::SeqCst), 1, "a hit must not decode");

    drop(cache);
    let (reopened, decodes) = counting_cache(tmp.path());
    assert!(reopened.get_or_create(&req).unwrap().cache_hit);
    assert_eq!(decodes.load(Ordering::SeqCst), 0, "hits survive a restart");
}

#[test]
fn thumbnail_cache_gc_evicts_lru() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = ThumbCache::open(tmp.path()).unwrap();
    let names = [
        "basic/2021/beach/beach_01.jpg",
        "basic/2021/beach/beach_02.jpg",
        "basic/2021/beach/beach_03.jpg",
        "basic/2021/beach/beach_04.jpg",
    ];
    let paths: Vec<_> = names.iter().map(|n| fixture(n)).collect();
    let keys: Vec<_> = paths.iter().map(|p| quick_key(p).unwrap()).collect();
    let mut thumbs = Vec::new();
    for (p, k) in paths.iter().zip(&keys) {
        thumbs.push(
            cache
                .get_or_create(&ThumbRequest::new(p, k, ThumbSize::S))
                .unwrap(),
        );
    }
    // Touch the oldest entry so entry 1 becomes the least recently used.
    assert!(
        cache
            .lookup(&ThumbRequest::new(&paths[0], &keys[0], ThumbSize::S))
            .is_some()
    );
    cache.flush().unwrap();

    let sizes: Vec<u64> = thumbs
        .iter()
        .map(|t| fs::metadata(&t.path).unwrap().len())
        .collect();
    let total: u64 = sizes.iter().sum();
    assert_eq!(cache.total_bytes(), total);

    // Reopen so the persisted access index is what drives eviction.
    drop(cache);
    let cache = ThumbCache::open(tmp.path()).unwrap();
    let limit = total - sizes[1] - sizes[2] + 1;
    let report = cache.gc(limit).unwrap();
    assert_eq!(report.evicted, 2);
    assert_eq!(report.freed_bytes, sizes[1] + sizes[2]);
    assert_eq!(report.remaining_bytes, sizes[0] + sizes[3]);
    assert!(thumbs[0].path.exists(), "recently used entry survives");
    assert!(
        !thumbs[1].path.exists(),
        "least recently used entry is evicted"
    );
    assert!(!thumbs[2].path.exists());
    assert!(thumbs[3].path.exists());
    assert!(cache.total_bytes() <= limit);

    let noop = cache.gc(u64::MAX).unwrap();
    assert_eq!(noop.evicted, 0);
}

#[test]
fn thumbnail_write_is_atomic() {
    let tmp = tempfile::tempdir().unwrap();
    let (cache, decodes) = counting_cache(tmp.path());
    let path = fixture("basic/2021/beach/beach_03.jpg");
    let qk = quick_key(&path).unwrap();
    let req = ThumbRequest::new(&path, &qk, ThumbSize::S);
    let dest = cache.path_for(&req.key(), ThumbSize::S);

    // A crash mid-write leaves only a temp file next to the destination.
    fs::create_dir_all(dest.parent().unwrap()).unwrap();
    let partial = dest.parent().unwrap().join(".tmp-99999-0.part");
    fs::write(&partial, b"\xff\xd8 truncated").unwrap();
    assert!(
        cache.lookup(&req).is_none(),
        "partial temp files are ignored"
    );

    let thumb = cache.get_or_create(&req).unwrap();
    assert!(!thumb.cache_hit);
    assert_eq!(decodes.load(Ordering::SeqCst), 1);
    assert!(
        image::open(&thumb.path).is_ok(),
        "final file is a complete JPEG"
    );

    let leftovers = |dir: &Path| {
        fs::read_dir(dir)
            .unwrap()
            .flatten()
            .filter(|e| e.file_name().to_string_lossy().ends_with(".part"))
            .count()
    };
    assert_eq!(
        leftovers(dest.parent().unwrap()),
        1,
        "only the foreign partial remains"
    );
    let report = cache.gc(u64::MAX).unwrap();
    assert_eq!(report.stale_temp_files, 1);
    assert_eq!(leftovers(dest.parent().unwrap()), 0);
    assert!(thumb.path.exists());
}

#[test]
fn concurrent_requests_for_one_key_leave_a_valid_file() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = Arc::new(ThumbCache::open(tmp.path()).unwrap());
    let path = fixture("basic/2021/beach/beach_04.jpg");
    let qk = quick_key(&path).unwrap();
    std::thread::scope(|s| {
        for _ in 0..6 {
            s.spawn(|| {
                let t = cache
                    .get_or_create(&ThumbRequest::new(&path, &qk, ThumbSize::S))
                    .unwrap();
                assert!(image::open(&t.path).is_ok());
            });
        }
    });
}

#[test]
fn unreadable_source_errors_and_leaves_no_cache_entry() {
    let tmp = tempfile::tempdir().unwrap();
    let cache = ThumbCache::open(tmp.path()).unwrap();
    let bad = tmp.path().join("broken.jpg");
    fs::write(&bad, b"definitely not a jpeg").unwrap();
    let req = ThumbRequest::new(&bad, "qk-broken", ThumbSize::S);
    assert!(cache.get_or_create(&req).is_err());
    assert!(cache.lookup(&req).is_none());
    assert_eq!(cache.total_bytes(), 0);

    let missing = tmp.path().join("gone.jpg");
    assert!(
        cache
            .get_or_create(&ThumbRequest::new(&missing, "qk-gone", ThumbSize::S))
            .is_err()
    );
}
