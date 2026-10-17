//! M6.4: file operations on originals.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use lb_app::{AppError, AppEvent, LightboxOptions, ViewSource};
use lb_core::{EditOp, PhotoStatus, RootMode};
use lb_testkit::OpenAction;
use lb_thumbs::ThumbSize;

#[test]
fn rename_file_updates_catalog() {
    let t = common::library(2);
    let (lb, id) = (&t.lb, t.ids[0]);
    lb.photos().set_starred(&[id], true).unwrap();
    lb.edits().push(id, EditOp::Sepia).unwrap();
    let old = lb.photos().get(id).unwrap().unwrap();
    let bytes = std::fs::read(&old.path).unwrap();

    let renamed = lb.photos().rename(id, "  Beach day ").unwrap();
    assert_eq!(renamed.id, id);
    assert_eq!(
        renamed.path,
        old.path.with_file_name("Beach day.jpg"),
        "extension is kept"
    );
    assert!(
        renamed.starred && renamed.edit_version == old.edit_version,
        "user data is kept"
    );
    assert!(!old.path.exists());
    assert_eq!(
        std::fs::read(&renamed.path).unwrap(),
        bytes,
        "file content is untouched"
    );
    assert_eq!(lb.edits().stack(id).unwrap().ops, vec![EditOp::Sepia]);
    lb.thumbs().generate(id, ThumbSize::S).unwrap();

    let found = lb.photos().list_ids(&ViewSource::AllPhotos, false).unwrap();
    assert!(found.contains(&id));
    let by_name = lb
        .catalog_handle()
        .photo_by_path(&renamed.path)
        .unwrap()
        .unwrap();
    assert_eq!(by_name.id, id);

    let png = lb.photos().rename(id, "beach.png").unwrap();
    assert_eq!(
        png.path.file_name().unwrap(),
        "beach.png",
        "an explicit extension wins"
    );
    assert_eq!(
        lb.photos().rename(id, "beach.png").unwrap().path,
        png.path,
        "same name is a no-op"
    );

    let rescan = lb
        .library()
        .rescan(lb.library().roots().unwrap()[0].id)
        .unwrap();
    assert_eq!(
        (rescan.added, rescan.missing),
        (0, 0),
        "a rescan sees nothing new or missing"
    );
}

#[test]
fn rename_rejects_invalid_names() {
    let t = common::library(2);
    let (lb, id) = (&t.lb, t.ids[0]);
    let before = lb.photos().get(id).unwrap().unwrap();
    let other = lb.photos().get(t.ids[1]).unwrap().unwrap();
    let other_name = other.path.file_name().unwrap().to_str().unwrap().to_owned();

    let mut bad: Vec<String> = [
        "",
        "   ",
        "a/b",
        "a\\b",
        ".",
        "..",
        ".hidden.jpg",
        "notes.txt",
        "a.exe",
    ]
    .iter()
    .map(|s| (*s).to_owned())
    .collect();
    bad.push("x".repeat(300));
    if cfg!(windows) {
        bad.extend([
            "CON".to_owned(),
            "nul.jpg".to_owned(),
            "trailing.".to_owned(),
        ]);
    }
    for name in &bad {
        let err = lb.photos().rename(id, name).unwrap_err();
        assert!(matches!(err, AppError::Invalid(_)), "{name:?} -> {err}");
    }
    let err = lb.photos().rename(id, &other_name).unwrap_err();
    assert!(matches!(err, AppError::AlreadyExists(_)), "{err}");
    assert!(matches!(
        lb.photos().rename(lb_core::PhotoId::new(999), "x"),
        Err(AppError::NotFound("photo"))
    ));

    let after = lb.photos().get(id).unwrap().unwrap();
    assert_eq!(after, before, "nothing changed in the catalog");
    assert!(before.path.exists(), "nothing changed on disk");
    assert!(other.path.exists());
}

