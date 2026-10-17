use super::manifest::{ExifSpec, Kind, Manifest, Status};
use super::*;
use exif::{In, Reader, Tag, Value};
use std::collections::BTreeMap;

fn manifest() -> Manifest {
    Manifest::load_default().unwrap()
}

fn tree(root: &Path) -> BTreeMap<String, Vec<u8>> {
    fn walk(dir: &Path, root: &Path, out: &mut BTreeMap<String, Vec<u8>>) {
        for entry in std::fs::read_dir(dir).unwrap() {
            let p = entry.unwrap().path();
            if p.is_dir() {
                walk(&p, root, out);
            } else {
                let rel = p
                    .strip_prefix(root)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/");
                out.insert(rel, std::fs::read(&p).unwrap());
            }
        }
    }
    let mut out = BTreeMap::new();
    walk(root, root, &mut out);
    out
}

fn generated() -> (tempfile::TempDir, Manifest) {
    let m = manifest();
    let dir = tempfile::tempdir().unwrap();
    generate(&m, &dir.path().join("gen")).unwrap();
    (dir, m)
}

#[test]
fn fixtures_are_deterministic() {
    let m = manifest();
    let (a, b) = (tempfile::tempdir().unwrap(), tempfile::tempdir().unwrap());
    generate(&m, &a.path().join("gen")).unwrap();
    generate(&m, &b.path().join("gen")).unwrap();
    let (ta, tb) = (tree(&a.path().join("gen")), tree(&b.path().join("gen")));
    assert_eq!(ta.keys().collect::<Vec<_>>(), tb.keys().collect::<Vec<_>>());
    for (path, bytes) in &ta {
        assert!(bytes == &tb[path], "{path} differs between runs");
    }
    assert_eq!(
        ta.len(),
        m.files.len() + 1,
        "every manifest file plus the marker"
    );
}

fn ascii(exif: &exif::Exif, tag: Tag) -> Option<String> {
    match &exif.get_field(tag, In::PRIMARY)?.value {
        Value::Ascii(v) => Some(String::from_utf8_lossy(v.first()?).into_owned()),
        _ => None,
    }
}

fn rational(exif: &exif::Exif, tag: Tag) -> Vec<f64> {
    match exif.get_field(tag, In::PRIMARY).map(|f| &f.value) {
        Some(Value::Rational(v)) => v
            .iter()
            .map(|r| f64::from(r.num) / f64::from(r.denom))
            .collect(),
        _ => Vec::new(),
    }
}

fn decimal(exif: &exif::Exif, value: Tag, reference: Tag) -> f64 {
    let dms = rational(exif, value);
    let sign = match ascii(exif, reference).as_deref() {
        Some("S" | "W") => -1.0,
        _ => 1.0,
    };
    sign * (dms[0] + dms[1] / 60.0 + dms[2] / 3600.0)
}

fn check_exif(path: &str, spec: &ExifSpec, bytes: &[u8]) {
    let exif = Reader::new()
        .read_from_container(&mut Cursor::new(bytes))
        .unwrap_or_else(|e| panic!("{path}: {e}"));
    assert_eq!(ascii(&exif, Tag::Make), spec.make, "{path} make");
    assert_eq!(ascii(&exif, Tag::Model), spec.model, "{path} model");
    assert_eq!(ascii(&exif, Tag::LensModel), spec.lens, "{path} lens");
    assert_eq!(
        ascii(&exif, Tag::OffsetTimeOriginal),
        spec.offset,
        "{path} offset"
    );
    assert_eq!(
        ascii(&exif, Tag::ImageDescription),
        spec.description,
        "{path} description"
    );
    let taken = spec
        .taken
        .as_ref()
        .map(|t| t.replacen('-', ":", 2).replace('T', " "));
    assert_eq!(
        ascii(&exif, Tag::DateTimeOriginal),
        taken,
        "{path} DateTimeOriginal"
    );
    assert_eq!(ascii(&exif, Tag::DateTime), taken, "{path} DateTime");
    assert_eq!(
        ascii(&exif, Tag::DateTimeDigitized),
        taken,
        "{path} DateTimeDigitized"
    );
    let orientation = exif
        .get_field(Tag::Orientation, In::PRIMARY)
        .and_then(|f| f.value.get_uint(0));
    assert_eq!(
        orientation,
        spec.orientation.map(u32::from),
        "{path} orientation"
    );
    let iso = exif
        .get_field(Tag::PhotographicSensitivity, In::PRIMARY)
        .and_then(|f| f.value.get_uint(0));
    assert_eq!(iso, spec.iso.map(u32::from), "{path} iso");
    if let Some(f) = spec.f_number {
        assert!(
            (rational(&exif, Tag::FNumber)[0] - f).abs() < 1e-3,
            "{path} f-number"
        );
    }
    if let Some(f) = spec.focal_mm {
        assert!(
            (rational(&exif, Tag::FocalLength)[0] - f).abs() < 1e-3,
            "{path} focal length"
        );
    }
    if spec.exposure.as_deref() == Some("1/250") {
        assert!(
            (rational(&exif, Tag::ExposureTime)[0] - 1.0 / 250.0).abs() < 1e-9,
            "{path} exposure"
        );
    }
    match spec.gps {
        Some([lat, lon]) => {
            assert!(
                (decimal(&exif, Tag::GPSLatitude, Tag::GPSLatitudeRef) - lat).abs() < 1e-6,
                "{path} lat"
            );
            assert!(
                (decimal(&exif, Tag::GPSLongitude, Tag::GPSLongitudeRef) - lon).abs() < 1e-6,
                "{path} lon"
            );
            if let Some(alt) = spec.altitude {
                assert!(
                    (rational(&exif, Tag::GPSAltitude)[0] - alt).abs() < 1e-3,
                    "{path} altitude"
                );
            }
        }
        None => assert!(
            exif.get_field(Tag::GPSLatitude, In::PRIMARY).is_none(),
            "{path} has unexpected GPS"
        ),
    }
}

