//! M10.3: face detection pipeline, clustering and people (SPEC §6.15), with mock networks.
#![cfg(feature = "faces")]
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use image::{Rgb, RgbImage};
use lb_app::{AppEvent, Lightbox, LightboxOptions, ViewQuery, ViewSource};
use lb_core::{NormRect, PeopleKey, PhotoId, RootMode};
use lb_faces::{FacePipeline, draw_mock_face};
use lb_jobs::JobQueue;
use std::path::Path;
use std::sync::Arc;

const ANA: usize = 0;
const BOB: usize = 1;
const CY: usize = 2;

struct Faces {
    _invariants: lb_testkit::Invariants,
    _home: tempfile::TempDir,
    dir: tempfile::TempDir,
    lb: Lightbox,
}

fn write(dir: &Path, name: &str, faces: &[(usize, u32, u32, u32)]) {
    let mut img = RgbImage::from_pixel(320, 240, Rgb([128, 128, 128]));
    for (color, x, y, size) in faces {
        draw_mock_face(&mut img, *color, *x, *y, *size);
    }
    img.save(dir.join(name)).unwrap();
}

fn pipeline_options() -> LightboxOptions {
    LightboxOptions {
        face_pipeline: Some(Arc::new(FacePipeline::mock())),
        spawn_event_ticker: false,
        ..LightboxOptions::default()
    }
}

fn setup() -> Faces {
    let home = tempfile::tempdir().unwrap();
    let dir = tempfile::tempdir().unwrap();
    write(
        dir.path(),
        "a.png",
        &[(ANA, 20, 30, 60), (BOB, 200, 80, 80)],
    );
    write(dir.path(), "b.png", &[(ANA, 100, 40, 50)]);
    write(dir.path(), "c.png", &[(BOB, 40, 100, 70)]);
    write(dir.path(), "d.png", &[(CY, 150, 50, 60)]);
    write(dir.path(), "e.png", &[]);
    write(dir.path(), "f.png", &[(CY, 10, 10, 12)]);
    let lb = Lightbox::open_with(home.path(), JobQueue::inline(), pipeline_options()).unwrap();
    lb.library().add_root(dir.path(), RootMode::Watch).unwrap();
    Faces {
        _invariants: common::invariants(home.path(), &[dir.path()]),
        _home: home,
        dir,
        lb,
    }
}

fn id_of(lb: &Lightbox, name: &str) -> PhotoId {
    lb.photos()
        .all()
        .unwrap()
        .into_iter()
        .find(|p| p.path.file_name().is_some_and(|n| n == name))
        .unwrap()
        .id
}

fn names(lb: &Lightbox, ids: &[PhotoId]) -> Vec<String> {
    let mut v: Vec<String> = lb
        .photos()
        .get_many(ids)
        .unwrap()
        .into_iter()
        .map(|p| p.path.file_name().unwrap().to_string_lossy().into_owned())
        .collect();
    v.sort();
    v
}

fn search(lb: &Lightbox, text: &str) -> Vec<String> {
    let mut view = ViewQuery::new(ViewSource::AllPhotos);
    view.search = text.to_owned();
    names(lb, &lb.search().query(&view).unwrap().ids())
}

#[test]
fn faces_detected_and_stored_normalized() {
    let t = setup();
    let faces = t.lb.faces();
    assert!(faces.available());
    let report = faces.process_pending(None).unwrap();
    assert_eq!(report.photos, 6);
    assert_eq!(report.faces, 5, "the 12 px face is below the 24 px minimum");
    assert_eq!(report.unreadable, 0);

    let a = faces.faces_in_photo(id_of(&t.lb, "a.png")).unwrap();
    assert_eq!(a.len(), 2);
    assert!((a[0].rect.x - 20.0 / 320.0).abs() < 0.01, "{:?}", a[0].rect);
    assert!((a[0].rect.y - 30.0 / 240.0).abs() < 0.01);
    assert!((a[0].rect.w - 60.0 / 320.0).abs() < 0.02);
    assert!((a[0].rect.h - 60.0 / 240.0).abs() < 0.02);
    for f in &a {
        let r = f.rect;
        assert!(r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= 1.0 && r.y + r.h <= 1.0);
        assert!(f.score >= 0.8);
    }
    let stored = t.lb.catalog_handle().face_embeddings().unwrap();
    assert_eq!(stored.len(), 5);
    for e in stored {
        assert_eq!(e.embedding.len(), 128);
        let norm: f32 = e.embedding.iter().map(|v| v * v).sum::<f32>().sqrt();
        assert!(
            (norm - 1.0).abs() < 1e-4,
            "stored embeddings are unit length"
        );
    }

    let again = faces.process_pending(None).unwrap();
    assert_eq!(again.photos, 0, "analysed photos are not analysed again");
    assert_eq!(
        search(&t.lb, "has:faces"),
        ["a.png", "b.png", "c.png", "d.png"]
    );
}

