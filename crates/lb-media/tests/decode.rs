//! Decoding acceptance tests (SPEC §6.2.3, §6.2.4) over the generated fixture set.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use image::{DynamicImage, GenericImageView};
use lb_core::Format;
use lb_media::{Decoder, MediaError, decode, decode_with, probe};
use lb_testkit::fixtures::{self, manifest::Status};
use std::path::{Path, PathBuf};

fn fixtures_dir() -> PathBuf {
    let dir = fixtures::default_generated_dir();
    assert!(
        dir.join(fixtures::MARKER).is_file(),
        "run `cargo xtask fixtures` first"
    );
    dir
}

fn spec_of(path: &str) -> fixtures::manifest::FileSpec {
    fixtures::manifest::Manifest::load_default()
        .unwrap()
        .files
        .into_iter()
        .find(|f| f.path == path)
        .unwrap()
}

#[test]
fn decode_all_basic_formats() {
    let dir = fixtures_dir();
    let manifest = fixtures::manifest::Manifest::load_default().unwrap();
    let mut decoded = 0;
    for spec in manifest
        .files
        .iter()
        .filter(|f| f.path.starts_with("basic/") && f.expect.status == Status::Ok)
    {
        let path = dir.join(&spec.path);
        let img = decode(&path, None).unwrap_or_else(|e| panic!("{}: {e}", spec.path));
        assert_eq!(
            (Some(img.width()), Some(img.height())),
            (spec.expect.width, spec.expect.height),
            "{}",
            spec.path
        );
        let info = probe(&path).unwrap_or_else(|e| panic!("{}: {e}", spec.path));
        assert_eq!(
            (info.width, info.height),
            (img.width(), img.height()),
            "{}",
            spec.path
        );
        decoded += 1;
    }
    assert!(decoded >= 25, "decoded only {decoded} files");
    for (name, format) in [
        ("basic/misc/sample.png", Format::Png),
        ("basic/misc/animated.gif", Format::Gif),
        ("basic/misc/sample.webp", Format::Webp),
        ("basic/misc/sample.bmp", Format::Bmp),
        ("basic/misc/sample.tiff", Format::Tiff),
        ("basic/2021/beach/beach_01.jpg", Format::Jpeg),
    ] {
        assert_eq!(probe(&dir.join(name)).unwrap().format, format, "{name}");
    }
}

#[test]
fn exif_orientation_6_swaps_dimensions() {
    let dir = fixtures_dir();
    let spec = spec_of("basic/2022/city/city_03_o6.jpg");
    let path = dir.join(&spec.path);
    let (stored_w, stored_h) = (spec.width.unwrap(), spec.height.unwrap());
    assert!(stored_w > stored_h, "fixture is stored landscape");
    let info = probe(&path).unwrap();
    assert_eq!(info.orientation, 6);
    assert_eq!((info.width, info.height), (stored_h, stored_w));
    let img = decode(&path, None).unwrap();
    assert_eq!((img.width(), img.height()), (stored_h, stored_w));
    for name in ["city_02_o3", "city_04_o8", "city_01_o1"] {
        let spec = spec_of(&format!("basic/2022/city/{name}.jpg"));
        let img = decode(&dir.join(&spec.path), None).unwrap();
        assert_eq!(
            (Some(img.width()), Some(img.height())),
            (spec.expect.width, spec.expect.height),
            "{name}"
        );
    }
}

#[test]
fn orientation_pixels_are_rotated_upright() {
    let dir = fixtures_dir();
    let upright = decode(&dir.join("basic/2022/city/city_01_o1.jpg"), None).unwrap();
    let rotated = decode(&dir.join("basic/2022/city/city_03_o6.jpg"), None).unwrap();
    assert_eq!(upright.dimensions(), (rotated.height(), rotated.width()));
}

#[test]
fn dct_scaled_jpeg_is_smaller_and_fits() {
    let dir = fixtures_dir();
    let path = dir.join("basic/misc/large_6000x4000.jpg");
    let img = decode(&path, Some(160)).unwrap();
    assert_eq!(img.width().max(img.height()), 160);
    assert_eq!((img.width(), img.height()), (160, 107));
    let small = decode(&dir.join("basic/misc/tiny_1x1.png"), Some(160)).unwrap();
    assert_eq!((small.width(), small.height()), (1, 1), "never upscales");
}

struct PanickingDecoder;

impl Decoder for PanickingDecoder {
    fn decode_raw(&self, _: &Path, _: Format, _: Option<u32>) -> lb_media::Result<DynamicImage> {
        panic!("boom from decoder");
    }
}

#[test]
fn decoder_panic_is_caught() {
    let dir = fixtures_dir();
    let err = decode_with(
        &PanickingDecoder,
        &dir.join("basic/2021/beach/beach_01.jpg"),
        None,
    )
    .unwrap_err();
    assert!(
        matches!(&err, MediaError::Corrupt(m) if m.contains("boom from decoder")),
        "{err:?}"
    );
}

#[test]
fn corrupt_files_marked_unreadable_without_panic() {
    let dir = fixtures_dir();
    let manifest = fixtures::manifest::Manifest::load_default().unwrap();
    let bad: Vec<_> = manifest
        .files
        .iter()
        .filter(|f| f.expect.status == Status::Unreadable)
        .collect();
    assert_eq!(bad.len(), 3, "zero-byte, truncated, text-as-jpg");
    for spec in bad {
        let path = dir.join(&spec.path);
        assert!(
            matches!(probe(&path), Err(MediaError::Corrupt(_))),
            "probe {}",
            spec.path
        );
        assert!(
            matches!(decode(&path, None), Err(MediaError::Corrupt(_))),
            "decode {}",
            spec.path
        );
        assert!(
            matches!(decode(&path, Some(160)), Err(MediaError::Corrupt(_))),
            "scaled decode {}",
            spec.path
        );
    }
}

#[test]
fn wrong_extension_is_identified_by_content() {
    let dir = fixtures_dir();
    let tmp = tempfile::tempdir().unwrap();
    let renamed = tmp.path().join("really_a_png.jpg");
    std::fs::copy(dir.join("basic/misc/sample.png"), &renamed).unwrap();
    assert_eq!(probe(&renamed).unwrap().format, Format::Png);
    assert!(decode(&renamed, None).is_ok());
    assert!(matches!(
        decode(&tmp.path().join("missing.jpg"), None),
        Err(MediaError::Io(_))
    ));
}
