use crate::repo::test_util::*;
use crate::{PhotoQuery, QuerySource, SqlFragment};
use lb_core::{PhotoId, PhotoStatus, SortOrder, SqlValue};

fn ids(catalog: &crate::Catalog, q: &PhotoQuery) -> Vec<PhotoId> {
    catalog
        .query_photos(q)
        .unwrap()
        .into_iter()
        .map(|r| r.id)
        .collect()
}

#[test]
fn hidden_and_missing_are_excluded_unless_asked() {
    let (_d, catalog) = open();
    let (_, folder) = root_and_folder(&catalog);
    let p = insert_photos(&catalog, &folder, 4);
    catalog.set_hidden(&[p[1]], true).unwrap();
    catalog
        .set_photos_status(&[p[2]], PhotoStatus::Missing, None)
        .unwrap();
    let mut q = PhotoQuery::new(QuerySource::All);
    q.sort = SortOrder::FileNameAsc;
    assert_eq!(ids(&catalog, &q), vec![p[0], p[3]]);
    q.show_hidden = true;
    assert_eq!(ids(&catalog, &q), vec![p[0], p[1], p[3]]);
    q.show_missing = true;
    assert_eq!(ids(&catalog, &q), p);
}

#[test]
fn sources_and_orders() {
    let (_d, catalog) = open();
    let (_, folder) = root_and_folder(&catalog);
    let p = insert_photos(&catalog, &folder, 4);
    catalog.set_starred(&[p[3], p[0]], true).unwrap();
    let mut q = PhotoQuery::new(QuerySource::Starred);
    q.sort = SortOrder::FileNameAsc;
    assert_eq!(ids(&catalog, &q), vec![p[0], p[3]]);

    q.source = QuerySource::Folder(folder.id);
    assert_eq!(ids(&catalog, &q), p);
    q.sort = SortOrder::DateTakenDesc;
    assert_eq!(ids(&catalog, &q), p, "equal dates tie-break by file name");

    let album = catalog.create_album("A", None, now()).unwrap();
    catalog.add_to_album(album.id, &[p[2], p[0], p[3]]).unwrap();
    q.source = QuerySource::Album(album.id);
    assert_eq!(
        ids(&catalog, &q),
        vec![p[2], p[0], p[3]],
        "album order wins"
    );

    q.source = QuerySource::Ids(vec![p[1], p[3]]);
    q.sort = SortOrder::FileNameAsc;
    assert_eq!(ids(&catalog, &q), vec![p[1], p[3]]);
    q.source = QuerySource::Ids(vec![]);
    assert!(ids(&catalog, &q).is_empty());

    q.source = QuerySource::RecentlyEdited;
    assert!(ids(&catalog, &q).is_empty());
    catalog
        .set_edit(p[0], "[]".into(), "[]".into(), now())
        .unwrap();
    catalog
        .set_edit(
            p[1],
            "[]".into(),
            "[]".into(),
            now() + chrono::Duration::minutes(1),
        )
        .unwrap();
    assert_eq!(
        ids(&catalog, &q),
        vec![p[1], p[0]],
        "most recently edited first"
    );
}

#[test]
fn extra_fragment_and_fts_are_bound() {
    let (_d, catalog) = open();
    let (_, folder) = root_and_folder(&catalog);
    let p = insert_photos(&catalog, &folder, 3);
    catalog.set_rating(&[p[1]], 4).unwrap();
    catalog.set_rating(&[p[2]], 2).unwrap();
    catalog
        .set_caption(&[p[2]], Some("sunset at the beach".into()))
        .unwrap();
    let mut q = PhotoQuery::new(QuerySource::All);
    q.sort = SortOrder::FileNameAsc;
    q.extra = Some(SqlFragment {
        sql: "p.rating >= ?".into(),
        params: vec![SqlValue::Int(3)],
    });
    assert_eq!(ids(&catalog, &q), vec![p[1]]);
    q.extra = Some(SqlFragment {
        sql: "p.caption = ?".into(),
        params: vec![SqlValue::Text("x' OR 1=1 --".into())],
    });
    assert!(
        ids(&catalog, &q).is_empty(),
        "values never reach the SQL text"
    );
    q.extra = None;
    q.fts = Some("sunse*".into());
    assert_eq!(ids(&catalog, &q), vec![p[2]]);
}

#[test]
fn date_falls_back_to_file_mtime() {
    let (_d, catalog) = open();
    let (_, folder) = root_and_folder(&catalog);
    insert_photos(&catalog, &folder, 1);
    let rows = catalog
        .query_photos(&PhotoQuery::new(QuerySource::All))
        .unwrap();
    let epoch = chrono::NaiveDate::from_ymd_opt(2020, 1, 1)
        .and_then(|d| d.and_hms_opt(0, 0, 0))
        .unwrap();
    assert!(
        (rows[0].date - epoch).num_hours().abs() <= 14,
        "mtime 1577836800 expressed in local time, got {}",
        rows[0].date
    );
}

#[test]
fn restore_album_items_keeps_exact_positions() {
    let (_d, catalog) = open();
    let (_, folder) = root_and_folder(&catalog);
    let p = insert_photos(&catalog, &folder, 3);
    let album = catalog.create_album("A", None, now()).unwrap();
    catalog.add_to_album(album.id, &p).unwrap();
    let before = catalog.album_items(album.id).unwrap();
    catalog.remove_from_album(album.id, &[p[1]]).unwrap();
    catalog.restore_album_items(album.id, &[before[1]]).unwrap();
    assert_eq!(catalog.album_items(album.id).unwrap(), before);
}