#[test]
fn unreadable_images_are_marked_and_not_retried() {
    let t = setup();
    std::fs::write(
        t.dir.path().join("broken.png"),
        b"\x89PNG\r\n\x1a\nnot really",
    )
    .unwrap();
    t.lb.library().rescan_all().unwrap();
    let report = t.lb.faces().process_pending(None).unwrap();
    assert_eq!(report.faces, 5);
    assert_eq!(t.lb.faces().process_pending(None).unwrap().photos, 0);
}

#[test]
fn cluster_assigns_similar_embeddings() {
    let t = setup();
    let faces = t.lb.faces();
    faces.process_pending(None).unwrap();
    let people = faces.people(false).unwrap();
    assert_eq!(people.len(), 3, "one cluster per mock identity");
    assert!(people.iter().all(|p| p.name.is_none()));
    let counts: Vec<usize> = people.iter().map(|p| p.face_count).collect();
    assert_eq!(counts, [2, 2, 1], "largest unnamed cluster first");

    let (a, b) = (id_of(&t.lb, "a.png"), id_of(&t.lb, "b.png"));
    let cluster = |photo: PhotoId, left_most: bool| {
        let f = faces.faces_in_photo(photo).unwrap();
        if left_most { f[0] } else { f[f.len() - 1] }
            .cluster_id
            .unwrap()
    };
    assert_eq!(
        cluster(a, true),
        cluster(b, true),
        "Ana in a and b share a cluster"
    );
    assert_ne!(cluster(a, true), cluster(a, false), "Ana and Bob do not");

    assert_eq!(faces.recluster().unwrap(), 3, "full re-clustering agrees");
    assert_eq!(
        cluster(a, true),
        cluster(b, true),
        "cluster ids stay stable"
    );
}

#[test]
fn name_cluster_creates_person() {
    let t = setup();
    let faces = t.lb.faces();
    faces.process_pending(None).unwrap();
    let rx = t.lb.events();
    let a = id_of(&t.lb, "a.png");
    let ana_cluster = faces.faces_in_photo(a).unwrap()[0].cluster_id.unwrap();
    let ana = faces
        .name(PeopleKey::Cluster(ana_cluster), "  Ana  ")
        .unwrap();
    assert_eq!(ana.name, "Ana");
    t.lb.bus().tick();
    assert!(rx.try_iter().any(|e| e == AppEvent::PeopleChanged));

    let people = faces.people(false).unwrap();
    assert_eq!(people.len(), 3);
    assert_eq!(
        people.last().unwrap().name.as_deref(),
        Some("Ana"),
        "named people sort after unnamed"
    );
    let named = people.iter().find(|p| p.name.is_some()).unwrap();
    assert_eq!(named.key, PeopleKey::Person(ana.id));
    assert_eq!(named.face_count, 2);
    assert_eq!(named.photo_count, 2);
    assert_eq!(
        names(&t.lb, &faces.photos_of(PeopleKey::Person(ana.id)).unwrap()),
        ["a.png", "b.png"]
    );
    assert_eq!(faces.suggest_names("an", 5).unwrap(), ["Ana"]);
    assert!(faces.suggest_names("zz", 5).unwrap().is_empty());

    write(t.dir.path(), "g.png", &[(ANA, 220, 150, 50)]);
    t.lb.library().rescan_all().unwrap();
    faces.process_pending(None).unwrap();
    let g = faces.faces_in_photo(id_of(&t.lb, "g.png")).unwrap();
    assert_eq!(
        g[0].person_id,
        Some(ana.id),
        "new faces join the named person"
    );
    assert_eq!(
        faces.people(false).unwrap().len(),
        3,
        "no new cluster appeared"
    );
}

