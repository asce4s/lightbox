//! M6.6: export pipeline (size, edits, metadata, watermark, collisions, cancellation).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use lb_app::{
    AppEvent, CollisionPolicy, ExportFormat, ExportOptions, ExportSize, Lightbox, MetadataPolicy,
};
use lb_core::{EditOp, PhotoId, RootMode};
use lb_jobs::JobQueue;
use lb_testkit::fixtures::exif_writer::insert_exif;
use lb_testkit::fixtures::manifest::ExifSpec;
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::time::Duration;

struct Lib {
    _invariants: lb_testkit::Invariants,
    _home: tempfile::TempDir,
    src: tempfile::TempDir,
    out: tempfile::TempDir,
    lb: Lightbox,
}

fn exif() -> ExifSpec {
    ExifSpec {
        make: Some("Canon".into()),
        model: Some("EOS Test".into()),
        orientation: Some(6),
        gps: Some([48.8584, 2.2945]),
        taken: Some("2021-06-15T12:00:00".into()),
        ..ExifSpec::default()
    }
}

/// A 200x100 stored image with EXIF orientation 6, so it displays as 100x200.
fn write_photo(path: &Path, with_exif: bool) {
    let img = image::RgbImage::from_fn(200, 100, |x, y| {
        image::Rgb([(x + 40).min(255) as u8, (y * 2) as u8, 90])
    });
    let mut cur = std::io::Cursor::new(Vec::new());
    img.write_to(&mut cur, image::ImageFormat::Jpeg).unwrap();
    let mut bytes = cur.into_inner();
    if with_exif {
        bytes = insert_exif(&bytes, &exif()).unwrap();
    }
    std::fs::write(path, bytes).unwrap();
}

impl Lib {
    fn new(names: &[&str]) -> Self {
        let home = tempfile::tempdir().unwrap();
        let src = tempfile::tempdir().unwrap();
        for n in names {
            let path = src.path().join(n);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            write_photo(&path, true);
        }
        let lb = Lightbox::open_in(home.path(), JobQueue::inline()).unwrap();
        lb.library().add_root(src.path(), RootMode::Watch).unwrap();
        Self {
            _invariants: common::invariants(home.path(), &[src.path()]),
            _home: home,
            src,
            out: tempfile::tempdir().unwrap(),
            lb,
        }
    }

    fn id(&self, name: &str) -> PhotoId {
        self.lb
            .photos()
            .all()
            .unwrap()
            .into_iter()
            .find(|p| p.path == self.src.path().join(name))
            .unwrap()
            .id
    }

    fn opts(&self) -> ExportOptions {
        ExportOptions::new(self.out.path())
    }

    fn export(&self, name: &str, opts: &ExportOptions) -> PathBuf {
        let report = self.lb.export().export_now(&[self.id(name)], opts).unwrap();
        assert!(report.failed.is_empty(), "{:?}", report.failed);
        report.written[0].path.clone()
    }
}

fn dims(path: &Path) -> (u32, u32) {
    image::image_dimensions(path).unwrap()
}

fn exif_of(path: &Path) -> lb_media::ExifData {
    lb_media::read_exif(path)
}

