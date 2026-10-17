use crate::repo::test_util::*;
use crate::{CatalogError, NewFolder};
use lb_core::{PhotoStatus, RootMode};
use std::path::Path;

#[test]
fn indexed_files_under_matches_subtree_only() {
    let (_dir, catalog) = open();
    let root = catalog
        .add_root(Path::new("/photos"), RootMode::Watch, now())
        .unwrap();
    let mut folders = Vec::new();
    for p in [
        "/photos",
        "/photos/a",
        "/photos/a/deep",
        "/photos/ab",
        "/photos/100%_a",
    ] {
        folders.push(
            catalog
                .ensure_folder(NewFolder {
                    root_id: root.id,
                    path: p.into(),
                    parent_id: None,
                })
                .unwrap(),
        );
    }
    for f in &folders {
        catalog
            .upsert_photos(vec![upsert(f, "x.jpg")], now())
            .unwrap();
    }
    let under = |dir: &str| -> Vec<String> {
        catalog
            .indexed_files_under(Path::new(dir))
            .unwrap()
            .into_iter()
            .map(|f| f.path.display().to_string())
            .collect()
    };
    assert_eq!(
        under("/photos/a"),
        ["/photos/a/x.jpg", "/photos/a/deep/x.jpg"]
    );
    assert_eq!(under("/photos/100%_a"), ["/photos/100%_a/x.jpg"]);
    assert_eq!(under("/photos").len(), 5);
    assert!(under("/elsewhere").is_empty());
}

#[test]
fn status_update_and_tree_removal() {
    let (_dir, catalog) = open();
    let root = catalog
        .add_root(Path::new("/photos"), RootMode::Watch, now())
        .unwrap();
    let top = catalog
        .ensure_folder(NewFolder {
            root_id: root.id,
            path: "/photos".into(),
            parent_id: None,
        })
        .unwrap();
    let sub = catalog
        .ensure_folder(NewFolder {
            root_id: root.id,
            path: "/photos/sub".into(),
            parent_id: Some(top.id),
        })
        .unwrap();
    let keep = catalog
        .upsert_photos(vec![upsert(&top, "keep.jpg")], now())
        .unwrap()[0]
        .id;
    let gone = catalog
        .upsert_photos(
            vec![upsert(&sub, "gone.jpg"), upsert(&sub, "gone2.jpg")],
            now(),
        )
        .unwrap();

    assert_eq!(
        catalog
            .set_photos_status(&[keep], PhotoStatus::Missing, Some("moved"))
            .unwrap(),
        1
    );
    let p = catalog.get_photo(keep).unwrap().unwrap();
    assert_eq!(
        (p.status, p.error.as_deref()),
        (PhotoStatus::Missing, Some("moved"))
    );

    let removed = catalog
        .remove_folder_tree(Path::new("/photos/sub"))
        .unwrap();
    assert_eq!(removed, gone.iter().map(|o| o.id).collect::<Vec<_>>());
    assert!(
        catalog
            .folder_by_path(Path::new("/photos/sub"))
            .unwrap()
            .is_none()
    );
    assert!(catalog.get_photo(gone[0].id).unwrap().is_none());
    assert!(catalog.get_photo(keep).unwrap().is_some(), "siblings stay");
}

#[test]
fn relocate_keeps_id_and_user_fields() {
    let (_dir, catalog) = open();
    let (root, folder) = root_and_folder(&catalog);
    let other = catalog
        .ensure_folder(NewFolder {
            root_id: root.id,
            path: "/photos/other".into(),
            parent_id: Some(folder.id),
        })
        .unwrap();
    let id = catalog
        .upsert_photos(vec![upsert(&folder, "a.jpg")], now())
        .unwrap()[0]
        .id;
    catalog.set_caption(&[id], Some("kept".into())).unwrap();
    catalog
        .relocate_photo(id, other.id, "b.jpg", now())
        .unwrap();
    let p = catalog.get_photo(id).unwrap().unwrap();
    assert_eq!(p.path, Path::new("/photos/other/b.jpg"));
    assert_eq!(p.caption.as_deref(), Some("kept"));
    assert!(matches!(
        catalog.relocate_photo(lb_core::PhotoId::new(9999), other.id, "c.jpg", now()),
        Err(CatalogError::NotFound(_))
    ));
}

#[test]
fn sort_date_is_earliest_taken_at() {
    let (_dir, catalog) = open();
    let (root, folder) = root_and_folder(&catalog);
    let mut a = upsert(&folder, "a.jpg");
    a.taken_at = now().checked_sub_signed(chrono::Duration::days(3));
    let mut b = upsert(&folder, "b.jpg");
    b.taken_at = Some(now());
    catalog.upsert_photos(vec![a.clone(), b], now()).unwrap();
    catalog.refresh_folder_sort_dates(root.id).unwrap();
    let f = catalog.get_folder(folder.id).unwrap().unwrap();
    assert_eq!(f.sort_date, a.taken_at);
}
