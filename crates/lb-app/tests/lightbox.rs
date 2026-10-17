//! M6.1: facade, events and history.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use lb_app::{AppEvent, AppPaths, HistoryEntry, Lightbox, LightboxOptions};
use lb_core::{AlbumId, PhotoId};
use lb_jobs::JobQueue;
use lb_testkit::{FakeClock, TestHome};
use std::sync::Arc;

#[test]
fn open_creates_home_layout() {
    let home = tempfile::tempdir().unwrap();
    let lb = Lightbox::open(home.path(), JobQueue::inline()).unwrap();
    let p = home.path();
    assert!(p.join("data/catalog.sqlite3").is_file());
    for dir in [
        "data",
        "data/models",
        "data/logs",
        "data/backups",
        "cache",
        "cache/thumbs",
        "cache/tiles",
    ] {
        assert!(p.join(dir).is_dir(), "{dir} missing");
    }
    assert_eq!(lb.paths(), &AppPaths::under(p));
    drop(lb);
    Lightbox::open(home.path(), JobQueue::inline()).expect("reopening an existing library works");
}

#[test]
fn paths_resolve_honours_lightbox_home() {
    let home = TestHome::new();
    let paths = AppPaths::resolve().unwrap();
    assert_eq!(paths.data, home.data_dir());
    assert_eq!(paths.cache, home.cache_dir());
    assert_eq!(paths.catalog(), home.data_dir().join("catalog.sqlite3"));
    assert_eq!(paths.thumbs(), home.cache_dir().join("thumbs"));
}

#[test]
fn events_coalesced_within_16ms() {
    let home = tempfile::tempdir().unwrap();
    let clock = Arc::new(FakeClock::new());
    let lb = Lightbox::open_with(
        home.path(),
        JobQueue::inline(),
        LightboxOptions {
            clock: clock.clone(),
            spawn_event_ticker: false,
            ..LightboxOptions::default()
        },
    )
    .unwrap();
    let rx = lb.events();
    for i in 0..50 {
        lb.bus().emit(AppEvent::PhotosChanged {
            ids: vec![PhotoId::new(i), PhotoId::new(i + 1)],
        });
        clock.advance(std::time::Duration::from_micros(200));
        lb.bus().tick();
    }
    assert_eq!(rx.try_iter().count(), 0, "storm is held back inside 16 ms");
    clock.advance(std::time::Duration::from_millis(16));
    lb.bus().tick();
    let events: Vec<_> = rx.try_iter().collect();
    assert_eq!(events.len(), 1);
    let AppEvent::PhotosChanged { ids } = &events[0] else {
        panic!("{events:?}");
    };
    assert_eq!(ids.len(), 51);
}

#[test]
fn library_scan_emits_events_and_indexes_photos() {
    let t = common::library(3);
    assert_eq!(t.ids.len(), 3);
    let photos = t.lb.photos().get_many(&t.ids).unwrap();
    assert_eq!(
        photos
            .iter()
            .map(|p| p.path.file_name().unwrap().to_str().unwrap())
            .collect::<Vec<_>>(),
        ["p00.jpg", "p01.jpg", "p02.jpg"]
    );
    let rx = t.lb.events();
    let root = t.lb.library().roots().unwrap()[0].id;
    let summary = t.lb.library().rescan(root).unwrap();
    assert_eq!((summary.added, summary.unchanged), (0, 3));
    let events: Vec<_> = rx.try_iter().collect();
    assert!(
        events
            .iter()
            .any(|e| matches!(e, AppEvent::ScanFinished { added: 0, .. }))
    );
    assert!(events.contains(&AppEvent::FoldersChanged));
}

fn star_entry(ids: Vec<PhotoId>, was: Vec<bool>) -> HistoryEntry {
    let ids2 = ids.clone();
    HistoryEntry::new(
        "Star",
        move |c| {
            for (id, s) in ids.iter().zip(&was) {
                c.set_starred(&[*id], *s)?;
            }
            Ok(vec![AppEvent::PhotosChanged { ids: ids.clone() }])
        },
        move |c| {
            c.set_starred(&ids2, true)?;
            Ok(vec![AppEvent::PhotosChanged { ids: ids2.clone() }])
        },
    )
}

