//! M10.4: legacy `.picasa.ini` import (SPEC §6.17).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use lb_app::{Lightbox, LightboxOptions, PicasaOptions, ViewSource};
use lb_core::{EditOp, PhotoId, RootMode};
use lb_jobs::JobQueue;
use lb_testkit::fixtures;
use std::path::{Path, PathBuf};

struct Imported {
    _invariants: lb_testkit::Invariants,
    _home: tempfile::TempDir,
    dir: tempfile::TempDir,
    lb: Lightbox,
}

impl Imported {
    fn root(&self) -> PathBuf {
        self.dir.path().join("picasa")
    }

    fn id(&self, name: &str) -> PhotoId {
        self.lb
            .photos()
            .all()
            .unwrap()
            .into_iter()
            .find(|p| p.path.file_name().is_some_and(|n| n == name))
            .unwrap()
            .id
    }
}

fn setup() -> Imported {
    let generated = fixtures::default_generated_dir();
    assert!(
        generated.join(fixtures::MARKER).is_file(),
        "run `cargo xtask fixtures` first"
    );
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    fixtures::copy_tree(&generated.join("basic/picasa"), &dir.path().join("picasa")).unwrap();
    let options = LightboxOptions {
        spawn_event_ticker: false,
        ..LightboxOptions::default()
    };
    let lb = Lightbox::open_with(home.path(), JobQueue::inline(), options).unwrap();
    lb.library()
        .add_root(&dir.path().join("picasa"), RootMode::Watch)
        .unwrap();
    Imported {
        _invariants: common::invariants(home.path(), &[&dir.path().join("picasa")]),
        _home: home,
        dir,
        lb,
    }
}

fn snapshot(t: &Imported) -> String {
    let mut out = String::new();
    for p in t.lb.photos().all().unwrap() {
        let tags: Vec<String> =
            t.lb.tags()
                .for_photo(p.id)
                .unwrap()
                .into_iter()
                .map(|t| t.name)
                .collect();
        let stack = t.lb.edits().stack(p.id).unwrap();
        out.push_str(&format!(
            "{:?} {} {:?} {:?} {:?}\n",
            p.path.file_name().unwrap(),
            p.starred,
            p.caption,
            tags,
            stack.ops
        ));
    }
    for a in t.lb.albums().list().unwrap() {
        out.push_str(&format!("{} {:?}\n", a.name, t.lb.albums().photo_ids(a.id)));
    }
    out
}

#[test]
fn picasa_import_idempotent() {
    let t = setup();
    let first =
        t.lb.picasa()
            .import(&t.root(), PicasaOptions::default())
            .unwrap();
    assert!(!first.is_empty());
    assert_eq!(first.ini_files, 1);
    assert_eq!(first.photos_matched, 4);
    assert_eq!(first.starred, 2);
    assert_eq!(first.captions, 2);
    assert_eq!(first.albums_created, 2);
    assert_eq!(first.album_links, 2);
    assert_eq!(first.edits, 2);
    assert_eq!(first.folder_captions, 1);
    assert_eq!(first.skipped_filters, vec!["madeupfilter".to_string()]);

    let before = snapshot(&t);
    let second =
        t.lb.picasa()
            .import(&t.root(), PicasaOptions::default())
            .unwrap();
    assert!(second.is_empty(), "{second:?}");
    assert_eq!(snapshot(&t), before);
}

#[test]
fn picasa_preview_matches_import() {
    let t = setup();
    let before = snapshot(&t);
    let preview =
        t.lb.picasa()
            .preview(&t.root(), PicasaOptions::default())
            .unwrap();
    assert_eq!(snapshot(&t), before, "preview must not change anything");
    let applied =
        t.lb.picasa()
            .import(&t.root(), PicasaOptions::default())
            .unwrap();
    assert_eq!(preview, applied);
}