#[test]
fn fixtures_exif_readback() {
    let (dir, m) = generated();
    let root = dir.path().join("gen");
    let with_exif: Vec<_> = m.files.iter().filter(|f| f.exif.is_some()).collect();
    assert!(with_exif.len() >= 10, "beach (6) + city (4) carry EXIF");
    for f in with_exif {
        let bytes = std::fs::read(root.join(&f.path)).unwrap();
        check_exif(&f.path, f.exif.as_ref().unwrap(), &bytes);
    }
    let gps: Vec<_> = m
        .files
        .iter()
        .filter_map(|f| f.exif.as_ref()?.gps)
        .collect();
    assert_eq!(gps.len(), 2);
    assert!(gps.contains(&[38.7223, -9.1393]) && gps.contains(&[41.1579, -8.6291]));
}

#[test]
fn exif_writer_handles_south_west_and_below_sea_level() {
    let spec = ExifSpec {
        make: Some("M".into()),
        gps: Some([-33.8688, -70.6693]),
        altitude: Some(-10.0),
        taken: Some("1999-12-31T23:59:59".into()),
        exposure: Some("0.5".into()),
        ..ExifSpec::default()
    };
    let img = image::RgbImage::new(8, 8);
    let mut jpeg = Vec::new();
    image::codecs::jpeg::JpegEncoder::new(&mut jpeg)
        .encode_image(&img)
        .unwrap();
    let bytes = exif_writer::insert_exif(&jpeg, &spec).unwrap();
    let exif = Reader::new()
        .read_from_container(&mut Cursor::new(&bytes))
        .unwrap();
    assert!((decimal(&exif, Tag::GPSLatitude, Tag::GPSLatitudeRef) + 33.8688).abs() < 1e-6);
    assert!((decimal(&exif, Tag::GPSLongitude, Tag::GPSLongitudeRef) + 70.6693).abs() < 1e-6);
    assert_eq!(rational(&exif, Tag::GPSAltitude), vec![10.0]);
    assert_eq!(
        exif.get_field(Tag::GPSAltitudeRef, In::PRIMARY)
            .and_then(|f| f.value.get_uint(0)),
        Some(1)
    );
    assert_eq!(rational(&exif, Tag::ExposureTime), vec![0.5]);
    assert!(
        image::load_from_memory(&bytes).is_ok(),
        "image still decodes with EXIF inserted"
    );
}

#[test]
fn exif_writer_rejects_bad_input() {
    let spec = ExifSpec {
        orientation: Some(9),
        ..ExifSpec::default()
    };
    assert!(exif_writer::build_exif_payload(&spec).is_err());
    assert!(exif_writer::insert_exif(b"nope", &ExifSpec::default()).is_err());
}

#[test]
fn fixtures_match_spec_minimums() {
    let m = manifest();
    let count = |prefix: &str, status| m.files_with(prefix, status).count();
    assert_eq!(count("basic/2021/beach/", Status::Ok), 6);
    assert_eq!(count("basic/2022/city/", Status::Ok), 6);
    assert_eq!(
        count("basic/misc/", Status::Ok) + count("basic/misc/", Status::Unreadable),
        11
    );
    assert_eq!(count("basic/misc/", Status::Unreadable), 3);
    assert_eq!(count("basic/dupes/", Status::Ok), 3);
    assert_eq!(count("basic/picasa/", Status::Ok), 4);
    assert_eq!(count("basic/.hidden/", Status::Ignored), 1);
    assert_eq!(count("basic/@eaDir/", Status::Ignored), 1);
    let beach_gps = m
        .files_with("basic/2021/beach/", Status::Ok)
        .filter(|f| f.expect.gps.is_some())
        .count();
    assert_eq!(beach_gps, 2);
    let orientations: Vec<_> = m
        .files_with("basic/2022/city/", Status::Ok)
        .filter_map(|f| f.exif.as_ref()?.orientation)
        .collect();
    assert_eq!(orientations, vec![1, 3, 6, 8]);
    assert!(m.files.iter().any(|f| f.path.ends_with("café ☕ 写真.jpg")));
    assert!(
        m.files
            .iter()
            .any(|f| f.path.ends_with("large_6000x4000.jpg") && f.width == Some(6000))
    );
    assert!(
        m.files
            .iter()
            .any(|f| f.path.ends_with("tiny_1x1.png") && f.width == Some(1))
    );
    let paths: std::collections::BTreeSet<_> = m.files.iter().map(|f| &f.path).collect();
    assert_eq!(paths.len(), m.files.len(), "manifest paths are unique");
}

