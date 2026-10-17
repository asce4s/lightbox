use crate::repo::test_util::*;
use crate::{CatalogError, NewFolder};
use lb_core::RootMode;
use std::path::Path;

#[test]
fn roots_add_update_list_remove() {
    let (_dir, catalog) = open();
    let a = catalog
        .add_root(Path::new("/b/photos"), RootMode::Watch, now())
        .unwrap();
    let b = catalog
        .add_root(Path::new("/a/other"), RootMode::Once, now())
        .unwrap();
    assert_ne!(a.id, b.id);
    assert_eq!((a.mode, a.added_at), (RootMode::Watch, now()));

    let later = now() + chrono::Duration::hours(1);
    let again = catalog
        .add_root(Path::new("/b/photos"), RootMode::Exclude, later)
        .unwrap();
    assert_eq!(again.id, a.id, "same path updates instead of duplicating");
    assert_eq!(again.mode, RootMode::Exclude);
    assert_eq!(again.added_at, now(), "added_at is kept");

    let listed: Vec<_> = catalog
        .list_roots()
        .unwrap()
        .into_iter()
        .map(|r| r.path)
        .collect();
    assert_eq!(
        listed,
        [Path::new("/a/other"), Path::new("/b/photos")],
        "ordered by path"
    );
    assert_eq!(
        catalog
            .root_by_path(Path::new("/a/other"))
            .unwrap()
            .unwrap()
            .id,
        b.id
    );
    assert!(catalog.root_by_path(Path::new("/nope")).unwrap().is_none());

    assert!(catalog.set_root_mode(b.id, RootMode::Watch).unwrap());
    assert_eq!(
        catalog
            .root_by_path(Path::new("/a/other"))
            .unwrap()
            .unwrap()
            .mode,
        RootMode::Watch
    );
    assert!(catalog.remove_root(b.id).unwrap());
    assert!(!catalog.remove_root(b.id).unwrap());
    assert!(!catalog.set_root_mode(b.id, RootMode::Once).unwrap());
    assert_eq!(catalog.list_roots().unwrap().len(), 1);
}

#[test]
fn removing_a_root_cascades_to_folders_photos_and_dependents() {
    let (_dir, catalog) = open();
    let (root, folder) = root_and_folder(&catalog);
    let ids = insert_photos(&catalog, &folder, 3);
    let album = catalog.create_album("A", None, now()).unwrap();
    catalog.add_to_album(album.id, &ids).unwrap();
    catalog
        .set_edit(ids[0], "[]".into(), "[]".into(), now())
        .unwrap();
    catalog.add_tags(&ids, &["x".to_owned()]).unwrap();

    catalog.remove_root(root.id).unwrap();
    assert!(catalog.get_folder(folder.id).unwrap().is_none());
    assert!(catalog.get_many(&ids).unwrap().is_empty());
    assert!(catalog.album_items(album.id).unwrap().is_empty());
    assert!(
        catalog.get_album(album.id).unwrap().is_some(),
        "albums outlive their photos"
    );
    assert_eq!(
        catalog.list_tags().unwrap()[0].count,
        0,
        "tag rows stay, links are gone"
    );
}

#[test]
fn folder_ensure_is_idempotent_and_keeps_caption() {
    let (_dir, catalog) = open();
    let (root, top) = root_and_folder(&catalog);
    assert_eq!((top.name.as_str(), top.parent_id), ("photos", None));
    let sub = catalog
        .ensure_folder(NewFolder {
            root_id: root.id,
            path: "/photos/2021".into(),
            parent_id: Some(top.id),
        })
        .unwrap();
    assert_eq!((sub.name.as_str(), sub.parent_id), ("2021", Some(top.id)));

    assert!(
        catalog
            .set_folder_caption(sub.id, Some("Summer".into()))
            .unwrap()
    );
    let again = catalog
        .ensure_folder(NewFolder {
            root_id: root.id,
            path: "/photos/2021".into(),
            parent_id: Some(top.id),
        })
        .unwrap();
    assert_eq!(again.id, sub.id);
    assert_eq!(again.caption.as_deref(), Some("Summer"));

    let all: Vec<_> = catalog
        .folders_in_root(root.id)
        .unwrap()
        .into_iter()
        .map(|f| f.id)
        .collect();
    assert_eq!(all, [top.id, sub.id]);
    assert_eq!(
        catalog
            .folder_by_path(Path::new("/photos/2021"))
            .unwrap()
            .unwrap()
            .id,
        sub.id
    );
    assert!(catalog.set_folder_caption(sub.id, None).unwrap());
    assert_eq!(catalog.get_folder(sub.id).unwrap().unwrap().caption, None);
}

#[test]
fn folder_without_a_matching_root_is_rejected() {
    let (_dir, catalog) = open();
    let err = catalog
        .ensure_folder(NewFolder {
            root_id: lb_core::RootId::new(99),
            path: "/x".into(),
            parent_id: None,
        })
        .unwrap_err();
    assert!(matches!(err, CatalogError::Sqlite(_)), "{err:?}");
}

#[test]
fn settings_roundtrip() {
    let (_dir, catalog) = open();
    assert_eq!(catalog.get_setting("theme").unwrap(), None);
    catalog.set_setting("theme", "\"dark\"").unwrap();
    catalog.set_setting("zoom", "128").unwrap();
    catalog.set_setting("theme", "\"light\"").unwrap();
    assert_eq!(
        catalog.get_setting("theme").unwrap().as_deref(),
        Some("\"light\"")
    );
    assert_eq!(
        catalog.list_settings().unwrap(),
        [
            ("theme".to_owned(), "\"light\"".to_owned()),
            ("zoom".to_owned(), "128".to_owned())
        ]
    );
    assert!(catalog.delete_setting("zoom").unwrap());
    assert!(!catalog.delete_setting("zoom").unwrap());
}

#[cfg(unix)]
#[test]
fn non_utf8_paths_are_rejected_not_mangled() {
    use std::os::unix::ffi::OsStrExt;
    let (_dir, catalog) = open();
    let bad = Path::new(std::ffi::OsStr::from_bytes(b"/photos/\xff\xfe"));
    let err = catalog.add_root(bad, RootMode::Watch, now()).unwrap_err();
    assert!(matches!(err, CatalogError::NonUtf8Path(_)), "{err:?}");
}
