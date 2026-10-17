use crate::repo::test_util::*;

#[test]
fn verify_accepts_a_healthy_catalog_and_flags_fts_drift() {
    let (_d, c) = open();
    let (_r, folder) = root_and_folder(&c);
    let ids = insert_photos(&c, &folder, 5);
    c.set_caption(&ids[..2], Some("sunny beach".into()))
        .unwrap();
    assert_eq!(c.verify().unwrap(), Vec::<String>::new());

    c.write(move |conn| {
        conn.execute(
            "UPDATE photos SET caption = 'moonlit harbour' WHERE id = ?1",
            [ids[3].get()],
        )?;
        Ok(())
    })
    .unwrap();
    let problems = c.verify().unwrap();
    assert!(!problems.is_empty(), "{problems:?}");
    c.rebuild_fts().unwrap();
    assert_eq!(c.verify().unwrap(), Vec::<String>::new());
}

#[test]
fn verify_flags_dangling_foreign_keys() {
    let (_d, c) = open();
    let (_r, folder) = root_and_folder(&c);
    let ids = insert_photos(&c, &folder, 1);
    c.write(move |conn| {
        conn.pragma_update(None, "foreign_keys", false)?;
        conn.execute(
            "INSERT INTO photo_tags (photo_id, tag_id) VALUES (?1, 999)",
            [ids[0].get()],
        )?;
        conn.pragma_update(None, "foreign_keys", true)?;
        Ok(())
    })
    .unwrap();
    let problems = c.verify().unwrap();
    assert!(
        problems.iter().any(|p| p.starts_with("foreign key")),
        "{problems:?}"
    );
}
