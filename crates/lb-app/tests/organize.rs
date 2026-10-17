//! M6.3: stars, ratings, captions, hidden, tags and albums through the service layer.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use lb_app::{AppError, AppEvent, ViewSource};
use lb_catalog::{PhotoQuery, QuerySource};
use lb_core::PhotoId;

fn starred(t: &common::TestLib) -> Vec<bool> {
    t.lb.photos()
        .get_many(&t.ids)
        .unwrap()
        .iter()
        .map(|p| p.starred)
        .collect()
}

#[test]
fn star_toggle_batch() {
    let t = common::library(4);
    let photos = t.lb.photos();
    let rx = t.lb.events();

    assert!(photos.toggle_star(&t.ids[..2]).unwrap());
    assert_eq!(starred(&t), [true, true, false, false]);

    assert!(
        photos.toggle_star(&t.ids).unwrap(),
        "a mixed selection becomes all starred"
    );
    assert_eq!(starred(&t), [true; 4]);
    assert!(
        !photos.toggle_star(&t.ids).unwrap(),
        "all starred toggles off"
    );
    assert_eq!(starred(&t), [false; 4]);

    assert_eq!(
        photos.set_starred(&t.ids, false).unwrap(),
        0,
        "no-op changes nothing"
    );
    assert_eq!(
        t.lb.history().undo_label().as_deref(),
        Some("Unstar 4 photos")
    );

    t.lb.history().undo().unwrap();
    assert_eq!(starred(&t), [true; 4], "the whole batch is one undo step");
    t.lb.history().undo().unwrap();
    assert_eq!(
        starred(&t),
        [true, true, false, false],
        "undo restores each photo's own state"
    );
    t.lb.history().undo().unwrap();
    assert_eq!(starred(&t), [false; 4]);

    t.lb.bus().flush();
    assert!(
        rx.try_iter()
            .any(|e| matches!(e, AppEvent::PhotosChanged { ids } if ids.len() == 4))
    );
}

#[test]
fn rating_bounds_enforced() {
    let t = common::library(2);
    let photos = t.lb.photos();
    for r in 0..=5u8 {
        photos.set_rating(&t.ids, r).unwrap();
        assert!(
            photos
                .get_many(&t.ids)
                .unwrap()
                .iter()
                .all(|p| p.rating == r)
        );
    }
    let err = photos.set_rating(&t.ids, 6).unwrap_err();
    assert!(matches!(err, AppError::Invalid(_)), "{err}");
    assert!(
        photos
            .get_many(&t.ids)
            .unwrap()
            .iter()
            .all(|p| p.rating == 5),
        "unchanged"
    );
    photos.set_rating(&t.ids[..1], 2).unwrap();
    t.lb.history().undo().unwrap();
    assert!(photos.get(t.ids[0]).unwrap().unwrap().rating == 5);
}

fn fts_hits(t: &common::TestLib, expr: &str) -> Vec<PhotoId> {
    let mut q = PhotoQuery::new(QuerySource::All);
    q.fts = Some(expr.into());
    t.lb.catalog_handle()
        .query_photos(&q)
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect()
}

#[test]
fn caption_saved_and_indexed_in_fts() {
    let t = common::library(3);
    let photos = t.lb.photos();
    photos
        .set_caption(&t.ids[..2], Some("  Sunset at the beach \n"))
        .unwrap();
    assert_eq!(
        photos.get(t.ids[0]).unwrap().unwrap().caption.as_deref(),
        Some("Sunset at the beach"),
        "surrounding whitespace is trimmed"
    );
    assert_eq!(fts_hits(&t, "sunse*"), t.ids[..2].to_vec());
    assert_eq!(fts_hits(&t, "caption:beach"), t.ids[..2].to_vec());

    photos.set_caption(&t.ids[..1], Some("   ")).unwrap();
    assert_eq!(
        photos.get(t.ids[0]).unwrap().unwrap().caption,
        None,
        "blank clears"
    );
    assert_eq!(fts_hits(&t, "sunset"), vec![t.ids[1]]);

    let long = "x".repeat(2001);
    assert!(matches!(
        photos.set_caption(&t.ids, Some(&long)),
        Err(AppError::Invalid(_))
    ));
    photos.set_caption(&t.ids, Some(&"y".repeat(2000))).unwrap();

    t.lb.history().undo().unwrap();
    assert_eq!(
        fts_hits(&t, "sunset"),
        vec![t.ids[1]],
        "undo restores captions and the index"
    );
}