#[test]
fn move_cross_folder_preserves_id() {
    let t = common::build(
        &[("a", 2), ("b", 1), ("b/deep", 0)],
        LightboxOptions::default(),
    );
    let lb = &t.lb;
    let (a0, a1) = (t.ids[0], t.ids[1]);
    lb.photos().set_rating(&[a0], 4).unwrap();
    lb.photos().set_caption(&[a0], Some("keep me")).unwrap();
    lb.tags().add(&[a0], &["tag".to_owned()]).unwrap();
    let album = lb.albums().create("Al", None).unwrap();
    lb.albums().add_photos(album.id, &[a0]).unwrap();
    lb.edits().push(a0, EditOp::BlackAndWhite).unwrap();
    let original = lb.photos().get(a0).unwrap().unwrap();
    let bytes = std::fs::read(&original.path).unwrap();
    let dest = t.photos.path().join("b/deep");

    let report = lb.photos().move_to_folder(&[a0, a0], &dest).unwrap();
    assert_eq!(report.moved, vec![a0]);
    assert!(report.failed.is_empty() && report.unchanged.is_empty());

    let moved = lb.photos().get(a0).unwrap().unwrap();
    assert_eq!(moved.id, a0, "id is preserved");
    assert_eq!(moved.path, dest.join("p00.jpg"));
    assert!(!original.path.exists());
    assert_eq!(std::fs::read(&moved.path).unwrap(), bytes);
    assert_eq!(
        (moved.rating, moved.caption.as_deref()),
        (4, Some("keep me"))
    );
    assert_eq!(lb.tags().for_photo(a0).unwrap().len(), 1);
    assert_eq!(lb.albums().photo_ids(album.id).unwrap(), vec![a0]);
    assert_eq!(
        lb.edits().stack(a0).unwrap().ops,
        vec![EditOp::BlackAndWhite]
    );
    let folder = lb
        .catalog_handle()
        .get_folder(moved.folder_id)
        .unwrap()
        .unwrap();
    assert_eq!(folder.path, dest);
    assert_eq!(folder.name, "deep");
    assert_ne!(moved.folder_id, original.folder_id);

    let report = lb.photos().move_to_folder(&[a0, a1], &dest).unwrap();
    assert_eq!(
        (report.unchanged.clone(), report.moved.clone()),
        (vec![a0], vec![a1])
    );

    let conflict_dir = t.photos.path().join("a");
    std::fs::write(conflict_dir.join("p00.jpg"), b"squatter").unwrap();
    let report = lb.photos().move_to_folder(&[a0], &conflict_dir).unwrap();
    assert_eq!(report.failed.len(), 1, "never overwrites an existing file");
    assert_eq!(
        std::fs::read(conflict_dir.join("p00.jpg")).unwrap(),
        b"squatter"
    );
    assert_eq!(lb.photos().get(a0).unwrap().unwrap().path, moved.path);

    let outside = tempfile::tempdir().unwrap();
    let b0 = t.ids[2];
    assert!(matches!(
        lb.photos().move_to_folder(&[b0], outside.path()),
        Err(AppError::Invalid(_))
    ));
    assert!(matches!(
        lb.photos()
            .move_to_folder(&[b0], &t.photos.path().join("nope")),
        Err(AppError::Invalid(_))
    ));
    assert!(lb.photos().get(b0).unwrap().unwrap().path.exists());

    let rescan = lb
        .library()
        .rescan(lb.library().roots().unwrap()[0].id)
        .unwrap();
    assert_eq!(rescan.missing, 0, "the catalog agrees with the disk");
}

#[test]
fn delete_moves_to_trash() {
    let f = common::fakes();
    let t = common::build(&[("", 3)], f.options.clone());
    let lb = &t.lb;
    let rx = lb.events();
    let paths: Vec<_> = t
        .ids
        .iter()
        .map(|id| lb.photos().get(*id).unwrap().unwrap().path)
        .collect();
    let bytes = std::fs::read(&paths[0]).unwrap();

    let report = lb.photos().delete(&t.ids[..2]).unwrap();
    assert_eq!(report.deleted, t.ids[..2].to_vec());
    assert!(report.failed.is_empty());
    assert!(!paths[0].exists() && !paths[1].exists());
    assert!(paths[2].exists());
    assert_eq!(f.trash.trashed(), paths[..2].to_vec());
    let in_trash = std::fs::read_dir(f.trash.dir()).unwrap().count();
    assert_eq!(in_trash, 2, "files are in the trash, not deleted");
    assert_eq!(std::fs::read(f.trash.dir().join("p00.jpg")).unwrap(), bytes);
    assert_eq!(lb.photos().all_ids().unwrap(), vec![t.ids[2]]);
    lb.bus().flush();
    assert!(
        rx.try_iter()
            .any(|e| matches!(e, AppEvent::PhotosRemoved { ids } if ids.len() == 2))
    );

    f.trash.set_unavailable(true);
    let report = lb.photos().delete(&[t.ids[2]]).unwrap();
    assert!(report.deleted.is_empty());
    assert_eq!(report.failed.len(), 1);
    assert!(
        paths[2].exists(),
        "no hard delete when the trash is unavailable"
    );
    assert!(
        lb.photos().get(t.ids[2]).unwrap().is_some(),
        "the photo stays in the library"
    );

    f.trash.set_unavailable(false);
    std::fs::remove_file(&paths[2]).unwrap();
    lb.photos()
        .delete(&[t.ids[2], lb_core::PhotoId::new(555)])
        .unwrap();
    assert!(
        lb.photos().all_ids().unwrap().is_empty(),
        "a vanished file is just forgotten"
    );
}