#[test]
fn history_undo_star_tag_album() {
    let t = common::library(3);
    let lb = &t.lb;
    let history = lb.history();
    let (a, b, c) = (t.ids[0], t.ids[1], t.ids[2]);
    let starred = |id: PhotoId| lb.photos().get(id).unwrap().unwrap().starred;
    let tags = |id: PhotoId| lb.catalog_handle().tags_for_photo(id).unwrap().len();
    let album_ids = |album: AlbumId| lb.catalog_handle().album_photo_ids(album).unwrap();

    assert!(!history.can_undo() && history.undo().unwrap().is_none());

    history
        .execute(star_entry(vec![a, b], vec![false, false]))
        .unwrap();

    let tag_ids = vec![b, c];
    let (tu, tr) = (tag_ids.clone(), tag_ids.clone());
    history
        .execute(HistoryEntry::new(
            "Tag 2 photos",
            move |cat| {
                cat.remove_tags(&tu, &["beach".to_owned()])?;
                Ok(vec![AppEvent::TagsChanged])
            },
            move |cat| {
                cat.add_tags(&tr, &["beach".to_owned()])?;
                Ok(vec![AppEvent::TagsChanged])
            },
        ))
        .unwrap();

    let album = lb
        .catalog_handle()
        .create_album("Trip", None, lb.clock().now())
        .unwrap();
    let aid = album.id;
    history
        .execute(HistoryEntry::new(
            "Add to album",
            move |cat| {
                cat.remove_from_album(aid, &[a, c])?;
                Ok(vec![AppEvent::AlbumsChanged])
            },
            move |cat| {
                cat.add_to_album(aid, &[a, c])?;
                Ok(vec![AppEvent::AlbumsChanged])
            },
        ))
        .unwrap();

    assert!(starred(a) && starred(b) && !starred(c));
    assert_eq!((tags(a), tags(b), tags(c)), (0, 1, 1));
    assert_eq!(album_ids(aid), vec![a, c]);
    assert_eq!(history.undo_label().as_deref(), Some("Add to album"));

    let rx = lb.events();
    assert_eq!(history.undo().unwrap().as_deref(), Some("Add to album"));
    assert!(album_ids(aid).is_empty());
    assert_eq!(rx.try_recv(), Ok(AppEvent::AlbumsChanged));
    assert_eq!(history.undo().unwrap().as_deref(), Some("Tag 2 photos"));
    assert_eq!((tags(a), tags(b), tags(c)), (0, 0, 0));
    assert_eq!(history.undo().unwrap().as_deref(), Some("Star"));
    assert!(!starred(a) && !starred(b));
    assert!(!history.can_undo());
    assert_eq!(history.redo_label().as_deref(), Some("Star"));

    assert_eq!(history.redo().unwrap().as_deref(), Some("Star"));
    assert_eq!(history.redo().unwrap().as_deref(), Some("Tag 2 photos"));
    assert!(starred(a) && starred(b));
    assert_eq!((tags(a), tags(b), tags(c)), (0, 1, 1));
    assert!(history.can_redo());

    history.execute(star_entry(vec![c], vec![false])).unwrap();
    assert!(!history.can_redo(), "a new action clears the redo stack");
    assert!(starred(c));
}

#[test]
fn failed_history_action_is_not_recorded_and_stays_undoable() {
    let t = common::library(1);
    let history = t.lb.history();
    let failing = HistoryEntry::new(
        "Bad",
        |_| Ok(vec![]),
        |_| Err(lb_app::AppError::Invalid("nope".into())),
    );
    assert!(history.execute(failing).is_err());
    assert!(!history.can_undo());

    history
        .execute(HistoryEntry::new(
            "Flaky undo",
            |_| Err(lb_app::AppError::Invalid("cannot undo".into())),
            |_| Ok(vec![]),
        ))
        .unwrap();
    assert!(history.undo().is_err());
    assert!(
        history.can_undo() && !history.can_redo(),
        "entry stays where it was"
    );
}
