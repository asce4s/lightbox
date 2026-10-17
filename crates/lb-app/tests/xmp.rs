//! M12.2: XMP sidecar write-back (SPEC §6.7.6).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use lb_app::XmpSettings;
use std::path::{Path, PathBuf};

fn sidecar(photo: &Path) -> PathBuf {
    let mut name = photo.file_name().unwrap().to_os_string();
    name.push(".xmp");
    photo.with_file_name(name)
}

fn enable(t: &common::TestLib) {
    t.lb.settings()
        .update(|s| {
            s.xmp = XmpSettings {
                write_sidecars: true,
            }
        })
        .unwrap();
}

fn path_of(t: &common::TestLib, i: usize) -> PathBuf {
    t.lb.photos().get(t.ids[i]).unwrap().unwrap().path
}

#[test]
fn xmp_written_next_to_file() {
    let t = common::library(2);
    enable(&t);
    let photo = path_of(&t, 0);
    t.lb.photos().set_rating(&t.ids[..1], 4).unwrap();
    t.lb.tags()
        .add(&t.ids[..1], &["holiday".to_owned(), "sea".to_owned()])
        .unwrap();
    t.lb.photos()
        .set_caption(&t.ids[..1], Some("Sun & <sand> \"fun\""))
        .unwrap();

    let xmp = std::fs::read_to_string(sidecar(&photo)).unwrap();
    assert!(xmp.starts_with("<?xpacket"), "{xmp}");
    assert!(xmp.contains("xmp:Rating=\"4\""), "{xmp}");
    assert!(xmp.contains("<rdf:li>holiday</rdf:li>"), "{xmp}");
    assert!(xmp.contains("<rdf:li>sea</rdf:li>"), "{xmp}");
    assert!(
        xmp.contains(
            "<rdf:li xml:lang=\"x-default\">Sun &amp; &lt;sand&gt; &quot;fun&quot;</rdf:li>"
        ),
        "{xmp}"
    );
    assert!(
        !sidecar(&path_of(&t, 1)).exists(),
        "photos that did not change get no sidecar"
    );

    t.lb.history().undo().unwrap();
    let xmp = std::fs::read_to_string(sidecar(&photo)).unwrap();
    assert!(!xmp.contains("Sun"), "undo is written back: {xmp}");

    t.lb.photos().set_rating(&t.ids[..1], 0).unwrap();
    let xmp = std::fs::read_to_string(sidecar(&photo)).unwrap();
    assert!(xmp.contains("xmp:Rating=\"0\""), "{xmp}");
}

#[test]
fn xmp_is_off_by_default_and_can_be_written_on_request() {
    let t = common::library(1);
    let photo = path_of(&t, 0);
    t.lb.photos().set_rating(&t.ids, 3).unwrap();
    assert!(!sidecar(&photo).exists());

    let report = t.lb.xmp().write(&t.ids).unwrap();
    assert_eq!(report.written, vec![sidecar(&photo)]);
    assert!(report.failed.is_empty());
    let xmp = std::fs::read_to_string(sidecar(&photo)).unwrap();
    assert!(xmp.contains("xmp:Rating=\"3\""), "{xmp}");
    assert!(
        !xmp.contains("dc:subject") && !xmp.contains("dc:description"),
        "{xmp}"
    );
}

#[test]
fn xmp_never_touches_original() {
    let t = common::library(2);
    enable(&t);
    let photo = path_of(&t, 0);
    let before = std::fs::read(&photo).unwrap();
    let modified = std::fs::metadata(&photo).unwrap().modified().unwrap();

    t.lb.photos().set_rating(&t.ids, 5).unwrap();
    t.lb.tags().add(&t.ids, &["x".to_owned()]).unwrap();
    t.lb.xmp().write(&t.ids).unwrap();

    assert_eq!(std::fs::read(&photo).unwrap(), before);
    assert_eq!(
        std::fs::metadata(&photo).unwrap().modified().unwrap(),
        modified
    );
    assert_ne!(sidecar(&photo), photo);

    let summary = t.lb.library().rescan_all().unwrap();
    assert_eq!(
        t.lb.photos().all().unwrap().len(),
        2,
        "sidecars are not photos: {summary:?}"
    );
}

#[test]
fn xmp_reports_files_it_cannot_write() {
    let mut t = common::build(&[("sub", 1)], lb_app::LightboxOptions::default());
    t._invariants.declare_changed(&path_of(&t, 0));
    std::fs::remove_dir_all(t.photos.path().join("sub")).unwrap();
    let report = t.lb.xmp().write(&t.ids).unwrap();
    assert!(report.written.is_empty());
    assert_eq!(report.failed.len(), 1);
}
