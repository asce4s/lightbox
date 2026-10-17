use crate::CatalogError;
use crate::repo::test_util::*;
use lb_core::PhotoId;

fn ids(
    n: usize,
) -> (
    tempfile::TempDir,
    crate::Catalog,
    Vec<PhotoId>,
    lb_core::Album,
) {
    let (dir, catalog) = open();
    let (_, folder) = root_and_folder(&catalog);
    let photos = insert_photos(&catalog, &folder, n);
    let album = catalog.create_album("Summer", Some("trip"), now()).unwrap();
    (dir, catalog, photos, album)
}

#[test]
fn album_crud() {
    let (_dir, catalog, photos, album) = ids(4);
    assert_eq!(
        (
            album.name.as_str(),
            album.description.as_deref(),
            album.created_at
        ),
        ("Summer", Some("trip"), now())
    );
    let b = catalog.create_album("alpha", None, now()).unwrap();
    assert!(b.sort_order > album.sort_order);
    let names: Vec<_> = catalog
        .list_albums()
        .unwrap()
        .into_iter()
        .map(|a| a.name)
        .collect();
    assert_eq!(names, ["Summer", "alpha"], "sort_order first, then name");

    assert!(catalog.rename_album(album.id, "Winter").unwrap());
    assert_eq!(catalog.get_album(album.id).unwrap().unwrap().name, "Winter");
    assert!(catalog.set_album_cover(album.id, Some(photos[2])).unwrap());
    assert_eq!(
        catalog.get_album(album.id).unwrap().unwrap().cover_photo_id,
        Some(photos[2])
    );
    assert!(catalog.set_album_cover(album.id, None).unwrap());

    assert_eq!(catalog.add_to_album(album.id, &photos).unwrap(), 4);
    assert_eq!(
        catalog
            .add_to_album(album.id, &[photos[1], photos[3]])
            .unwrap(),
        0,
        "duplicates are skipped"
    );
    assert_eq!(catalog.album_photo_ids(album.id).unwrap(), photos);
    assert_eq!(
        catalog
            .remove_from_album(album.id, &[photos[0], photos[3], PhotoId::new(777)])
            .unwrap(),
        2
    );
    assert_eq!(
        catalog.album_photo_ids(album.id).unwrap(),
        [photos[1], photos[2]]
    );

    let copy = catalog
        .duplicate_album(album.id, "Winter (copy)", now())
        .unwrap();
    assert_ne!(copy.id, album.id);
    assert_eq!(
        catalog.album_items(copy.id).unwrap(),
        catalog.album_items(album.id).unwrap()
    );
    assert!(catalog.delete_album(album.id).unwrap());
    assert!(!catalog.delete_album(album.id).unwrap());
    assert_eq!(
        catalog.album_photo_ids(copy.id).unwrap().len(),
        2,
        "copy is independent"
    );
}

#[test]
fn album_delete_keeps_photos_and_cover_falls_back_to_none() {
    let (_dir, catalog, photos, album) = ids(2);
    catalog.add_to_album(album.id, &photos).unwrap();
    catalog.set_album_cover(album.id, Some(photos[0])).unwrap();
    let other = catalog.create_album("other", None, now()).unwrap();
    catalog.set_album_cover(other.id, Some(photos[1])).unwrap();

    catalog.delete_album(album.id).unwrap();
    assert_eq!(
        catalog.get_many(&photos).unwrap().len(),
        2,
        "photos untouched"
    );

    catalog
        .write(move |c| Ok(c.execute("DELETE FROM photos WHERE id = ?1", [photos[1].get()])?))
        .unwrap();
    assert_eq!(
        catalog.get_album(other.id).unwrap().unwrap().cover_photo_id,
        None,
        "ON DELETE SET NULL"
    );
}

