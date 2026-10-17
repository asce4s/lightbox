//! Shared helpers for service-layer integration tests.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use lb_app::Lightbox;
use lb_core::{Photo, PhotoId, RootMode};
use lb_jobs::JobQueue;
use std::path::{Path, PathBuf};

/// An open library over a private directory of generated JPEGs.
pub struct TestLib {
    pub home: tempfile::TempDir,
    pub photos: tempfile::TempDir,
    pub lb: Lightbox,
    pub ids: Vec<PhotoId>,
}

pub fn write_jpeg(path: &Path, seed: u8) {
    let img = image::RgbImage::from_fn(64, 48, |x, y| {
        image::Rgb([
            (x * 4) as u8,
            (y * 5) as u8,
            seed.wrapping_mul(37).wrapping_add((x + y) as u8),
        ])
    });
    img.save(path).unwrap();
}

/// `n` JPEGs `p00.jpg`, `p01.jpg`, ... in one folder, scanned in name order.
pub fn library(n: usize) -> TestLib {
    let home = tempfile::tempdir().unwrap();
    let photos = tempfile::tempdir().unwrap();
    for i in 0..n {
        write_jpeg(&photos.path().join(format!("p{i:02}.jpg")), i as u8 + 1);
    }
    let lb = Lightbox::open(home.path(), JobQueue::inline()).unwrap();
    lb.library()
        .add_root(photos.path(), RootMode::Watch)
        .unwrap();
    let mut rows: Vec<(PathBuf, PhotoId)> = all_photos(&lb)
        .into_iter()
        .map(|p| (p.path, p.id))
        .collect();
    rows.sort();
    TestLib {
        home,
        photos,
        lb,
        ids: rows.into_iter().map(|(_, id)| id).collect(),
    }
}

/// Every photo in the library, via the public API.
pub fn all_photos(lb: &Lightbox) -> Vec<Photo> {
    lb.photos().all().unwrap()
}
