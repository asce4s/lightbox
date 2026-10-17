use crate::CatalogError;
use crate::repo::test_util::*;
use lb_core::PhotoId;

#[test]
fn edit_set_bumps_edit_version_and_roundtrips() {
    let (_dir, catalog) = open();
    let (_, folder) = root_and_folder(&catalog);
    let id = insert_photos(&catalog, &folder, 1)[0];
    assert_eq!(catalog.get_photo(id).unwrap().unwrap().edit_version, 0);
    assert!(catalog.get_edit(id).unwrap().is_none());

    let v1 = catalog
        .set_edit(id, r#"[{"op":"sepia"}]"#.into(), "[]".into(), now())
        .unwrap();
    let v2 = catalog
        .set_edit(
            id,
            r#"[{"op":"sepia"},{"op":"vignette"}]"#.into(),
            "[]".into(),
            now(),
        )
        .unwrap();
    assert_eq!((v1, v2), (1, 2));
    let edit = catalog.get_edit(id).unwrap().unwrap();
    assert_eq!(edit.ops_json, r#"[{"op":"sepia"},{"op":"vignette"}]"#);
    assert_eq!((edit.redo_json.as_str(), edit.updated_at), ("[]", now()));
    assert_eq!(catalog.get_photo(id).unwrap().unwrap().edit_version, 2);

    let v3 = catalog.clear_edit(id).unwrap();
    assert_eq!(v3, 3, "reverting is an edit-stack change too");
    assert!(catalog.get_edit(id).unwrap().is_none());
}

#[test]
fn edit_on_missing_photo_is_not_found_and_leaves_nothing() {
    let (_dir, catalog) = open();
    let err = catalog
        .set_edit(PhotoId::new(404), "[]".into(), "[]".into(), now())
        .unwrap_err();
    assert!(matches!(err, CatalogError::NotFound("photo")), "{err:?}");
    assert!(catalog.get_edit(PhotoId::new(404)).unwrap().is_none());
    assert!(matches!(
        catalog.clear_edit(PhotoId::new(404)).unwrap_err(),
        CatalogError::NotFound("photo")
    ));
}