#[test]
fn hidden_excluded_by_default() {
    let t = common::library(3);
    let photos = t.lb.photos();
    photos.set_hidden(&t.ids[1..2], true).unwrap();
    let default = photos.list_ids(&ViewSource::AllPhotos, false).unwrap();
    assert_eq!(default, vec![t.ids[0], t.ids[2]]);
    let all = photos.list_ids(&ViewSource::AllPhotos, true).unwrap();
    assert_eq!(all.len(), 3);
    photos.set_starred(&t.ids, true).unwrap();
    assert_eq!(
        photos.list_ids(&ViewSource::Starred, false).unwrap(),
        default,
        "hidden photos stay out of every source"
    );
    t.lb.history().undo().unwrap();
    t.lb.history().undo().unwrap();
    assert_eq!(
        photos
            .list_ids(&ViewSource::AllPhotos, false)
            .unwrap()
            .len(),
        3
    );
}

fn tag_names(t: &common::TestLib, id: PhotoId) -> Vec<String> {
    t.lb.tags()
        .for_photo(id)
        .unwrap()
        .into_iter()
        .map(|t| t.name)
        .collect()
}

#[test]
fn tag_normalization() {
    let t = common::library(2);
    let tags = t.lb.tags();
    let names = |v: &[&str]| v.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
    tags.add(&t.ids, &names(&["  Beach   Trip ", "beach trip", "Sun"]))
        .unwrap();
    assert_eq!(tag_names(&t, t.ids[0]), ["Beach Trip", "Sun"]);
    for bad in ["", "   ", "a,b"] {
        let err = tags.add(&t.ids, &names(&[bad])).unwrap_err();
        assert!(matches!(err, AppError::Invalid(_)), "{bad:?}: {err}");
    }
    assert!(matches!(
        tags.add(&t.ids, &names(&[&"x".repeat(65)])),
        Err(AppError::Invalid(_))
    ));
    assert_eq!(tags.list().unwrap().len(), 2);
}

#[test]
fn tag_add_remove_batch_is_undoable() {
    let t = common::library(3);
    let tags = t.lb.tags();
    let rx = t.lb.events();
    let beach = vec!["beach".to_owned()];
    assert_eq!(tags.add(&t.ids[..1], &beach).unwrap(), 1);
    assert_eq!(
        tags.add(&t.ids, &beach).unwrap(),
        2,
        "only photos lacking the tag are counted"
    );
    assert_eq!(tags.remove(&t.ids[1..], &beach).unwrap(), 2);
    assert_eq!(tag_names(&t, t.ids[0]), ["beach"]);
    assert!(tag_names(&t, t.ids[1]).is_empty());

    t.lb.history().undo().unwrap();
    assert_eq!(tag_names(&t, t.ids[1]), ["beach"]);
    t.lb.history().undo().unwrap();
    assert!(tag_names(&t, t.ids[1]).is_empty() && tag_names(&t, t.ids[2]).is_empty());
    assert_eq!(
        tag_names(&t, t.ids[0]),
        ["beach"],
        "the photo tagged earlier keeps its tag"
    );
    t.lb.history().redo().unwrap();
    assert_eq!(tag_names(&t, t.ids[2]), ["beach"]);
    assert_eq!(tags.remove(&t.ids, &["unknown".to_owned()]).unwrap(), 0);

    t.lb.bus().flush();
    assert!(rx.try_iter().any(|e| e == AppEvent::TagsChanged));
}

#[test]
fn tag_rename_merges() {
    let t = common::library(3);
    let tags = t.lb.tags();
    tags.add(&t.ids[..2], &["holiday".to_owned()]).unwrap();
    tags.add(&t.ids[1..], &["vacation".to_owned()]).unwrap();
    let holiday = tags
        .list()
        .unwrap()
        .into_iter()
        .find(|u| u.tag.name == "holiday")
        .unwrap();
    let kept = tags.rename(holiday.tag.id, "Vacation").unwrap();
    assert_eq!(
        kept.name, "vacation",
        "merging keeps the surviving tag's spelling"
    );
    let usage = tags.list().unwrap();
    assert_eq!(usage.len(), 1);
    assert_eq!(usage[0].count, 3);
    for id in &t.ids {
        assert_eq!(tag_names(&t, *id), ["vacation"]);
    }
    let renamed = tags.rename(kept.id, "Trip").unwrap();
    assert_eq!(renamed.name, "Trip");
    assert!(matches!(
        tags.rename(kept.id, "a,b"),
        Err(AppError::Invalid(_))
    ));
    tags.delete(renamed.id).unwrap();
    assert!(tags.list().unwrap().is_empty());
    assert!(matches!(
        tags.delete(renamed.id),
        Err(AppError::NotFound("tag"))
    ));
}

