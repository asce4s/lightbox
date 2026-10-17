//! M9.4: exact and similar duplicates.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use image::{DynamicImage, GrayImage, Luma, RgbImage};
use lb_app::{DEFAULT_SIMILARITY, Lightbox, LightboxOptions};
use lb_core::{PhotoId, RootMode};
use lb_jobs::JobQueue;
use lb_testkit::FakeTrash;
use std::path::Path;
use std::sync::Arc;

/// A picture of 8x6 random grey blocks, so different seeds look different.
fn blocks(seed: u64, w: u32, h: u32) -> DynamicImage {
    let mut state = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) | 1;
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let cells: Vec<u8> = (0..48).map(|_| (next() % 256) as u8).collect();
    let grey = GrayImage::from_fn(w, h, |x, y| {
        Luma([cells[((y * 6 / h) * 8 + x * 8 / w) as usize]])
    });
    DynamicImage::ImageRgb8(RgbImage::from_fn(w, h, |x, y| {
        let v = grey.get_pixel(x, y).0[0];
        image::Rgb([v, v, v])
    }))
}

struct Lib {
    _home: tempfile::TempDir,
    dir: tempfile::TempDir,
    lb: Lightbox,
    trash: Arc<FakeTrash>,
    _trash_dir: tempfile::TempDir,
}

fn lib(files: &[(&str, DynamicImage)], copies: &[(&str, &str)]) -> Lib {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    let trash_dir = tempfile::tempdir().unwrap();
    for (name, img) in files {
        img.save(dir.path().join(name)).unwrap();
    }
    for (from, to) in copies {
        std::fs::copy(dir.path().join(from), dir.path().join(to)).unwrap();
    }
    let trash = Arc::new(FakeTrash::new(trash_dir.path().join("trash")));
    let options = LightboxOptions {
        trash: trash.clone(),
        ..LightboxOptions::default()
    };
    let lb = Lightbox::open_with(home.path(), JobQueue::inline(), options).unwrap();
    lb.library().add_root(dir.path(), RootMode::Watch).unwrap();
    Lib {
        _home: home,
        dir,
        lb,
        trash,
        _trash_dir: trash_dir,
    }
}

fn id(l: &Lib, name: &str) -> PhotoId {
    l.lb.catalog_handle()
        .photo_by_path(&l.dir.path().canonicalize().unwrap().join(name))
        .unwrap()
        .unwrap_or_else(|| panic!("{name} is indexed"))
        .id
}

#[test]
fn exact_duplicates_grouped() {
    let l = lib(
        &[
            ("a.jpg", blocks(1, 320, 240)),
            ("b.jpg", blocks(2, 320, 240)),
        ],
        &[("a.jpg", "a_copy.jpg")],
    );
    let groups = l.lb.duplicates().find(0).unwrap();
    assert_eq!(groups.len(), 1, "{groups:?}");
    assert!(groups[0].exact);
    assert_eq!(groups[0].ids, vec![id(&l, "a.jpg"), id(&l, "a_copy.jpg")]);

    let hashes = l.lb.catalog_handle().photo_hashes().unwrap();
    assert!(hashes.iter().all(|h| h.phash.is_some()), "stored");
    assert!(
        hashes.iter().filter(|h| h.content_hash.is_some()).count() >= 2,
        "exact candidates hashed"
    );
}

#[test]
fn resized_copy_is_similar() {
    let big = blocks(7, 640, 480);
    let small = big.resize_exact(160, 120, image::imageops::FilterType::Triangle);
    let l = lib(
        &[
            ("big.jpg", big),
            ("small.jpg", small),
            ("other.jpg", blocks(8, 640, 480)),
        ],
        &[],
    );
    let groups = l.lb.duplicates().find(DEFAULT_SIMILARITY).unwrap();
    assert_eq!(groups.len(), 1, "{groups:?}");
    assert!(!groups[0].exact);
    assert_eq!(groups[0].ids, vec![id(&l, "big.jpg"), id(&l, "small.jpg")]);
}

#[test]
fn different_images_not_similar() {
    let files: Vec<(String, DynamicImage)> = (1..=12)
        .map(|s| (format!("p{s}.jpg"), blocks(s * 101, 320, 240)))
        .collect();
    let refs: Vec<(&str, DynamicImage)> =
        files.iter().map(|(n, i)| (n.as_str(), i.clone())).collect();
    let l = lib(&refs, &[]);
    assert!(
        l.lb.duplicates()
            .find(DEFAULT_SIMILARITY)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn duplicate_review_actions() {
    let l = lib(
        &[("a.jpg", blocks(1, 320, 240))],
        &[("a.jpg", "a2.jpg"), ("a.jpg", "a3.jpg")],
    );
    let groups = l.lb.duplicates().find(DEFAULT_SIMILARITY).unwrap();
    assert_eq!(groups[0].ids.len(), 3);
    let keep = groups[0].ids[1];
    l.lb.duplicates().star_best(keep).unwrap();
    assert!(l.lb.photos().get(keep).unwrap().unwrap().starred);

    let album = l.lb.albums().create("Dupes", None).unwrap();
    assert_eq!(
        l.lb.duplicates()
            .add_group_to_album(&groups[0], album.id)
            .unwrap(),
        3
    );

    let report = l.lb.duplicates().keep_one(&groups[0], keep).unwrap();
    assert_eq!(report.deleted.len(), 2);
    assert_eq!(l.trash.trashed().len(), 2);
    assert!(
        l.lb.duplicates()
            .find(DEFAULT_SIMILARITY)
            .unwrap()
            .is_empty()
    );
    assert!(Path::new(&l.lb.photos().get(keep).unwrap().unwrap().path).exists());
}