#[test]
fn merge_people() {
    let t = setup();
    let faces = t.lb.faces();
    faces.process_pending(None).unwrap();
    let a = faces.faces_in_photo(id_of(&t.lb, "a.png")).unwrap();
    let ana = faces
        .name(PeopleKey::Cluster(a[0].cluster_id.unwrap()), "Ana")
        .unwrap();
    let ann = faces
        .name(PeopleKey::Cluster(a[1].cluster_id.unwrap()), "Anna")
        .unwrap();
    faces.merge(ann.id, ana.id).unwrap();

    let persons = faces.persons().unwrap();
    assert_eq!(persons.len(), 1);
    assert_eq!(persons[0].name, "Ana");
    assert_eq!(
        names(&t.lb, &faces.photos_of(PeopleKey::Person(ana.id)).unwrap()),
        ["a.png", "b.png", "c.png"]
    );
    assert_eq!(search(&t.lb, "person:Anna"), Vec::<String>::new());
    assert_eq!(search(&t.lb, "person:ana"), ["a.png", "b.png", "c.png"]);
    assert!(
        faces.merge(ana.id, ana.id).is_ok(),
        "merging into itself is a no-op"
    );
}

#[test]
fn remove_face_from_person() {
    let t = setup();
    let faces = t.lb.faces();
    faces.process_pending(None).unwrap();
    let b = id_of(&t.lb, "b.png");
    let face = faces.faces_in_photo(b).unwrap()[0];
    let ana = faces
        .name(PeopleKey::Cluster(face.cluster_id.unwrap()), "Ana")
        .unwrap();
    assert_eq!(search(&t.lb, "person:Ana"), ["a.png", "b.png"]);

    faces.remove_from_person(face.id).unwrap();
    let after = faces.faces_in_photo(b).unwrap()[0];
    assert_eq!(after.person_id, None);
    assert_ne!(
        after.cluster_id, face.cluster_id,
        "it does not rejoin the person's cluster"
    );
    assert_eq!(search(&t.lb, "person:Ana"), ["a.png"]);
    assert_eq!(faces.people(false).unwrap().len(), 4);

    faces.assign(face.id, ana.id).unwrap();
    assert_eq!(search(&t.lb, "person:Ana"), ["a.png", "b.png"]);
    faces.delete_face(face.id).unwrap();
    assert!(faces.faces_in_photo(b).unwrap().is_empty());
    assert_eq!(search(&t.lb, "person:Ana"), ["a.png"]);
}

#[test]
fn person_search() {
    let t = setup();
    let faces = t.lb.faces();
    faces.process_pending(None).unwrap();
    let a = faces.faces_in_photo(id_of(&t.lb, "a.png")).unwrap();
    let ana = faces
        .name(PeopleKey::Cluster(a[0].cluster_id.unwrap()), "Ana Maria")
        .unwrap();
    faces
        .name(PeopleKey::Cluster(a[1].cluster_id.unwrap()), "Bob")
        .unwrap();

    assert_eq!(search(&t.lb, "person:\"Ana Maria\""), ["a.png", "b.png"]);
    assert_eq!(search(&t.lb, "person:bob"), ["a.png", "c.png"]);
    assert_eq!(
        search(&t.lb, "Maria"),
        ["a.png", "b.png"],
        "names are in free-text search"
    );
    assert_eq!(search(&t.lb, "person:\"Ana Maria\" person:bob"), ["a.png"]);
    assert_eq!(search(&t.lb, "-person:bob has:faces"), ["b.png", "d.png"]);

    let mut view = ViewQuery::new(ViewSource::Person(ana.id));
    assert_eq!(
        names(&t.lb, &t.lb.search().query(&view).unwrap().ids()),
        ["a.png", "b.png"]
    );
    view.search = "person:bob".into();
    assert_eq!(
        names(&t.lb, &t.lb.search().query(&view).unwrap().ids()),
        ["a.png"]
    );

    faces.name(PeopleKey::Person(ana.id), "Ana").unwrap();
    assert_eq!(search(&t.lb, "person:\"Ana Maria\""), Vec::<String>::new());
    assert_eq!(
        search(&t.lb, "person:ana"),
        ["a.png", "b.png"],
        "renaming updates the index"
    );
}