#[test]
fn reveal_and_open_use_the_opener() {
    let f = common::fakes();
    let t = common::build(&[("", 1)], f.options.clone());
    let path = t.lb.photos().get(t.ids[0]).unwrap().unwrap().path;
    t.lb.photos().reveal(t.ids[0]).unwrap();
    t.lb.photos().open_externally(t.ids[0]).unwrap();
    assert_eq!(
        f.opener.calls(),
        vec![(OpenAction::Reveal, path.clone()), (OpenAction::Open, path)]
    );
    assert!(matches!(
        t.lb.photos().reveal(lb_core::PhotoId::new(5)),
        Err(AppError::NotFound("photo"))
    ));
}

#[test]
fn relink_missing_by_quick_key() {
    let t = common::build(&[("", 2)], LightboxOptions::default());
    let lb = &t.lb;
    lb.photos().set_starred(&t.ids, true).unwrap();
    lb.tags().add(&t.ids, &["keep".to_owned()]).unwrap();
    let originals: Vec<_> = t
        .ids
        .iter()
        .map(|id| lb.photos().get(*id).unwrap().unwrap().path)
        .collect();

    let elsewhere = tempfile::tempdir().unwrap();
    let new_dir = elsewhere.path().join("moved");
    std::fs::create_dir_all(&new_dir).unwrap();
    for p in &originals {
        std::fs::rename(p, new_dir.join(p.file_name().unwrap())).unwrap();
    }
    let root = lb.library().roots().unwrap()[0].id;
    let summary = lb.library().rescan(root).unwrap();
    assert_eq!(summary.missing, 2);
    assert!(
        lb.photos()
            .get_many(&t.ids)
            .unwrap()
            .iter()
            .all(|p| p.status == PhotoStatus::Missing)
    );
    assert!(
        lb.photos()
            .list_ids(&ViewSource::AllPhotos, false)
            .unwrap()
            .is_empty()
    );

    let relinked = lb.library().relink_missing(&new_dir).unwrap();
    assert_eq!(relinked, 2);
    let photos = lb.photos().get_many(&t.ids).unwrap();
    for (photo, original) in photos.iter().zip(&originals) {
        assert_eq!(photo.status, PhotoStatus::Ok);
        assert_eq!(photo.path, new_dir.join(original.file_name().unwrap()));
        assert!(photo.starred, "user data survives relinking");
    }
    assert_eq!(lb.tags().for_photo(t.ids[0]).unwrap().len(), 1);
    assert_eq!(
        lb.photos().all_ids().unwrap().len(),
        2,
        "no duplicate rows were created"
    );
    assert!(
        lb.library()
            .roots()
            .unwrap()
            .iter()
            .any(|r| r.path == new_dir.canonicalize().unwrap() && r.mode == RootMode::Once)
    );

    assert_eq!(
        lb.library().relink_missing(&new_dir).unwrap(),
        0,
        "nothing left to relink"
    );
}

#[test]
fn rename_folder_updates_descendants() {
    let t = common::build(
        &[("a", 1), ("a/deep", 1), ("ab", 1)],
        LightboxOptions::default(),
    );
    let lb = &t.lb;
    let folders = lb
        .library()
        .folders(lb.library().roots().unwrap()[0].id)
        .unwrap();
    let a = folders.iter().find(|f| f.name == "a").unwrap().clone();

    let renamed = lb.library().rename_folder(a.id, "Trip").unwrap();
    assert_eq!(renamed.path, t.photos.path().join("Trip"));
    assert!(t.photos.path().join("Trip/deep/p00.jpg").is_file());
    assert!(!t.photos.path().join("a").exists());
    let photos = lb.photos().all().unwrap();
    assert!(
        photos
            .iter()
            .any(|p| p.path == t.photos.path().join("Trip/deep/p00.jpg"))
    );
    assert!(
        photos
            .iter()
            .any(|p| p.path == t.photos.path().join("ab/p00.jpg"))
    );

    assert!(matches!(
        lb.library().rename_folder(a.id, "ab"),
        Err(AppError::AlreadyExists(_))
    ));
    assert!(matches!(
        lb.library().rename_folder(a.id, "x/y"),
        Err(AppError::Invalid(_))
    ));
    let root_folder = folders
        .iter()
        .find(|f| f.path == t.photos.path().canonicalize().unwrap())
        .unwrap();
    assert!(matches!(
        lb.library().rename_folder(root_folder.id, "zzz"),
        Err(AppError::Invalid(_))
    ));
    let rescan = lb
        .library()
        .rescan(lb.library().roots().unwrap()[0].id)
        .unwrap();
    assert_eq!((rescan.added, rescan.missing), (0, 0));
}
