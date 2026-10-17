use crate::repo::test_util::*;
use crate::{Catalog, NewFolder};
use lb_core::{PhotoId, RootMode};
use std::path::Path;

fn hits(catalog: &Catalog, query: &str) -> Vec<PhotoId> {
    catalog
        .read(|c| {
            let mut stmt =
                c.prepare("SELECT rowid FROM photos_fts WHERE photos_fts MATCH ?1 ORDER BY rowid")?;
            let rows = stmt.query_map([query], |r| Ok(PhotoId::new(r.get(0)?)))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .unwrap()
}

/// Every `(term, column, docs, occurrences)` in the index, via a temporary fts5vocab table.
fn index_dump(catalog: &Catalog) -> Vec<(String, String, i64, i64)> {
    catalog
        .write(|c| {
            c.execute_batch("CREATE VIRTUAL TABLE IF NOT EXISTS temp.fts_vocab USING fts5vocab('main', 'photos_fts', 'col')")?;
            let rows = {
                let mut stmt = c.prepare("SELECT term, col, doc, cnt FROM temp.fts_vocab ORDER BY term, col")?;
                stmt.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?.collect::<rusqlite::Result<Vec<_>>>()?
            };
            c.execute_batch("DROP TABLE temp.fts_vocab")?;
            Ok(rows)
        })
        .unwrap()
}

struct Fixture {
    _dir: tempfile::TempDir,
    catalog: Catalog,
    folder: lb_core::Folder,
    ids: Vec<PhotoId>,
}

fn fixture() -> Fixture {
    let (dir, catalog) = open();
    let (_, folder) = root_and_folder(&catalog);
    let mut rows = vec![
        upsert(&folder, "beach_01.jpg"),
        upsert(&folder, "mountain.jpg"),
        upsert(&folder, "IMG_0003.jpg"),
    ];
    rows[0].camera_make = Some("Canon".into());
    rows[0].camera_model = Some("EOS R6".into());
    let ids = catalog
        .upsert_photos(rows, now())
        .unwrap()
        .into_iter()
        .map(|o| o.id)
        .collect();
    Fixture {
        _dir: dir,
        catalog,
        folder,
        ids,
    }
}

#[test]
fn fts_updated_on_upsert() {
    let Fixture {
        _dir: _keep,
        catalog,
        folder,
        ids,
        ..
    } = fixture();
    assert_eq!(hits(&catalog, "beach"), [ids[0]], "file name tokens");
    assert_eq!(hits(&catalog, "beac*"), [ids[0]], "prefix queries");
    assert_eq!(hits(&catalog, "jpg").len(), 3);
    assert_eq!(hits(&catalog, "folder_path:photos").len(), 3, "folder path");
    assert_eq!(hits(&catalog, "camera:canon"), [ids[0]]);
    assert_eq!(hits(&catalog, "camera:\"eos r6\""), [ids[0]]);

    let mut rescanned = upsert(&folder, "beach_01.jpg");
    rescanned.camera_make = Some("Nikon".into());
    rescanned.camera_model = Some("Z6".into());
    rescanned.import_caption = Some("Sunny Café".into());
    catalog.upsert_photos(vec![rescanned], now()).unwrap();
    assert!(
        hits(&catalog, "camera:canon").is_empty(),
        "old camera is gone"
    );
    assert_eq!(hits(&catalog, "camera:nikon"), [ids[0]]);
    assert_eq!(
        hits(&catalog, "caption:cafe"),
        [ids[0]],
        "imported caption indexed, diacritics folded"
    );
}

#[test]
fn fts_updated_on_caption_tag_album_change() {
    let Fixture {
        _dir: _keep,
        catalog,
        ids,
        ..
    } = fixture();

    assert_eq!(
        catalog
            .set_caption(&ids[..2], Some("Sunset over the harbour".into()))
            .unwrap(),
        2
    );
    assert_eq!(hits(&catalog, "caption:sunset"), [ids[0], ids[1]]);
    catalog.set_caption(&ids[..1], Some("Dawn".into())).unwrap();
    assert_eq!(
        hits(&catalog, "sunset"),
        [ids[1]],
        "replaced caption leaves no stale terms"
    );
    assert_eq!(hits(&catalog, "dawn"), [ids[0]]);
    catalog.set_caption(&ids, None).unwrap();
    assert!(
        hits(&catalog, "sunset OR dawn").is_empty(),
        "cleared captions leave the index"
    );

    catalog
        .add_tags(&ids[..2], &["Family Trip".into(), "Beach".into()])
        .unwrap();
    assert_eq!(hits(&catalog, "tags:family"), [ids[0], ids[1]]);
    assert_eq!(hits(&catalog, "tags:\"family trip\""), [ids[0], ids[1]]);
    catalog
        .remove_tags(&ids[..1], &["family trip".into()])
        .unwrap();
    assert_eq!(hits(&catalog, "tags:family"), [ids[1]]);
    let beach = catalog.find_tag("beach").unwrap().unwrap();
    catalog.rename_tag(beach.id, "Coast").unwrap();
    assert!(hits(&catalog, "tags:beach").is_empty());
    assert_eq!(hits(&catalog, "tags:coast"), [ids[0], ids[1]]);
    let family = catalog.find_tag("family trip").unwrap().unwrap();
    catalog.rename_tag(family.id, "coast").unwrap();
    assert_eq!(
        hits(&catalog, "tags:family"),
        Vec::<PhotoId>::new(),
        "merge reindexes the merged tag's photos"
    );
    assert_eq!(hits(&catalog, "tags:coast"), [ids[0], ids[1]]);
    let coast = catalog.find_tag("coast").unwrap().unwrap();
    catalog.delete_tag(coast.id).unwrap();
    assert!(
        hits(&catalog, "tags:coast").is_empty(),
        "deleting a tag reindexes its photos"
    );

    let album = catalog.create_album("Summer 2021", None, now()).unwrap();
    assert!(hits(&catalog, "album_names:summer").is_empty());
    catalog.add_to_album(album.id, &ids[1..]).unwrap();
    assert_eq!(hits(&catalog, "album_names:summer"), [ids[1], ids[2]]);
    catalog.remove_from_album(album.id, &ids[2..]).unwrap();
    assert_eq!(hits(&catalog, "album_names:summer"), [ids[1]]);
    catalog.rename_album(album.id, "Winter").unwrap();
    assert!(hits(&catalog, "summer").is_empty());
    assert_eq!(hits(&catalog, "album_names:winter"), [ids[1]]);
    let copy = catalog.duplicate_album(album.id, "Holiday", now()).unwrap();
    assert_eq!(hits(&catalog, "album_names:holiday"), [ids[1]]);
    catalog.delete_album(copy.id).unwrap();
    catalog.delete_album(album.id).unwrap();
    assert!(
        hits(&catalog, "winter OR holiday").is_empty(),
        "deleting albums reindexes members"
    );
}

#[test]
fn fts_rows_removed_when_photos_are_deleted() {
    let Fixture {
        _dir: _keep,
        catalog,
        folder,
        ids,
        ..
    } = fixture();
    let root_id = folder.root_id;
    catalog
        .write(move |c| Ok(c.execute("DELETE FROM photos WHERE id = ?1", [ids[0].get()])?))
        .unwrap();
    assert!(hits(&catalog, "beach").is_empty(), "direct delete");
    assert_eq!(hits(&catalog, "jpg").len(), 2);

    catalog.remove_root(root_id).unwrap();
    assert!(
        hits(&catalog, "jpg").is_empty(),
        "FK cascade from the root removes index rows too"
    );
}

#[test]
fn fts_follows_folder_path_for_new_folders() {
    let Fixture {
        _dir: _keep,
        catalog,
        folder,
        ..
    } = fixture();
    let sub = catalog
        .ensure_folder(NewFolder {
            root_id: folder.root_id,
            path: Path::new("/photos/Holidays 2021").into(),
            parent_id: Some(folder.id),
        })
        .unwrap();
    let id = catalog
        .upsert_photos(vec![upsert(&sub, "x.jpg")], now())
        .unwrap()[0]
        .id;
    assert_eq!(hits(&catalog, "folder_path:holidays"), [id]);
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }
}

const WORDS: &[&str] = &[
    "beach", "mountain", "café", "Sunset", "family", "harbour", "winter", "日本", "river", "Zoë",
];
const CAMERAS: &[(&str, &str)] = &[
    ("Canon", "EOS R6"),
    ("Nikon", "Z6"),
    ("Sony", "A7 IV"),
    ("", ""),
];

#[test]
fn fts_incremental_equals_rebuild() {
    let (_dir, catalog) = open();
    let (root, top) = root_and_folder(&catalog);
    let folders = [
        top.clone(),
        catalog
            .ensure_folder(NewFolder {
                root_id: root.id,
                path: "/photos/trips".into(),
                parent_id: Some(top.id),
            })
            .unwrap(),
    ];
    let root2 = catalog
        .add_root(Path::new("/other"), RootMode::Once, now())
        .unwrap();
    let other = catalog
        .ensure_folder(NewFolder {
            root_id: root2.id,
            path: "/other".into(),
            parent_id: None,
        })
        .unwrap();
    let mut rng = Rng(0x9E37_79B9_7F4A_7C15);

    let mut photos: Vec<PhotoId> = Vec::new();
    let mut albums = Vec::new();
    for step in 0..400 {
        match rng.below(12) {
            0..=2 => {
                let folder = if step % 17 == 0 {
                    &other
                } else {
                    rng.pick(&folders)
                };
                let name = format!("{}_{}.jpg", rng.pick(WORDS), rng.below(30));
                let mut row = upsert(folder, &name);
                let (make, model) = *rng.pick(CAMERAS);
                row.camera_make = (!make.is_empty()).then(|| make.to_owned());
                row.camera_model = (!model.is_empty()).then(|| model.to_owned());
                if rng.below(3) == 0 {
                    row.import_caption = Some(format!("{} {}", rng.pick(WORDS), rng.pick(WORDS)));
                }
                let out = catalog.upsert_photos(vec![row], now()).unwrap();
                if !photos.contains(&out[0].id) {
                    photos.push(out[0].id);
                }
            }
            3 if !photos.is_empty() => {
                let subset: Vec<_> = (0..1 + rng.below(4)).map(|_| *rng.pick(&photos)).collect();
                let caption =
                    (rng.below(4) != 0).then(|| format!("{} {}", rng.pick(WORDS), rng.pick(WORDS)));
                catalog.set_caption(&subset, caption).unwrap();
            }
            4..=5 if !photos.is_empty() => {
                let subset: Vec<_> = (0..1 + rng.below(4)).map(|_| *rng.pick(&photos)).collect();
                let names = vec![
                    rng.pick(WORDS).to_uppercase(),
                    format!("{} {}", rng.pick(WORDS), rng.pick(WORDS)),
                ];
                catalog.add_tags(&subset, &names).unwrap();
            }
            6 if !photos.is_empty() => {
                let subset: Vec<_> = (0..1 + rng.below(4)).map(|_| *rng.pick(&photos)).collect();
                catalog
                    .remove_tags(&subset, &[rng.pick(WORDS).to_string()])
                    .unwrap();
            }
            7 => {
                let tags = catalog.list_tags().unwrap();
                if !tags.is_empty() {
                    let tag = rng.pick(&tags).tag.clone();
                    if rng.below(3) == 0 {
                        catalog.delete_tag(tag.id).unwrap();
                    } else {
                        catalog.rename_tag(tag.id, rng.pick(WORDS)).unwrap();
                    }
                }
            }
            8 => albums.push(
                catalog
                    .create_album(
                        &format!("{} {}", rng.pick(WORDS), rng.below(5)),
                        None,
                        now(),
                    )
                    .unwrap()
                    .id,
            ),
            9 if !albums.is_empty() && !photos.is_empty() => {
                let album = *rng.pick(&albums);
                let subset: Vec<_> = (0..1 + rng.below(5)).map(|_| *rng.pick(&photos)).collect();
                if rng.below(3) == 0 {
                    catalog.remove_from_album(album, &subset).unwrap();
                } else {
                    catalog.add_to_album(album, &subset).unwrap();
                }
            }
            10 if !albums.is_empty() => {
                let album = *rng.pick(&albums);
                match rng.below(3) {
                    0 => {
                        catalog.rename_album(album, rng.pick(WORDS)).unwrap();
                    }
                    1 => albums.push(
                        catalog
                            .duplicate_album(album, rng.pick(WORDS), now())
                            .unwrap()
                            .id,
                    ),
                    _ => {
                        catalog.delete_album(album).unwrap();
                        albums.retain(|a| *a != album);
                    }
                }
            }
            11 if !photos.is_empty() => {
                let victim = photos.swap_remove(rng.below(photos.len()));
                catalog
                    .write(move |c| {
                        Ok(c.execute("DELETE FROM photos WHERE id = ?1", [victim.get()])?)
                    })
                    .unwrap();
            }
            _ => {}
        }
        if step % 8 == 7 {
            let incremental = index_dump(&catalog);
            catalog.rebuild_fts().unwrap();
            assert_eq!(index_dump(&catalog), incremental, "drift after step {step}");
        }
    }

    let incremental = index_dump(&catalog);
    assert!(
        incremental.len() > 50,
        "the sequence produced a non-trivial index ({} terms)",
        incremental.len()
    );
    let queries: Vec<_> = [
        "beach",
        "caption:sunset",
        "tags:family",
        "album_names:winter",
        "camera:canon",
        "café",
        "日本",
    ]
    .iter()
    .map(|q| (*q, hits(&catalog, q)))
    .collect();

    catalog.rebuild_fts().unwrap();
    assert_eq!(
        index_dump(&catalog),
        incremental,
        "incremental maintenance equals a full rebuild"
    );
    for (q, before) in queries {
        assert_eq!(hits(&catalog, q), before, "query {q:?}");
    }
    let rows: i64 = catalog
        .read(|c| Ok(c.query_row("SELECT COUNT(*) FROM photos", [], |r| r.get(0))?))
        .unwrap();
    assert_eq!(
        rows as usize,
        photos.len(),
        "bookkeeping matches the catalog"
    );
}

#[test]
fn rebuild_fts_repairs_a_corrupted_index() {
    let Fixture {
        _dir: _keep,
        catalog,
        ids,
        ..
    } = fixture();
    let expected = index_dump(&catalog);
    catalog
        .write(|c| {
            Ok(c.execute(
                "INSERT INTO photos_fts (photos_fts) VALUES ('delete-all')",
                [],
            )?)
        })
        .unwrap();
    assert!(hits(&catalog, "beach").is_empty());
    catalog.rebuild_fts().unwrap();
    assert_eq!(index_dump(&catalog), expected);
    assert_eq!(hits(&catalog, "beach"), [ids[0]]);
}
