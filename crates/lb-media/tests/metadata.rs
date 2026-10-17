//! Metadata acceptance tests (SPEC §6.2.2) over the generated fixtures.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use chrono::NaiveDateTime;
use lb_media::{extract_image, mtime_to_naive_local, read_exif};
use lb_testkit::fixtures::{self, manifest::Manifest, manifest::Status};

fn setup() -> (std::path::PathBuf, Manifest) {
    let dir = fixtures::default_generated_dir();
    assert!(
        dir.join(fixtures::MARKER).is_file(),
        "run `cargo xtask fixtures`"
    );
    (dir, Manifest::load_default().unwrap())
}

fn parse(text: &str) -> NaiveDateTime {
    NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S").unwrap()
}

#[test]
fn taken_at_precedence() {
    let (dir, manifest) = setup();
    let mut sources = std::collections::BTreeMap::<String, usize>::new();
    for spec in manifest
        .files
        .iter()
        .filter(|f| f.path.starts_with("basic/") && f.expect.status == Status::Ok)
    {
        let path = dir.join(&spec.path);
        let name = path.file_name().unwrap().to_string_lossy().into_owned();
        let meta = extract_image(&path, &name, i64::try_from(manifest.mtime_unix).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", spec.path));
        let expected_src = spec.expect.taken_at_src.as_deref().unwrap();
        assert_eq!(meta.taken_at.source.as_str(), expected_src, "{}", spec.path);
        let expected_at = match &spec.expect.taken_at {
            Some(text) => parse(text),
            None => mtime_to_naive_local(i64::try_from(manifest.mtime_unix).unwrap()).unwrap(),
        };
        assert_eq!(meta.taken_at.at, expected_at, "{}", spec.path);
        *sources.entry(expected_src.to_owned()).or_default() += 1;
    }
    for src in ["exif", "filename", "mtime"] {
        assert!(
            sources.get(src).copied().unwrap_or(0) > 0,
            "no fixture exercises {src}: {sources:?}"
        );
    }
}

#[test]
fn gps_parsed_to_decimal_degrees() {
    let (dir, manifest) = setup();
    let mut with_gps = 0;
    for spec in manifest.files.iter().filter(|f| f.expect.gps.is_some()) {
        let [lat, lon] = spec.expect.gps.unwrap();
        let exif = read_exif(&dir.join(&spec.path));
        let gps = exif.gps.unwrap_or_else(|| panic!("{}: no gps", spec.path));
        assert!(
            (gps.lat - lat).abs() < 1e-6,
            "{}: {} vs {lat}",
            spec.path,
            gps.lat
        );
        assert!(
            (gps.lon - lon).abs() < 1e-6,
            "{}: {} vs {lon}",
            spec.path,
            gps.lon
        );
        with_gps += 1;
    }
    assert!(with_gps >= 2, "Lisbon and Porto fixtures");
    let southern = manifest
        .files
        .iter()
        .filter_map(|f| f.expect.gps)
        .any(|[lat, lon]| lat < 0.0 || lon < 0.0);
    assert!(southern, "western longitudes exercise the sign handling");
    let no_gps = read_exif(&dir.join("basic/2021/beach/beach_01.jpg"));
    assert!(no_gps.gps.is_none());
}

#[test]
fn exif_fields_match_manifest() {
    let (dir, manifest) = setup();
    for spec in manifest
        .files
        .iter()
        .filter(|f| f.exif.is_some() && f.path.ends_with(".jpg"))
    {
        let want = spec.exif.as_ref().unwrap();
        let got = read_exif(&dir.join(&spec.path));
        assert_eq!(got.make, want.make, "{}", spec.path);
        assert_eq!(got.model, want.model, "{}", spec.path);
        assert_eq!(got.lens, want.lens, "{}", spec.path);
        assert_eq!(got.iso, want.iso.map(u32::from), "{}", spec.path);
        assert_eq!(got.f_number, want.f_number, "{}", spec.path);
        assert_eq!(got.focal_mm, want.focal_mm, "{}", spec.path);
        assert_eq!(got.offset, want.offset, "{}", spec.path);
        assert_eq!(
            got.orientation,
            want.orientation.map_or(1, |o| u8::try_from(o).unwrap()),
            "{}",
            spec.path
        );
        assert_eq!(got.description, want.description, "{}", spec.path);
        assert_eq!(
            got.date_time_original,
            want.taken.as_deref().map(parse),
            "{}",
            spec.path
        );
    }
}

#[test]
fn exposure_is_a_fraction_of_a_second() {
    let (dir, _) = setup();
    let exif = read_exif(&dir.join("basic/2021/beach/beach_01.jpg"));
    assert!((exif.exposure_s.unwrap() - 1.0 / 250.0).abs() < 1e-9);
}

#[test]
fn files_without_exif_give_defaults() {
    let (dir, _) = setup();
    for rel in [
        "basic/misc/sample.png",
        "basic/misc/zero_byte.jpg",
        "basic/misc/not_an_image.jpg",
        "does/not/exist.jpg",
    ] {
        let exif = read_exif(&dir.join(rel));
        assert_eq!(exif.orientation, 1, "{rel}");
        assert!(
            exif.make.is_none() && exif.gps.is_none() && exif.date_time_original.is_none(),
            "{rel}"
        );
    }
}
