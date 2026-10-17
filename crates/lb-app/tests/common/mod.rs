//! Shared helpers for service-layer integration tests.
#![allow(dead_code, clippy::unwrap_used, clippy::expect_used)]

use lb_app::{Lightbox, LightboxOptions};
use lb_core::{Photo, PhotoId, RootMode};
use lb_jobs::JobQueue;
use lb_testkit::{FakeOpener, FakeTrash};
use std::path::{Path, PathBuf};
use std::sync::Arc;

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
    build(&[("", n)], LightboxOptions::default())
}

/// Fake trash and opener plugged into library options.
pub struct Fakes {
    pub trash: Arc<FakeTrash>,
    pub opener: Arc<FakeOpener>,
    pub options: LightboxOptions,
    pub trash_dir: tempfile::TempDir,
}

pub fn fakes() -> Fakes {
    let trash_dir = tempfile::tempdir().unwrap();
    let trash = Arc::new(FakeTrash::new(trash_dir.path().join("trash")));
    let opener = Arc::new(FakeOpener::new());
    let options = LightboxOptions {
        trash: trash.clone(),
        opener: opener.clone(),
        ..LightboxOptions::default()
    };
    Fakes {
        trash,
        opener,
        options,
        trash_dir,
    }
}

/// Folders named by `spec` (`""` is the root itself) holding the given number of JPEGs named
/// `p00.jpg`, ...; ids are ordered by path.
pub fn build(spec: &[(&str, usize)], options: LightboxOptions) -> TestLib {
    let home = tempfile::tempdir().unwrap();
    let photos = tempfile::tempdir().unwrap();
    let mut seed = 0u8;
    for (dir, n) in spec {
        let dir = photos.path().join(dir);
        std::fs::create_dir_all(&dir).unwrap();
        for i in 0..*n {
            seed += 1;
            write_jpeg(&dir.join(format!("p{i:02}.jpg")), seed);
        }
    }
    let lb = Lightbox::open_with(home.path(), JobQueue::inline(), options).unwrap();
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

/// One folder of JPEGs with the given names; names like `IMG_20210615_120000.jpg` fix the capture
/// date. Ids follow the order of `names`.
pub fn named(names: &[&str]) -> TestLib {
    let home = tempfile::tempdir().unwrap();
    let photos = tempfile::tempdir().unwrap();
    for (i, name) in names.iter().enumerate() {
        write_jpeg(&photos.path().join(name), i as u8 + 1);
    }
    let lb =
        Lightbox::open_with(home.path(), JobQueue::inline(), LightboxOptions::default()).unwrap();
    lb.library()
        .add_root(photos.path(), RootMode::Watch)
        .unwrap();
    let all = all_photos(&lb);
    let ids = names
        .iter()
        .map(|n| all.iter().find(|p| p.path.ends_with(n)).unwrap().id)
        .collect();
    TestLib {
        home,
        photos,
        lb,
        ids,
    }
}