#[test]
fn tag_autocomplete_prefix() {
    let t = common::library(12);
    let tags = t.lb.tags();
    for (i, name) in [
        "beach", "beam", "bear", "bee", "bell", "berry", "best", "bet", "bib", "bike", "bird",
        "other",
    ]
    .iter()
    .enumerate()
    {
        tags.add(&t.ids[..=i], &[(*name).to_owned()]).unwrap();
    }
    let top = tags.autocomplete("B").unwrap();
    assert_eq!(top.len(), 10, "at most ten suggestions");
    assert_eq!(top[0].tag.name, "bird", "most used first");
    assert!(top.iter().all(|u| u.tag.name.starts_with('b')));
    assert!(top.windows(2).all(|w| w[0].count >= w[1].count));
    let be = tags.autocomplete("bea").unwrap();
    assert_eq!(
        be.iter().map(|u| u.tag.name.as_str()).collect::<Vec<_>>(),
        ["bear", "beam", "beach"]
    );
    assert!(tags.autocomplete("zzz").unwrap().is_empty());
    assert_eq!(tags.autocomplete("").unwrap().len(), 10);
}

#[test]
fn album_crud() {
    let t = common::library(4);
    let albums = t.lb.albums();
    let rx = t.lb.events();
    let trip = albums.create("  Summer   Trip ", Some("fun")).unwrap();
    assert_eq!(trip.name, "Summer Trip");
    let other = albums.create("Family", None).unwrap();
    assert!(matches!(
        albums.create("  ", None),
        Err(AppError::Invalid(_))
    ));
    assert_eq!(
        albums
            .list()
            .unwrap()
            .iter()
            .map(|a| a.name.as_str())
            .collect::<Vec<_>>(),
        ["Summer Trip", "Family"]
    );

    assert_eq!(
        albums
            .add_photos(trip.id, &[t.ids[2], t.ids[0], t.ids[2], t.ids[1]])
            .unwrap(),
        3
    );
    assert_eq!(
        albums.add_photos(trip.id, &[t.ids[0]]).unwrap(),
        0,
        "already members"
    );
    assert_eq!(
        albums.photo_ids(trip.id).unwrap(),
        vec![t.ids[2], t.ids[0], t.ids[1]]
    );
    assert_eq!(
        t.lb.photos()
            .list_ids(&ViewSource::Album(trip.id), false)
            .unwrap(),
        vec![t.ids[2], t.ids[0], t.ids[1]],
        "album views keep album order"
    );

    albums
        .move_photo(trip.id, t.ids[1], Some(t.ids[2]))
        .unwrap();
    assert_eq!(
        albums.photo_ids(trip.id).unwrap(),
        vec![t.ids[1], t.ids[2], t.ids[0]]
    );
    t.lb.history().undo().unwrap();
    assert_eq!(
        albums.photo_ids(trip.id).unwrap(),
        vec![t.ids[2], t.ids[0], t.ids[1]]
    );

    assert_eq!(albums.remove_photos(trip.id, &[t.ids[0]]).unwrap(), 1);
    assert_eq!(albums.photo_ids(trip.id).unwrap(), vec![t.ids[2], t.ids[1]]);
    t.lb.history().undo().unwrap();
    assert_eq!(
        albums.photo_ids(trip.id).unwrap(),
        vec![t.ids[2], t.ids[0], t.ids[1]],
        "undoing a removal restores the original position"
    );

    albums.rename(trip.id, "Holiday").unwrap();
    albums.set_cover(trip.id, Some(t.ids[1])).unwrap();
    assert_eq!(
        albums.get(trip.id).unwrap().unwrap().cover_photo_id,
        Some(t.ids[1])
    );
    let copy = albums.duplicate(trip.id, "Holiday copy").unwrap();
    assert_eq!(
        albums.photo_ids(copy.id).unwrap(),
        albums.photo_ids(trip.id).unwrap()
    );
    assert!(matches!(
        albums.rename(AlbumIdMissing::id(), "x"),
        Err(AppError::NotFound("album"))
    ));
    assert!(matches!(
        albums.add_photos(AlbumIdMissing::id(), &t.ids),
        Err(AppError::NotFound("album"))
    ));
    let _ = other;

    t.lb.bus().flush();
    assert!(rx.try_iter().any(|e| e == AppEvent::AlbumsChanged));
}

struct AlbumIdMissing;
impl AlbumIdMissing {
    fn id() -> lb_core::AlbumId {
        lb_core::AlbumId::new(424_242)
    }
}

#[test]
fn album_delete_keeps_photos() {
    let t = common::library(3);
    let albums = t.lb.albums();
    let album = albums.create("Temp", None).unwrap();
    albums.add_photos(album.id, &t.ids).unwrap();
    albums.delete(album.id).unwrap();
    assert!(albums.get(album.id).unwrap().is_none());
    assert_eq!(
        t.lb.photos().get_many(&t.ids).unwrap().len(),
        3,
        "photos untouched"
    );
    assert!(matches!(
        albums.delete(album.id),
        Err(AppError::NotFound("album"))
    ));
    assert!(
        t.lb.photos()
            .list_ids(&ViewSource::Album(album.id), false)
            .unwrap()
            .is_empty()
    );
}
