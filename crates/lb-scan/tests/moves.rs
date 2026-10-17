//! Move and rename detection (SPEC §6.3.2).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use common::*;
use lb_core::{PhotoId, PhotoStatus, RootMode};
use lb_testkit::wait_until;

fn decorate(env: &Env, id: PhotoId) -> lb_core::AlbumId {
    let now = chrono::NaiveDateTime::default();
    env.catalog.set_starred(&[id], true).unwrap();
    env.catalog.set_rating(&[id], 3).unwrap();
    env.catalog
        .set_caption(&[id], Some("sunset".into()))
        .unwrap();
    env.catalog.add_tags(&[id], &["family".to_owned()]).unwrap();
    let album = env.catalog.create_album("Trip", None, now).unwrap();
    env.catalog.add_to_album(album.id, &[id]).unwrap();
    env.catalog
        .set_edit(id, r#"[{"op":"sepia"}]"#.into(), "[]".into(), now)
        .unwrap();
    album.id
}

fn assert_decorated(env: &Env, id: PhotoId, album: lb_core::AlbumId) {
    let p = env.catalog.get_photo(id).unwrap().unwrap();
    assert!(p.starred && p.rating == 3, "{p:?}");
    assert_eq!(p.caption.as_deref(), Some("sunset"));
    assert_eq!(p.edit_version, 1);
    assert_eq!(p.status, PhotoStatus::Ok);
    assert_eq!(
        env.catalog
            .tags_for_photo(id)
            .unwrap()
            .iter()
            .map(|t| t.name.as_str())
            .collect::<Vec<_>>(),
        ["family"]
    );
    assert_eq!(env.catalog.album_photo_ids(album).unwrap(), [id]);
    assert!(env.catalog.get_edit(id).unwrap().is_some());
}

#[test]
fn rename_preserves_metadata() {
    let env = env();
    let dir = env.subdir("lib");
    put(
        &generated_basic().join("2021/beach/beach_02.jpg"),
        &dir.join("before.jpg"),
        0,
    );
    let w = watch(&env, fast_watch());
    add_watch_root(&w, &dir);
    let id = env.photos_under(&dir)[0].id;
    let album = decorate(&env, id);

    std::fs::rename(dir.join("before.jpg"), dir.join("after.jpg")).unwrap();
    assert!(
        wait_until(WAIT, || env
            .catalog
            .get_photo(id)
            .unwrap()
            .unwrap()
            .path
            .ends_with("after.jpg")),
        "row follows the renamed file"
    );
    assert_decorated(&env, id, album);
    assert_eq!(
        env.photos_under(&dir).len(),
        1,
        "no duplicate row was created"
    );
}

#[test]
fn move_across_folders_preserves_metadata() {
    let env = env();
    let dir = env.subdir("lib");
    put(
        &generated_basic().join("2021/beach/beach_02.jpg"),
        &dir.join("a/photo.jpg"),
        0,
    );
    std::fs::create_dir_all(dir.join("b/c")).unwrap();
    let w = watch(&env, fast_watch());
    add_watch_root(&w, &dir);
    let id = env.photos_under(&dir)[0].id;
    let album = decorate(&env, id);

    std::fs::rename(dir.join("a/photo.jpg"), dir.join("b/c/moved.jpg")).unwrap();
    assert!(wait_until(WAIT, || env
        .catalog
        .get_photo(id)
        .unwrap()
        .unwrap()
        .path
        .ends_with("b/c/moved.jpg")));
    assert_decorated(&env, id, album);
    assert_eq!(env.photos_under(&dir).len(), 1);
    let folder = env
        .catalog
        .folder_by_path(&dir.join("b/c"))
        .unwrap()
        .unwrap();
    assert_eq!(
        env.catalog.get_photo(id).unwrap().unwrap().folder_id,
        folder.id
    );
}

#[test]
fn move_of_a_whole_directory_preserves_metadata() {
    let env = env();
    let dir = env.subdir("lib");
    for (i, n) in ["beach_01.jpg", "beach_02.jpg", "beach_03.jpg"]
        .iter()
        .enumerate()
    {
        put(
            &generated_basic().join("2021/beach").join(n),
            &dir.join("old").join(n),
            i as u64,
        );
    }
    let w = watch(&env, fast_watch());
    add_watch_root(&w, &dir);
    let before = env.photos_under(&dir);
    assert_eq!(before.len(), 3);
    let album = decorate(&env, before[0].id);
    std::fs::rename(dir.join("old"), dir.join("renamed")).unwrap();
    assert!(wait_until(WAIT, || before.iter().all(|p| {
        env.catalog
            .get_photo(p.id)
            .unwrap()
            .unwrap()
            .path
            .starts_with(dir.join("renamed"))
    })));
    assert_decorated(&env, before[0].id, album);
    assert_eq!(env.photos_under(&dir).len(), 3);
    assert!(
        env.photos_under(&dir)
            .iter()
            .all(|p| p.status == PhotoStatus::Ok)
    );
}

#[test]
fn rescan_relinks_moved_file() {
    let env = env();
    let dir = env.subdir("lib");
    put(
        &generated_basic().join("2021/beach/beach_02.jpg"),
        &dir.join("a/photo.jpg"),
        0,
    );
    put(
        &generated_basic().join("2021/beach/beach_03.jpg"),
        &dir.join("a/stay.jpg"),
        0,
    );
    let root = env.add_root(&dir, RootMode::Once);
    env.scan(&root);
    let photo = env
        .photos_under(&dir)
        .into_iter()
        .find(|p| p.path.ends_with("photo.jpg"))
        .unwrap();
    let album = decorate(&env, photo.id);

    std::fs::create_dir_all(dir.join("elsewhere")).unwrap();
    std::fs::rename(dir.join("a/photo.jpg"), dir.join("elsewhere/renamed.jpg")).unwrap();
    let (summary, _) = env.scan(&root);
    assert_eq!(
        (
            summary.moved,
            summary.added,
            summary.missing,
            summary.updated
        ),
        (1, 0, 0, 0),
        "{summary:?}"
    );
    assert_decorated(&env, photo.id, album);
    assert!(
        env.catalog
            .get_photo(photo.id)
            .unwrap()
            .unwrap()
            .path
            .ends_with("elsewhere/renamed.jpg")
    );
    assert_eq!(env.photos_under(&dir).len(), 2);
    assert_eq!(env.scan(&root).0.moved, 0, "relinking is idempotent");
}

#[test]
fn rescan_relinks_a_file_moved_between_roots_after_it_went_missing() {
    let env = env();
    let (a, b) = (env.subdir("root_a"), env.subdir("root_b"));
    put(
        &generated_basic().join("2021/beach/beach_02.jpg"),
        &a.join("p.jpg"),
        0,
    );
    let root_a = env.add_root(&a, RootMode::Once);
    let root_b = env.add_root(&b, RootMode::Once);
    env.scan(&root_a);
    let id = env.photos_under(&a)[0].id;
    let album = decorate(&env, id);
    std::fs::rename(a.join("p.jpg"), b.join("q.jpg")).unwrap();
    let (s, _) = env.scan(&root_a);
    assert_eq!(s.missing, 1);
    assert_eq!(
        env.catalog.get_photo(id).unwrap().unwrap().status,
        PhotoStatus::Missing
    );
    let (s, _) = env.scan(&root_b);
    assert_eq!((s.moved, s.added), (1, 0), "{s:?}");
    assert_decorated(&env, id, album);
    assert!(
        env.catalog
            .get_photo(id)
            .unwrap()
            .unwrap()
            .path
            .starts_with(&b)
    );
}

#[test]
fn copies_are_not_mistaken_for_moves() {
    let env = env();
    let dir = env.subdir("lib");
    put(
        &generated_basic().join("2021/beach/beach_02.jpg"),
        &dir.join("orig.jpg"),
        0,
    );
    let root = env.add_root(&dir, RootMode::Once);
    env.scan(&root);
    std::fs::copy(dir.join("orig.jpg"), dir.join("copy.jpg")).unwrap();
    let (s, _) = env.scan(&root);
    assert_eq!((s.added, s.moved, s.missing), (1, 0, 0), "{s:?}");
    assert_eq!(env.photos_under(&dir).len(), 2);
}