#[test]
fn hiding_manual_faces_crops_and_detection_reset() {
    let t = setup();
    let faces = t.lb.faces();
    faces.process_pending(None).unwrap();
    let d = id_of(&t.lb, "d.png");
    let cy = faces.faces_in_photo(d).unwrap()[0];
    let person = faces
        .name(PeopleKey::Cluster(cy.cluster_id.unwrap()), "Cy")
        .unwrap();
    faces.set_hidden(person.id, true).unwrap();
    assert!(
        faces
            .people(false)
            .unwrap()
            .iter()
            .all(|p| p.name.is_none()),
        "hidden people are not listed"
    );
    assert_eq!(faces.people(true).unwrap().len(), 3);
    assert_eq!(
        search(&t.lb, "person:cy"),
        ["d.png"],
        "hidden people stay searchable"
    );

    let e = id_of(&t.lb, "e.png");
    assert!(
        faces
            .add_manual(
                e,
                NormRect {
                    x: 0.2,
                    y: 0.2,
                    w: 0.0,
                    h: 0.3
                },
                None
            )
            .is_err()
    );
    let manual = faces
        .add_manual(
            e,
            NormRect {
                x: 0.2,
                y: 0.2,
                w: 0.3,
                h: 0.4,
            },
            Some("Dee"),
        )
        .unwrap();
    assert_eq!(search(&t.lb, "person:dee"), ["e.png"]);
    let crop = faces.crop(manual, 64).unwrap();
    assert_eq!(crop.dimensions(), (64, 64));
    let crop = faces.crop(cy.id, 32).unwrap();
    assert_eq!(crop.dimensions(), (32, 32));
    assert_eq!(
        crop.get_pixel(16, 16).0,
        lb_faces::MOCK_PALETTE[CY],
        "the crop is centered on the face"
    );

    let removed = faces.reset_detection().unwrap();
    assert_eq!(removed, 5);
    assert_eq!(
        faces.faces_in_photo(e).unwrap().len(),
        1,
        "manual faces survive"
    );
    let again = faces.process_pending(None).unwrap();
    assert_eq!(again.photos, 6);
    assert_eq!(
        search(&t.lb, "person:cy"),
        Vec::<String>::new(),
        "named detections were reset"
    );
}

#[test]
fn faces_are_detected_after_a_scan_when_enabled() {
    let t = setup();
    assert!(t.lb.faces().spawn_pending().is_none(), "off by default");
    t.lb.settings().update(|s| s.faces.enabled = true).unwrap();
    assert!(t.lb.faces().enabled());
    write(t.dir.path(), "h.png", &[(BOB, 30, 30, 60)]);
    t.lb.library().rescan_all().unwrap();
    assert_eq!(
        t.lb.catalog_handle()
            .photos_without_face_scan()
            .unwrap()
            .len(),
        0,
        "the inline queue ran the job right after the scan"
    );
    assert_eq!(search(&t.lb, "has:faces").len(), 5);
}

#[test]
fn without_models_faces_are_unavailable() {
    let home = tempfile::tempdir().unwrap();
    let lb = Lightbox::open_with(
        home.path(),
        JobQueue::inline(),
        LightboxOptions {
            spawn_event_ticker: false,
            ..LightboxOptions::default()
        },
    )
    .unwrap();
    if cfg!(feature = "onnx") {
        return;
    }
    assert!(!lb.faces().available());
    assert!(lb.faces().process_pending(None).is_err());
    assert!(lb.faces().spawn_pending().is_none());
}

#[test]
fn sidebar_lists_unnamed_and_named_people() {
    use lb_app::vm::{SidebarKey, SidebarVm};
    let t = setup();
    let faces = t.lb.faces();
    let mut sidebar = SidebarVm::new(&t.lb).unwrap();
    assert!(
        sidebar.sections().iter().all(|s| s.title != "People"),
        "no People section before any face exists"
    );
    faces.process_pending(None).unwrap();
    sidebar.refresh().unwrap();
    let people = sidebar
        .sections()
        .iter()
        .find(|s| s.title == "People")
        .unwrap();
    assert_eq!(people.items.len(), 1);
    assert_eq!(people.items[0].key, SidebarKey::Unnamed);
    assert_eq!(people.items[0].count, 4);

    let a = faces.faces_in_photo(id_of(&t.lb, "a.png")).unwrap();
    let ana = faces
        .name(PeopleKey::Cluster(a[0].cluster_id.unwrap()), "Ana")
        .unwrap();
    sidebar.refresh().unwrap();
    let people = sidebar
        .sections()
        .iter()
        .find(|s| s.title == "People")
        .unwrap();
    assert_eq!(
        people.items[0].key,
        SidebarKey::Unnamed,
        "Unnamed comes first"
    );
    assert_eq!(people.items[1].label, "Ana");
    assert_eq!(people.items[1].count, 2);
    assert_eq!(
        sidebar.select(SidebarKey::Person(ana.id)),
        Some(ViewSource::Person(ana.id))
    );
    assert_eq!(
        SidebarVm::key_for(&ViewSource::Person(ana.id)),
        Some(SidebarKey::Person(ana.id))
    );
    let Some(ViewSource::Ids(ids)) = sidebar.select(SidebarKey::Unnamed) else {
        panic!("the Unnamed row lists photos");
    };
    assert_eq!(names(&t.lb, &ids), ["a.png", "c.png", "d.png"]);
}