fn files_in(dir: &Path) -> Vec<String> {
    let mut v: Vec<String> = std::fs::read_dir(dir)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

#[test]
fn export_resizes_long_edge() {
    let lib = Lib::new(&["a.jpg"]);
    let mut opts = lib.opts();
    opts.size = ExportSize::LongEdge(50);
    let out = lib.export("a.jpg", &opts);
    assert_eq!(
        dims(&out),
        (25, 50),
        "orientation is baked in, then the long edge is 50"
    );

    opts.size = ExportSize::LongEdge(1600);
    opts.collision = CollisionPolicy::Overwrite;
    let big = lib.export("a.jpg", &opts);
    assert_eq!(dims(&big), (100, 200), "never upscaled");

    opts.size = ExportSize::Original;
    assert_eq!(dims(&lib.export("a.jpg", &opts)), (100, 200));

    opts.quality = 0;
    assert!(
        lib.lb
            .export()
            .export_now(&[lib.id("a.jpg")], &opts)
            .is_err(),
        "quality 0 is rejected"
    );
    opts.quality = 85;
    opts.size = ExportSize::LongEdge(0);
    assert!(opts.validate().is_err());
}

#[test]
fn export_applies_edits() {
    let lib = Lib::new(&["a.jpg"]);
    let id = lib.id("a.jpg");
    let original = std::fs::read(lib.src.path().join("a.jpg")).unwrap();
    lib.lb
        .edits()
        .push(id, EditOp::RotateQuarter { turns: 1 })
        .unwrap();
    let mut opts = lib.opts();
    let edited = lib.export("a.jpg", &opts);
    assert_eq!(dims(&edited), (200, 100), "rotation baked into the pixels");

    opts.apply_edits = false;
    opts.collision = CollisionPolicy::Overwrite;
    let plain = lib.export("a.jpg", &opts);
    assert_eq!(plain, edited);
    assert_eq!(
        dims(&plain),
        (100, 200),
        "edits off exports the original look"
    );

    opts.apply_edits = true;
    opts.size = ExportSize::LongEdge(100);
    let small = lib.export("a.jpg", &opts);
    assert_eq!(dims(&small), (100, 50), "edits first, then resize");
    assert_eq!(
        std::fs::read(lib.src.path().join("a.jpg")).unwrap(),
        original,
        "the original file is never modified"
    );
}

#[test]
fn export_strip_gps() {
    let lib = Lib::new(&["a.jpg"]);
    let mut opts = lib.opts();
    opts.collision = CollisionPolicy::Overwrite;

    let default = lib.export("a.jpg", &opts);
    let data = exif_of(&default);
    assert_eq!(data.gps, None, "GPS is dropped by default");
    assert_eq!(data.make.as_deref(), Some("Canon"), "other EXIF is kept");
    assert!(data.date_time_original.is_some());
    let raw = std::fs::read(&default).unwrap();
    assert!(
        !raw.windows(5).any(|w| w == b"GPS\0\0"),
        "no GPS version bytes"
    );

    opts.metadata = MetadataPolicy::KeepAll;
    assert!(exif_of(&lib.export("a.jpg", &opts)).gps.is_some());

    opts.metadata = MetadataPolicy::StripAll;
    let stripped = lib.export("a.jpg", &opts);
    let data = exif_of(&stripped);
    assert_eq!((data.gps, data.make), (None, None), "no EXIF at all");
    assert!(lb_media::raw_exif(&stripped).is_none());
}

#[test]
fn export_keep_exif_resets_orientation() {
    let lib = Lib::new(&["a.jpg"]);
    let mut opts = lib.opts();
    opts.metadata = MetadataPolicy::KeepAll;
    let out = lib.export("a.jpg", &opts);
    assert_eq!(exif_of(&out).orientation, 1, "orientation baked, so reset");
    assert_eq!(dims(&out), (100, 200));
    let decoded = lb_media::decode(&out, None).unwrap();
    assert_eq!((decoded.width(), decoded.height()), (100, 200));

    opts.format = ExportFormat::KeepOriginal;
    opts.collision = CollisionPolicy::Overwrite;
    let untouched = lib.export("a.jpg", &opts);
    assert_eq!(
        std::fs::read(&untouched).unwrap(),
        std::fs::read(lib.src.path().join("a.jpg")).unwrap(),
        "an untouched JPEG with all metadata is copied byte for byte"
    );
    assert_eq!(
        exif_of(&untouched).orientation,
        6,
        "pixels not rotated, tag kept"
    );

    opts.metadata = MetadataPolicy::KeepExifNoGps;
    let lossless = lib.export("a.jpg", &opts);
    let (a, b) = (
        image::open(&lossless).unwrap().to_rgb8(),
        image::open(lib.src.path().join("a.jpg")).unwrap().to_rgb8(),
    );
    assert_eq!(a, b, "metadata-only rewrite keeps the pixels identical");
    assert_eq!(exif_of(&lossless).gps, None);
    assert_eq!(exif_of(&lossless).orientation, 6);
}

#[test]
fn export_watermark_present() {
    let lib = Lib::new(&["a.jpg"]);
    let mut opts = lib.opts();
    opts.size = ExportSize::Original;
    let plain = image::open(lib.export("a.jpg", &opts)).unwrap().to_rgb8();
    opts.watermark = Some("Lightbox".into());
    opts.collision = CollisionPolicy::Rename;
    let marked_path = lib.export("a.jpg", &opts);
    assert!(marked_path.to_string_lossy().contains("(1)"));
    let marked = image::open(&marked_path).unwrap().to_rgb8();
    assert_eq!(plain.dimensions(), marked.dimensions());

    let (w, h) = plain.dimensions();
    let diff = |x: u32, y: u32| {
        let (a, b) = (plain.get_pixel(x, y), marked.get_pixel(x, y));
        a.0.iter()
            .zip(b.0)
            .map(|(p, q)| i32::from(*p).abs_diff(i32::from(q)))
            .max()
            .unwrap()
    };
    let corner: u32 = (w / 2..w)
        .flat_map(|x| (h * 3 / 4..h).map(move |y| (x, y)))
        .filter(|&(x, y)| diff(x, y) > 20)
        .count() as u32;
    let elsewhere: u32 = (0..w)
        .flat_map(|x| (0..h / 2).map(move |y| (x, y)))
        .filter(|&(x, y)| diff(x, y) > 20)
        .count() as u32;
    assert!(
        corner > 10,
        "watermark pixels in the bottom-right: {corner}"
    );
    assert_eq!(elsewhere, 0, "top half untouched");
}

#[test]
fn export_collision_policy() {
    let lib = Lib::new(&["a/p.jpg", "b/p.jpg"]);
    let ids = [lib.id("a/p.jpg"), lib.id("b/p.jpg")];
    let svc = lib.lb.export();
    let mut opts = lib.opts();

    let r = svc.export_now(&ids, &opts).unwrap();
    assert_eq!(r.written.len(), 2);
    assert_eq!(
        files_in(lib.out.path()),
        ["p (1).jpg", "p.jpg"],
        "two sources with one name never overwrite each other within a run"
    );

    let r = svc.export_now(&ids[..1], &opts).unwrap();
    assert!(r.written[0].path.ends_with("p (2).jpg"));

    opts.collision = CollisionPolicy::Skip;
    let before = std::fs::read(lib.out.path().join("p.jpg")).unwrap();
    let r = svc.export_now(&ids[..1], &opts).unwrap();
    assert_eq!((r.written.len(), r.skipped.len()), (0, 1));

    opts.collision = CollisionPolicy::Overwrite;
    opts.size = ExportSize::LongEdge(40);
    let r = svc.export_now(&ids[..1], &opts).unwrap();
    assert_eq!(r.written.len(), 1);
    assert_eq!(dims(&lib.out.path().join("p.jpg")), (20, 40));
    assert_ne!(std::fs::read(lib.out.path().join("p.jpg")).unwrap(), before);
    assert_eq!(files_in(lib.out.path()).len(), 3, "no extra files");
}

#[test]
fn export_cancel_cleans_temp() {
    let lib = Lib::new(&["a.jpg", "b.jpg", "c.jpg"]);
    let ids = [lib.id("a.jpg"), lib.id("b.jpg"), lib.id("c.jpg")];
    let svc = lib.lb.export();
    let opts = lib.opts();

    for stop_at in 1..=12 {
        let checks = Cell::new(0);
        let report = svc
            .run(
                &ids,
                &opts,
                &|| {
                    checks.set(checks.get() + 1);
                    checks.get() >= stop_at
                },
                &|_, _| {},
            )
            .unwrap();
        let files = files_in(lib.out.path());
        assert!(
            files.iter().all(|f| !f.contains("lbtmp")),
            "stop at check {stop_at}: temp files left: {files:?}"
        );
        if stop_at <= 10 {
            assert!(report.cancelled, "stop at {stop_at}");
        }
        for f in &files {
            assert!(
                image::open(lib.out.path().join(f)).is_ok(),
                "every file present is complete"
            );
        }
        for f in files {
            std::fs::remove_file(lib.out.path().join(f)).unwrap();
        }
    }
}

#[test]
fn export_job_reports_progress_and_finishes() {
    let lib = Lib::new(&["a.jpg", "b.jpg"]);
    let rx = lib.lb.events();
    let ids = [lib.id("a.jpg"), lib.id("b.jpg")];
    let handle = lib.lb.export().start(&ids, lib.opts()).unwrap();
    let report = handle.wait(Duration::from_secs(5)).unwrap();
    assert_eq!(report.written.len(), 2);
    lib.lb.bus().flush();
    let events: Vec<AppEvent> = rx.try_iter().collect();
    let progress: Vec<f32> = events
        .iter()
        .filter_map(|e| match e {
            AppEvent::JobProgress {
                progress,
                kind: "export",
                ..
            } => Some(*progress),
            _ => None,
        })
        .collect();
    assert_eq!(progress, [0.5, 1.0]);
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AppEvent::JobFinished { result: Ok(()), .. }))
    );
}

#[test]
fn export_png_and_failures() {
    let lib = Lib::new(&["a.jpg"]);
    let mut opts = lib.opts();
    opts.format = ExportFormat::Png;
    let png = lib.export("a.jpg", &opts);
    assert_eq!(png.extension().unwrap(), "png");
    assert_eq!(dims(&png), (100, 200));

    let report = lib
        .lb
        .export()
        .export_now(&[PhotoId::new(9999)], &opts)
        .unwrap();
    assert_eq!(
        report.failed.len(),
        1,
        "unknown photos are reported, not fatal"
    );
    assert!(report.written.is_empty());

    std::fs::remove_file(lib.src.path().join("a.jpg")).unwrap();
    lib.lb.library().rescan_all().unwrap();
    let report = lib
        .lb
        .export()
        .export_now(&[lib.id("a.jpg")], &opts)
        .unwrap();
    assert_eq!(report.failed.len(), 1, "missing files are reported");
}