#[test]
fn fixtures_decode_as_manifest_expects() {
    let (dir, m) = generated();
    let root = dir.path().join("gen");
    for f in &m.files {
        let path = root.join(&f.path);
        match f.expect.status {
            Status::Ok => {
                let img = image::open(&path).unwrap_or_else(|e| panic!("{}: {e}", f.path));
                let swapped = matches!(f.exif.as_ref().and_then(|e| e.orientation), Some(5..=8));
                let (w, h) = if swapped {
                    (img.height(), img.width())
                } else {
                    (img.width(), img.height())
                };
                assert_eq!(
                    (Some(w), Some(h)),
                    (f.expect.width, f.expect.height),
                    "{}",
                    f.path
                );
            }
            // The `image` JPEG decoder (zune-jpeg) tolerates truncation and returns a partial
            // image, so truncation is verified structurally in
            // `fixtures_truncated_jpeg_is_shorter_than_full`; lb-media (M2.1) must detect it.
            Status::Unreadable if f.kind == Kind::TruncatedJpeg => {}
            Status::Unreadable => {
                assert!(image::open(&path).is_err(), "{} must not decode", f.path)
            }
            Status::Ignored => {}
        }
    }
}

#[test]
fn fixtures_truncated_jpeg_is_shorter_than_full() {
    let (dir, m) = generated();
    let root = dir.path().join("gen");
    let t = m
        .files
        .iter()
        .find(|f| f.kind == Kind::TruncatedJpeg)
        .unwrap();
    let bytes = std::fs::read(root.join(&t.path)).unwrap();
    assert_eq!(&bytes[..2], &[0xFF, 0xD8], "starts like a JPEG");
    assert!(bytes.len() > 100);
    assert_ne!(
        &bytes[bytes.len() - 2..],
        &[0xFF, 0xD9],
        "missing EOI marker"
    );
}

#[test]
fn fixtures_dupes_copy_is_byte_identical_and_variants_differ() {
    let (dir, _) = generated();
    let root = dir.path().join("gen/basic");
    let original = std::fs::read(root.join("2021/beach/beach_01.jpg")).unwrap();
    assert!(original == std::fs::read(root.join("dupes/beach_01_copy.jpg")).unwrap());
    assert!(original != std::fs::read(root.join("dupes/beach_01_q60.jpg")).unwrap());
    let half = image::open(root.join("dupes/beach_01_half.jpg")).unwrap();
    assert_eq!((half.width(), half.height()), (320, 240));
}

#[test]
fn fixtures_have_manifest_mtime_and_gif_has_three_frames() {
    use image::AnimationDecoder;
    let (dir, m) = generated();
    let root = dir.path().join("gen");
    let expected = SystemTime::UNIX_EPOCH + Duration::from_secs(m.mtime_unix);
    for f in &m.files {
        let mtime = std::fs::metadata(root.join(&f.path))
            .unwrap()
            .modified()
            .unwrap();
        assert_eq!(mtime, expected, "{}", f.path);
    }
    let gif = image::codecs::gif::GifDecoder::new(std::io::BufReader::new(
        std::fs::File::open(root.join("basic/misc/animated.gif")).unwrap(),
    ))
    .unwrap();
    assert_eq!(gif.into_frames().count(), 3);
}

#[test]
fn generate_refuses_to_wipe_foreign_directory() {
    let m = manifest();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("precious.txt"), "keep me").unwrap();
    let err = generate(&m, dir.path()).unwrap_err();
    assert!(err.to_string().contains("refusing"), "{err}");
    assert!(dir.path().join("precious.txt").exists());
}

#[test]
fn manifest_rejects_path_traversal_and_bad_version() {
    let bad = Manifest::parse("version = 2\nmtime_unix = 0\nfile = []").unwrap_err();
    assert!(bad.to_string().contains("version"), "{bad}");
    let m = Manifest::parse(
        "version = 1\nmtime_unix = 0\n[[file]]\npath = \"../evil.txt\"\nkind = \"text\"\ncontents = \"x\"\n[file.expect]\nstatus = \"ignored\"\n",
    )
    .unwrap();
    let dir = tempfile::tempdir().unwrap();
    assert!(generate(&m, &dir.path().join("gen")).is_err());
    assert!(!dir.path().join("evil.txt").exists());
}