#[test]
fn picasa_import_fills_metadata() {
    let t = setup();
    t.lb.picasa()
        .import(&t.root(), PicasaOptions::default())
        .unwrap();
    let one = t.lb.photos().get(t.id("IMG_0001.JPG")).unwrap().unwrap();
    assert!(one.starred);
    assert_eq!(one.caption.as_deref(), Some("At the beach"));
    let tags: Vec<String> =
        t.lb.tags()
            .for_photo(one.id)
            .unwrap()
            .into_iter()
            .map(|t| t.name)
            .collect();
    assert!(tags.contains(&"beach".to_string()) && tags.contains(&"family".to_string()));
    let stack = t.lb.edits().stack(one.id).unwrap();
    assert!(matches!(
        stack.ops.first(),
        Some(EditOp::RotateQuarter { .. })
    ));
    assert!(stack.ops.len() >= 4);

    let three = t.lb.photos().get(t.id("IMG_0003.JPG")).unwrap().unwrap();
    assert_eq!(three.caption.as_deref(), Some("Line one\nLine two"));

    let folder =
        t.lb.library()
            .roots()
            .unwrap()
            .into_iter()
            .flat_map(|r| t.lb.library().folders(r.id).unwrap())
            .find(|f| f.path == t.root())
            .unwrap();
    assert_eq!(
        folder.caption.as_deref(),
        Some("Folder caption from Picasa")
    );
}

#[test]
fn picasa_import_does_not_overwrite_by_default() {
    let t = setup();
    let one = t.id("IMG_0001.JPG");
    t.lb.photos().set_caption(&[one], Some("Mine")).unwrap();
    t.lb.edits().push(one, EditOp::BlackAndWhite).unwrap();
    let mine = t.lb.edits().stack(one).unwrap();

    t.lb.picasa()
        .import(&t.root(), PicasaOptions::default())
        .unwrap();
    let photo = t.lb.photos().get(one).unwrap().unwrap();
    assert_eq!(photo.caption.as_deref(), Some("Mine"));
    assert_eq!(t.lb.edits().stack(one).unwrap().ops, mine.ops);

    let report =
        t.lb.picasa()
            .import(&t.root(), PicasaOptions { overwrite: true })
            .unwrap();
    assert_eq!(report.captions, 1);
    assert_eq!(report.edits, 1);
    let photo = t.lb.photos().get(one).unwrap().unwrap();
    assert_eq!(photo.caption.as_deref(), Some("At the beach"));
    assert_ne!(t.lb.edits().stack(one).unwrap().ops, mine.ops);
    let again =
        t.lb.picasa()
            .import(&t.root(), PicasaOptions { overwrite: true })
            .unwrap();
    assert!(again.is_empty(), "{again:?}");
}

#[test]
fn picasa_albums_and_contacts() {
    let t = setup();
    t.lb.picasa()
        .import(&t.root(), PicasaOptions::default())
        .unwrap();
    let albums = t.lb.albums().list().unwrap();
    let mut names: Vec<&str> = albums.iter().map(|a| a.name.as_str()).collect();
    names.sort_unstable();
    assert_eq!(names, ["Family", "Summer 2009"]);
    let one = t.id("IMG_0001.JPG");
    for a in &albums {
        assert_eq!(t.lb.albums().photo_ids(a.id).unwrap(), vec![one]);
    }

    #[cfg(feature = "faces")]
    {
        let faces = t.lb.faces().faces_in_photo(one).unwrap();
        assert_eq!(faces.len(), 2);
        assert!(
            faces
                .iter()
                .all(|f| f.source == lb_core::FaceSource::Picasa)
        );
        let ana = t.lb.faces().persons().unwrap();
        assert_eq!(ana.len(), 1);
        assert_eq!(ana[0].name, "Ana");
        assert_eq!(
            t.lb.photos()
                .list_ids(&ViewSource::Person(ana[0].id), false)
                .unwrap(),
            vec![one]
        );
        assert_eq!(faces.iter().filter(|f| f.person_id.is_some()).count(), 1);
    }
    let _ = ViewSource::AllPhotos;
}

#[test]
fn picasa_import_reports_missing_photos_and_bad_folder() {
    let t = setup();
    std::fs::remove_file(t.root().join("IMG_0004.JPG")).unwrap();
    t.lb.library().rescan_all().unwrap();
    let report =
        t.lb.picasa()
            .import(&t.root(), PicasaOptions::default())
            .unwrap();
    assert_eq!(report.photos_matched, 3);
    assert_eq!(report.photos_missing, 1);
    assert_eq!(report.starred, 1);
    assert!(
        t.lb.picasa()
            .import(Path::new("/definitely/not/here"), PicasaOptions::default())
            .is_err()
    );
}