#[test]
fn album_missing_album_and_items_are_not_found() {
    let (_dir, catalog, photos, album) = ids(2);
    let ghost = lb_core::AlbumId::new(404);
    assert!(matches!(
        catalog.add_to_album(ghost, &photos).unwrap_err(),
        CatalogError::NotFound("album")
    ));
    assert!(matches!(
        catalog.duplicate_album(ghost, "x", now()).unwrap_err(),
        CatalogError::NotFound("album")
    ));
    catalog.add_to_album(album.id, &photos[..1]).unwrap();
    assert!(matches!(
        catalog
            .move_in_album(album.id, photos[1], None)
            .unwrap_err(),
        CatalogError::NotFound("album item")
    ));
    assert!(matches!(
        catalog
            .move_in_album(album.id, photos[0], Some(photos[1]))
            .unwrap_err(),
        CatalogError::NotFound("album item")
    ));
}

#[test]
fn album_reorder_fractional_positions() {
    let (_dir, catalog, p, album) = ids(5);
    catalog.add_to_album(album.id, &p).unwrap();
    let before = catalog.album_items(album.id).unwrap();
    assert_eq!(
        before.iter().map(|(_, pos)| *pos).collect::<Vec<_>>(),
        [1.0, 2.0, 3.0, 4.0, 5.0]
    );

    assert!(
        !catalog.move_in_album(album.id, p[4], Some(p[1])).unwrap(),
        "no renormalization needed"
    );
    assert_eq!(
        catalog.album_photo_ids(album.id).unwrap(),
        [p[0], p[4], p[1], p[2], p[3]]
    );
    let after = catalog.album_items(album.id).unwrap();
    let pos = |id: PhotoId| after.iter().find(|(i, _)| *i == id).unwrap().1;
    assert_eq!(pos(p[4]), 1.5, "midpoint between p0 (1.0) and p1 (2.0)");
    for untouched in [p[0], p[1], p[2], p[3]] {
        assert_eq!(
            pos(untouched),
            before.iter().find(|(i, _)| *i == untouched).unwrap().1,
            "only the moved row changes"
        );
    }

    catalog.move_in_album(album.id, p[3], Some(p[0])).unwrap();
    assert_eq!(
        catalog.album_photo_ids(album.id).unwrap()[0],
        p[3],
        "moving to the front uses first position - 1"
    );
    assert_eq!(catalog.album_items(album.id).unwrap()[0].1, 0.0);

    catalog.move_in_album(album.id, p[3], None).unwrap();
    assert_eq!(
        catalog.album_photo_ids(album.id).unwrap().last(),
        Some(&p[3]),
        "None moves to the end"
    );
    catalog.move_in_album(album.id, p[3], None).unwrap();
    assert_eq!(catalog.album_photo_ids(album.id).unwrap().len(), 5);
}

#[test]
fn album_renormalize_positions() {
    let (_dir, catalog, p, album) = ids(3);
    catalog.add_to_album(album.id, &p).unwrap();

    let mut renormalized_at = None;
    for step in 0..60 {
        // Squeeze the last photo in front of the second one; the gap before it halves every time.
        let order = catalog.album_photo_ids(album.id).unwrap();
        let last = *order.last().unwrap();
        let second = order[1];
        if catalog.move_in_album(album.id, last, Some(second)).unwrap() {
            renormalized_at = Some(step);
            break;
        }
    }
    let step = renormalized_at.expect("gaps below 1e-9 trigger renormalization");
    assert!(
        step > 20,
        "renormalization is lazy, not on every move (step {step})"
    );

    let items = catalog.album_items(album.id).unwrap();
    assert_eq!(
        items.iter().map(|(_, pos)| *pos).collect::<Vec<_>>(),
        [1.0, 2.0, 3.0],
        "positions reset to 1..n"
    );
    assert!(items.windows(2).all(|w| w[1].1 - w[0].1 >= 1.0));

    let order_before = catalog.album_photo_ids(album.id).unwrap();
    catalog.renormalize_album(album.id).unwrap();
    assert_eq!(
        catalog.album_photo_ids(album.id).unwrap(),
        order_before,
        "renormalizing never reorders"
    );
}
