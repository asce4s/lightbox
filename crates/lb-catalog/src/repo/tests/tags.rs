use crate::CatalogError;
use crate::repo::test_util::*;
use lb_core::InvalidTagName;

fn names(tags: Vec<lb_core::Tag>) -> Vec<String> {
    tags.into_iter().map(|t| t.name).collect()
}

#[test]
fn tag_normalization() {
    let (_dir, catalog) = open();
    let beach = catalog.get_or_create_tag("  Beach \t Trip ").unwrap();
    assert_eq!(beach.name, "Beach Trip", "trimmed and whitespace collapsed");
    assert_eq!(
        catalog.get_or_create_tag("beach trip").unwrap(),
        beach,
        "case-insensitive, first spelling wins"
    );
    assert_eq!(
        catalog.get_or_create_tag("BEACH   TRIP").unwrap().id,
        beach.id
    );
    assert_eq!(
        catalog.find_tag("bEaCh tRiP").unwrap().unwrap().id,
        beach.id
    );
    assert!(catalog.find_tag("nothing").unwrap().is_none());

    let cafe = catalog.get_or_create_tag("Café").unwrap();
    assert_eq!(
        catalog.get_or_create_tag("CAFÉ").unwrap().id,
        cafe.id,
        "non-ASCII names fold case too"
    );
    assert_eq!(catalog.get_or_create_tag("café").unwrap().id, cafe.id);

    for (bad, why) in [
        ("", InvalidTagName::Empty),
        ("   ", InvalidTagName::Empty),
        ("a,b", InvalidTagName::Comma),
        (&"x".repeat(65), InvalidTagName::TooLong),
    ] {
        let err = catalog.get_or_create_tag(bad).unwrap_err();
        assert!(
            matches!(&err, CatalogError::InvalidTagName(e) if *e == why),
            "{bad:?}: {err:?}"
        );
    }
    assert_eq!(
        catalog.list_tags().unwrap().len(),
        2,
        "rejected names create nothing"
    );
}

#[test]
fn tags_add_remove_and_count() {
    let (_dir, catalog) = open();
    let (_, folder) = root_and_folder(&catalog);
    let p = insert_photos(&catalog, &folder, 3);

    let added = catalog
        .add_tags(&p[..2], &["Beach".into(), "family".into()])
        .unwrap();
    assert_eq!(added, 4);
    assert_eq!(
        catalog.add_tags(&p[..2], &["beach".into()]).unwrap(),
        0,
        "re-adding is a no-op"
    );
    assert_eq!(catalog.add_tags(&p[1..], &["BEACH".into()]).unwrap(), 1);
    assert_eq!(
        names(catalog.tags_for_photo(p[1]).unwrap()),
        ["Beach", "family"]
    );
    assert_eq!(names(catalog.tags_for_photo(p[2]).unwrap()), ["Beach"]);

    let usage: Vec<_> = catalog
        .list_tags()
        .unwrap()
        .into_iter()
        .map(|u| (u.tag.name, u.count))
        .collect();
    assert_eq!(usage, [("Beach".to_owned(), 3), ("family".to_owned(), 2)]);

    assert_eq!(
        catalog
            .remove_tags(&p, &["FAMILY".into(), "unknown".into()])
            .unwrap(),
        2
    );
    assert_eq!(names(catalog.tags_for_photo(p[0]).unwrap()), ["Beach"]);
    assert_eq!(
        catalog
            .list_tags()
            .unwrap()
            .iter()
            .find(|u| u.tag.name == "family")
            .unwrap()
            .count,
        0
    );
}

#[test]
fn tag_add_is_atomic_when_a_name_is_invalid() {
    let (_dir, catalog) = open();
    let (_, folder) = root_and_folder(&catalog);
    let p = insert_photos(&catalog, &folder, 1);
    assert!(
        catalog
            .add_tags(&p, &["ok".into(), "bad,name".into()])
            .is_err()
    );
    assert!(
        catalog.list_tags().unwrap().is_empty(),
        "nothing created when the batch fails"
    );
    assert!(catalog.tags_for_photo(p[0]).unwrap().is_empty());
}

#[test]
fn tag_rename_merges_into_existing() {
    let (_dir, catalog) = open();
    let (_, folder) = root_and_folder(&catalog);
    let p = insert_photos(&catalog, &folder, 3);
    catalog.add_tags(&p[..2], &["sea".into()]).unwrap();
    catalog.add_tags(&p[1..], &["ocean".into()]).unwrap();
    let sea = catalog.find_tag("sea").unwrap().unwrap();
    let ocean = catalog.find_tag("ocean").unwrap().unwrap();

    let renamed = catalog.rename_tag(sea.id, "Sea Side").unwrap();
    assert_eq!((renamed.id, renamed.name.as_str()), (sea.id, "Sea Side"));
    assert_eq!(
        catalog.rename_tag(sea.id, "sea side").unwrap().id,
        sea.id,
        "case-only rename is not a merge"
    );

    let merged = catalog.rename_tag(sea.id, "OCEAN").unwrap();
    assert_eq!(merged.id, ocean.id, "the existing target survives");
    let usage = catalog.list_tags().unwrap();
    assert_eq!(usage.len(), 1);
    assert_eq!(
        (usage[0].tag.name.as_str(), usage[0].count),
        ("ocean", 3),
        "union of photos, p[1] counted once"
    );
    for photo in &p {
        assert_eq!(names(catalog.tags_for_photo(*photo).unwrap()), ["ocean"]);
    }

    assert!(matches!(
        catalog.rename_tag(sea.id, "x").unwrap_err(),
        CatalogError::NotFound("tag")
    ));
    assert!(matches!(
        catalog.rename_tag(ocean.id, "a,b").unwrap_err(),
        CatalogError::InvalidTagName(_)
    ));
    assert!(catalog.delete_tag(ocean.id).unwrap());
    assert!(catalog.tags_for_photo(p[0]).unwrap().is_empty());
}
