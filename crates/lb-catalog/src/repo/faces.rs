//! Faces, people and face clusters (SPEC §6.15).

use super::{dt_to_db, enum_from_db};
use crate::{
    Catalog,
    error::{CatalogError, Result},
};
use chrono::NaiveDateTime;
use lb_core::{
    Face, FaceId, FaceSource, NormRect, PeopleEntry, PeopleKey, Person, PersonId, PhotoId,
};
use rusqlite::{OptionalExtension, Transaction};
use std::collections::{BTreeMap, BTreeSet};

/// Longest accepted person name in characters.
pub const MAX_PERSON_NAME_CHARS: usize = 100;

/// A face about to be stored.
#[derive(Debug, Clone, PartialEq)]
pub struct NewFace {
    /// The photo it is in.
    pub photo_id: PhotoId,
    /// Box normalized to the upright image.
    pub rect: NormRect,
    /// Detector confidence.
    pub score: f32,
    /// L2-normalized embedding, if one was computed.
    pub embedding: Option<Vec<f32>>,
    /// Cluster the face joins.
    pub cluster_id: Option<i64>,
    /// Person the face belongs to.
    pub person_id: Option<PersonId>,
    /// Where the face came from.
    pub source: FaceSource,
}

/// The embedding of a stored face with its group.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceEmbedding {
    /// The face.
    pub id: FaceId,
    /// Its cluster.
    pub cluster_id: Option<i64>,
    /// Its person.
    pub person_id: Option<PersonId>,
    /// L2-normalized embedding.
    pub embedding: Vec<f32>,
}

const FACE_COLUMNS: &str = "id, photo_id, x, y, w, h, score, cluster_id, person_id, source";

fn face_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Face> {
    let source: String = r.get(9)?;
    Ok(Face {
        id: FaceId::new(r.get(0)?),
        photo_id: PhotoId::new(r.get(1)?),
        rect: NormRect {
            x: r.get(2)?,
            y: r.get(3)?,
            w: r.get(4)?,
            h: r.get(5)?,
        },
        score: r.get(6)?,
        cluster_id: r.get(7)?,
        person_id: r.get::<_, Option<i64>>(8)?.map(PersonId::new),
        source: enum_from_db(9, &source)?,
    })
}

fn person_row(r: &rusqlite::Row<'_>) -> rusqlite::Result<Person> {
    Ok(Person {
        id: PersonId::new(r.get(0)?),
        name: r.get(1)?,
        hidden: r.get::<_, i64>(2)? != 0,
    })
}

fn embedding_to_blob(e: &[f32]) -> Vec<u8> {
    e.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn blob_to_embedding(b: &[u8]) -> Vec<f32> {
    b.as_chunks::<4>()
        .0
        .iter()
        .map(|c| f32::from_le_bytes(*c))
        .collect()
}

fn normalize_person_name(raw: &str) -> Result<String> {
    let name = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() || name.chars().count() > MAX_PERSON_NAME_CHARS {
        return Err(CatalogError::InvalidPersonName);
    }
    Ok(name)
}

fn find_person(tx: &Transaction<'_>, name: &str) -> Result<Option<Person>> {
    if let Some(p) = tx
        .query_row(
            "SELECT id, name, hidden FROM people WHERE name = ?1 COLLATE NOCASE",
            [name],
            person_row,
        )
        .optional()?
    {
        return Ok(Some(p));
    }
    if name.is_ascii() {
        return Ok(None);
    }
    let wanted = name.to_lowercase();
    let mut stmt = tx.prepare_cached("SELECT id, name, hidden FROM people")?;
    for p in stmt.query_map([], person_row)? {
        let p = p?;
        if p.name.to_lowercase() == wanted {
            return Ok(Some(p));
        }
    }
    Ok(None)
}

fn get_or_create_person(tx: &Transaction<'_>, raw: &str) -> Result<Person> {
    let name = normalize_person_name(raw)?;
    if let Some(p) = find_person(tx, &name)? {
        return Ok(p);
    }
    tx.execute("INSERT INTO people (name) VALUES (?1)", [&name])?;
    Ok(Person {
        id: PersonId::new(tx.last_insert_rowid()),
        name,
        hidden: false,
    })
}

fn person_exists(tx: &Transaction<'_>, id: PersonId) -> Result<bool> {
    Ok(tx
        .query_row("SELECT 1 FROM people WHERE id = ?1", [id.get()], |r| {
            r.get::<_, i64>(0)
        })
        .optional()?
        .is_some())
}

fn photos_where(tx: &Transaction<'_>, clause: &str, value: i64) -> Result<Vec<PhotoId>> {
    let mut stmt = tx.prepare(&format!(
        "SELECT DISTINCT photo_id FROM faces WHERE {clause} ORDER BY photo_id"
    ))?;
    Ok(stmt
        .query_map([value], |r| Ok(PhotoId::new(r.get(0)?)))?
        .collect::<rusqlite::Result<_>>()?)
}

fn photo_of_face(tx: &Transaction<'_>, id: FaceId) -> Result<PhotoId> {
    tx.query_row(
        "SELECT photo_id FROM faces WHERE id = ?1",
        [id.get()],
        |r| Ok(PhotoId::new(r.get(0)?)),
    )
    .optional()?
    .ok_or(CatalogError::NotFound("face"))
}

fn next_cluster_id(tx: &Transaction<'_>) -> Result<i64> {
    Ok(tx.query_row(
        "SELECT COALESCE(MAX(cluster_id), 0) + 1 FROM faces",
        [],
        |r| r.get(0),
    )?)
}

fn insert_face(tx: &Transaction<'_>, f: &NewFace) -> Result<FaceId> {
    tx.execute(
        "INSERT INTO faces (photo_id, x, y, w, h, score, embedding, cluster_id, person_id, source)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        rusqlite::params![
            f.photo_id.get(),
            f.rect.x,
            f.rect.y,
            f.rect.w,
            f.rect.h,
            f.score,
            f.embedding.as_deref().map(embedding_to_blob),
            f.cluster_id,
            f.person_id.map(PersonId::get),
            f.source.as_str(),
        ],
    )?;
    Ok(FaceId::new(tx.last_insert_rowid()))
}

impl Catalog {
    /// Readable images that have not been through face detection yet, ascending by id.
    pub fn photos_without_face_scan(&self) -> Result<Vec<PhotoId>> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT p.id FROM photos p
                 WHERE p.media_type = 'image' AND p.status = 'ok'
                   AND NOT EXISTS (SELECT 1 FROM face_scans s WHERE s.photo_id = p.id)
                 ORDER BY p.id",
            )?;
            Ok(stmt
                .query_map([], |r| Ok(PhotoId::new(r.get(0)?)))?
                .collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Stores the result of detecting faces in `photo`: previous detected faces are replaced
    /// (manual and imported faces stay) and the photo is marked as scanned. Returns the new ids.
    pub fn record_face_scan(
        &self,
        photo: PhotoId,
        faces: Vec<NewFace>,
        now: NaiveDateTime,
    ) -> Result<Vec<FaceId>> {
        self.write_tx(move |tx| {
            tx.execute(
                "DELETE FROM faces WHERE photo_id = ?1 AND source = 'detected'",
                [photo.get()],
            )?;
            let mut ids = Vec::with_capacity(faces.len());
            for face in &faces {
                let mut face = face.clone();
                if let (None, Some(cluster)) = (face.person_id, face.cluster_id) {
                    face.person_id = tx
                        .query_row(
                            "SELECT MIN(person_id) FROM faces WHERE cluster_id = ?1",
                            [cluster],
                            |r| r.get::<_, Option<i64>>(0),
                        )?
                        .map(PersonId::new);
                }
                ids.push(insert_face(tx, &face)?);
            }
            tx.execute(
                "INSERT OR REPLACE INTO face_scans (photo_id, scanned_at) VALUES (?1, ?2)",
                (photo.get(), dt_to_db(now)),
            )?;
            crate::fts::refresh_photos(tx, &[photo])?;
            Ok(ids)
        })
    }

    /// Forgets every detected face and scan marker so photos are analysed again. Named people
    /// stay; manual and imported faces are kept.
    pub fn reset_face_detection(&self) -> Result<usize> {
        self.write_tx(|tx| {
            let affected: Vec<PhotoId> = {
                let mut stmt =
                    tx.prepare("SELECT DISTINCT photo_id FROM faces WHERE source = 'detected'")?;
                stmt.query_map([], |r| Ok(PhotoId::new(r.get(0)?)))?
                    .collect::<rusqlite::Result<_>>()?
            };
            let removed = tx.execute("DELETE FROM faces WHERE source = 'detected'", [])?;
            tx.execute("DELETE FROM face_scans", [])?;
            crate::fts::refresh_photos(tx, &affected)?;
            Ok(removed)
        })
    }

    /// Adds a face drawn by the user (SPEC §6.15). With `person` the face belongs to them.
    pub fn add_manual_face(
        &self,
        photo: PhotoId,
        rect: NormRect,
        person: Option<PersonId>,
    ) -> Result<FaceId> {
        self.write_tx(move |tx| {
            if let Some(p) = person
                && !person_exists(tx, p)?
            {
                return Err(CatalogError::NotFound("person"));
            }
            let exists = tx
                .query_row("SELECT 1 FROM photos WHERE id = ?1", [photo.get()], |r| {
                    r.get::<_, i64>(0)
                })
                .optional()?;
            if exists.is_none() {
                return Err(CatalogError::NotFound("photo"));
            }
            let id = insert_face(
                tx,
                &NewFace {
                    photo_id: photo,
                    rect,
                    score: 1.0,
                    embedding: None,
                    cluster_id: None,
                    person_id: person,
                    source: FaceSource::Manual,
                },
            )?;
            crate::fts::refresh_photos(tx, &[photo])?;
            Ok(id)
        })
    }

    /// Adds a face imported from a `.picasa.ini`, optionally belonging to `person`.
    pub fn add_picasa_face(
        &self,
        photo: PhotoId,
        rect: NormRect,
        person: Option<PersonId>,
    ) -> Result<FaceId> {
        self.write_tx(move |tx| {
            if let Some(p) = person
                && !person_exists(tx, p)?
            {
                return Err(CatalogError::NotFound("person"));
            }
            let id = insert_face(
                tx,
                &NewFace {
                    photo_id: photo,
                    rect,
                    score: 1.0,
                    embedding: None,
                    cluster_id: None,
                    person_id: person,
                    source: FaceSource::Picasa,
                },
            )?;
            crate::fts::refresh_photos(tx, &[photo])?;
            Ok(id)
        })
    }

    /// Deletes the faces of `photo` that were imported from a `.picasa.ini`.
    pub fn remove_picasa_faces(&self, photo: PhotoId) -> Result<usize> {
        self.write_tx(move |tx| {
            let n = tx.execute(
                "DELETE FROM faces WHERE photo_id = ?1 AND source = 'picasa'",
                [photo.get()],
            )?;
            if n > 0 {
                crate::fts::refresh_photos(tx, &[photo])?;
            }
            Ok(n)
        })
    }

    /// Faces in `photo`, left to right.
    pub fn faces_of_photo(&self, photo: PhotoId) -> Result<Vec<Face>> {
        self.read(|c| {
            let mut stmt = c.prepare_cached(&format!(
                "SELECT {FACE_COLUMNS} FROM faces WHERE photo_id = ?1 ORDER BY x, id"
            ))?;
            Ok(stmt
                .query_map([photo.get()], face_row)?
                .collect::<rusqlite::Result<_>>()?)
        })
    }

    /// One face.
    pub fn get_face(&self, id: FaceId) -> Result<Option<Face>> {
        self.read(|c| {
            Ok(c.query_row(
                &format!("SELECT {FACE_COLUMNS} FROM faces WHERE id = ?1"),
                [id.get()],
                face_row,
            )
            .optional()?)
        })
    }

    /// Every stored embedding with its cluster and person, ascending by face id.
    pub fn face_embeddings(&self) -> Result<Vec<FaceEmbedding>> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT id, cluster_id, person_id, embedding FROM faces
                 WHERE embedding IS NOT NULL ORDER BY id",
            )?;
            Ok(stmt
                .query_map([], |r| {
                    let blob: Vec<u8> = r.get(3)?;
                    Ok(FaceEmbedding {
                        id: FaceId::new(r.get(0)?),
                        cluster_id: r.get(1)?,
                        person_id: r.get::<_, Option<i64>>(2)?.map(PersonId::new),
                        embedding: blob_to_embedding(&blob),
                    })
                })?
                .collect::<rusqlite::Result<_>>()?)
        })
    }

    /// The highest cluster id in use (0 when there are none).
    pub fn max_cluster_id(&self) -> Result<i64> {
        self.read(|c| {
            Ok(
                c.query_row("SELECT COALESCE(MAX(cluster_id), 0) FROM faces", [], |r| {
                    r.get(0)
                })?,
            )
        })
    }

    /// Reassigns clusters of faces in bulk, as produced by a full re-clustering. Faces that move
    /// into a cluster with a person take that person; people whose faces were all moved away keep
    /// no automatic assignment.
    pub fn set_face_clusters(&self, assignments: Vec<(FaceId, i64)>) -> Result<()> {
        self.write_tx(move |tx| {
            let mut stmt = tx.prepare_cached("UPDATE faces SET cluster_id = ?2 WHERE id = ?1")?;
            for (face, cluster) in &assignments {
                stmt.execute((face.get(), cluster))?;
            }
            Ok(())
        })
    }

    /// Gives every face of `cluster` to the person called `name`, creating them if needed.
    pub fn name_cluster(&self, cluster: i64, name: &str) -> Result<Person> {
        let name = name.to_owned();
        self.write_tx(move |tx| {
            let person = get_or_create_person(tx, &name)?;
            let photos = photos_where(tx, "cluster_id = ?1", cluster)?;
            if photos.is_empty() {
                return Err(CatalogError::NotFound("cluster"));
            }
            tx.execute(
                "UPDATE faces SET person_id = ?2 WHERE cluster_id = ?1",
                (cluster, person.id.get()),
            )?;
            crate::fts::refresh_photos(tx, &photos)?;
            Ok(person)
        })
    }

    /// Makes `face` (and only it) belong to `person`.
    pub fn assign_face_to_person(&self, face: FaceId, person: PersonId) -> Result<()> {
        self.write_tx(move |tx| {
            if !person_exists(tx, person)? {
                return Err(CatalogError::NotFound("person"));
            }
            let photo = photo_of_face(tx, face)?;
            tx.execute(
                "UPDATE faces SET person_id = ?2 WHERE id = ?1",
                (face.get(), person.get()),
            )?;
            crate::fts::refresh_photos(tx, &[photo])?;
            Ok(())
        })
    }

    /// Takes `face` away from its person and puts it in a cluster of its own so that it is not
    /// matched to them again ("not this person").
    pub fn remove_face_from_person(&self, face: FaceId) -> Result<()> {
        self.write_tx(move |tx| {
            let photo = photo_of_face(tx, face)?;
            let cluster = next_cluster_id(tx)?;
            tx.execute(
                "UPDATE faces SET person_id = NULL, cluster_id = ?2 WHERE id = ?1",
                (face.get(), cluster),
            )?;
            crate::fts::refresh_photos(tx, &[photo])?;
            Ok(())
        })
    }

    /// Deletes a face box (a false detection).
    pub fn delete_face(&self, face: FaceId) -> Result<()> {
        self.write_tx(move |tx| {
            let photo = photo_of_face(tx, face)?;
            tx.execute("DELETE FROM faces WHERE id = ?1", [face.get()])?;
            crate::fts::refresh_photos(tx, &[photo])?;
            Ok(())
        })
    }

    /// Moves all faces and clusters of `from` to `into` and deletes `from`.
    pub fn merge_people(&self, from: PersonId, into: PersonId) -> Result<()> {
        self.write_tx(move |tx| {
            if !person_exists(tx, from)? {
                return Err(CatalogError::NotFound("person"));
            }
            if !person_exists(tx, into)? {
                return Err(CatalogError::NotFound("person"));
            }
            if from == into {
                return Ok(());
            }
            merge(tx, from, into)
        })
    }

    /// Renames a person. When another person already has the name (case-insensitively) the two
    /// are merged into that one; returns the surviving person.
    pub fn rename_person(&self, id: PersonId, name: &str) -> Result<Person> {
        let name = normalize_person_name(name)?;
        self.write_tx(move |tx| {
            if !person_exists(tx, id)? {
                return Err(CatalogError::NotFound("person"));
            }
            let photos = photos_where(tx, "person_id = ?1", id.get())?;
            let survivor = match find_person(tx, &name)? {
                Some(target) if target.id != id => {
                    merge(tx, id, target.id)?;
                    target
                }
                _ => {
                    tx.execute(
                        "UPDATE people SET name = ?2 WHERE id = ?1",
                        (id.get(), &name),
                    )?;
                    Person {
                        id,
                        name,
                        hidden: tx.query_row(
                            "SELECT hidden FROM people WHERE id = ?1",
                            [id.get()],
                            |r| Ok(r.get::<_, i64>(0)? != 0),
                        )?,
                    }
                }
            };
            crate::fts::refresh_photos(tx, &photos)?;
            Ok(survivor)
        })
    }

    /// Hides a person from (or shows them in) the People list.
    pub fn set_person_hidden(&self, id: PersonId, hidden: bool) -> Result<()> {
        self.write_tx(move |tx| {
            let n = tx.execute(
                "UPDATE people SET hidden = ?2 WHERE id = ?1",
                (id.get(), i64::from(hidden)),
            )?;
            if n == 0 {
                return Err(CatalogError::NotFound("person"));
            }
            Ok(())
        })
    }

    /// Deletes a person; their faces stay and become unnamed.
    pub fn delete_person(&self, id: PersonId) -> Result<()> {
        self.write_tx(move |tx| {
            let photos = photos_where(tx, "person_id = ?1", id.get())?;
            let n = tx.execute("DELETE FROM people WHERE id = ?1", [id.get()])?;
            if n == 0 {
                return Err(CatalogError::NotFound("person"));
            }
            crate::fts::refresh_photos(tx, &photos)?;
            Ok(())
        })
    }

    /// One person.
    pub fn get_person(&self, id: PersonId) -> Result<Option<Person>> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT id, name, hidden FROM people WHERE id = ?1",
                [id.get()],
                person_row,
            )
            .optional()?)
        })
    }

    /// The person called `name` (case-insensitive), if any.
    pub fn find_person(&self, name: &str) -> Result<Option<Person>> {
        let name = normalize_person_name(name)?;
        self.write_tx(move |tx| find_person(tx, &name))
    }

    /// Finds or creates the person called `name`.
    pub fn get_or_create_person(&self, name: &str) -> Result<Person> {
        let name = name.to_owned();
        self.write_tx(move |tx| get_or_create_person(tx, &name))
    }

    /// Every named person, ordered by name.
    pub fn list_persons(&self) -> Result<Vec<Person>> {
        self.read(|c| {
            let mut stmt =
                c.prepare("SELECT id, name, hidden FROM people ORDER BY name COLLATE NOCASE, id")?;
            Ok(stmt
                .query_map([], person_row)?
                .collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Faces grouped by person or unnamed cluster: unnamed groups first (largest first), then
    /// named people by name. Hidden people are listed only with `include_hidden`.
    pub fn list_people(&self, include_hidden: bool) -> Result<Vec<PeopleEntry>> {
        let persons: BTreeMap<PersonId, Person> = self
            .list_persons()?
            .into_iter()
            .map(|p| (p.id, p))
            .collect();
        let faces: Vec<Face> = self.read(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {FACE_COLUMNS} FROM faces
                 WHERE person_id IS NOT NULL OR cluster_id IS NOT NULL ORDER BY id"
            ))?;
            Ok(stmt
                .query_map([], face_row)?
                .collect::<rusqlite::Result<_>>()?)
        })?;
        let mut groups: BTreeMap<PeopleKey, Vec<Face>> = BTreeMap::new();
        for face in faces {
            let key = match (face.person_id, face.cluster_id) {
                (Some(p), _) => PeopleKey::Person(p),
                (None, Some(c)) => PeopleKey::Cluster(c),
                (None, None) => continue,
            };
            groups.entry(key).or_default().push(face);
        }
        let mut entries: Vec<PeopleEntry> = groups
            .into_iter()
            .filter_map(|(key, faces)| {
                let person = match key {
                    PeopleKey::Person(id) => Some(persons.get(&id)?),
                    PeopleKey::Cluster(_) => None,
                };
                let hidden = person.is_some_and(|p| p.hidden);
                if hidden && !include_hidden {
                    return None;
                }
                let cover = *faces
                    .iter()
                    .max_by(|a, b| a.score.total_cmp(&b.score).then_with(|| b.id.cmp(&a.id)))?;
                let photos: BTreeSet<PhotoId> = faces.iter().map(|f| f.photo_id).collect();
                Some(PeopleEntry {
                    key,
                    name: person.map(|p| p.name.clone()),
                    hidden,
                    face_count: faces.len(),
                    photo_count: photos.len(),
                    cover,
                })
            })
            .collect();
        entries.sort_by(|a, b| match (&a.name, &b.name) {
            (None, None) => b.face_count.cmp(&a.face_count).then(a.key.cmp(&b.key)),
            (None, Some(_)) => std::cmp::Ordering::Less,
            (Some(_), None) => std::cmp::Ordering::Greater,
            (Some(x), Some(y)) => x
                .to_lowercase()
                .cmp(&y.to_lowercase())
                .then(a.key.cmp(&b.key)),
        });
        Ok(entries)
    }

    /// Faces of a person or cluster, best score first.
    pub fn faces_of(&self, key: PeopleKey) -> Result<Vec<Face>> {
        let (clause, value) = match key {
            PeopleKey::Person(id) => ("person_id = ?1", id.get()),
            PeopleKey::Cluster(c) => ("person_id IS NULL AND cluster_id = ?1", c),
        };
        self.read(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT {FACE_COLUMNS} FROM faces WHERE {clause} ORDER BY score DESC, id"
            ))?;
            Ok(stmt
                .query_map([value], face_row)?
                .collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Photos containing the faces of a person or cluster.
    pub fn photos_of(&self, key: PeopleKey) -> Result<Vec<PhotoId>> {
        let (clause, value) = match key {
            PeopleKey::Person(id) => ("person_id = ?1", id.get()),
            PeopleKey::Cluster(c) => ("person_id IS NULL AND cluster_id = ?1", c),
        };
        self.read(|c| {
            let mut stmt = c.prepare(&format!(
                "SELECT DISTINCT photo_id FROM faces WHERE {clause} ORDER BY photo_id"
            ))?;
            Ok(stmt
                .query_map([value], |r| Ok(PhotoId::new(r.get(0)?)))?
                .collect::<rusqlite::Result<_>>()?)
        })
    }
}

fn merge(tx: &Transaction<'_>, from: PersonId, into: PersonId) -> Result<()> {
    let photos = photos_where(tx, "person_id = ?1", from.get())?;
    let target_cluster: Option<i64> = tx.query_row(
        "SELECT MIN(cluster_id) FROM faces WHERE person_id = ?1",
        [into.get()],
        |r| r.get(0),
    )?;
    if let Some(target) = target_cluster {
        tx.execute(
            "UPDATE faces SET cluster_id = ?3 WHERE cluster_id IN
               (SELECT DISTINCT cluster_id FROM faces WHERE person_id = ?1)
             OR (person_id = ?1 AND cluster_id IS NOT NULL) OR (person_id = ?2)",
            (from.get(), into.get(), target),
        )?;
    }
    tx.execute(
        "UPDATE faces SET person_id = ?2 WHERE person_id = ?1",
        (from.get(), into.get()),
    )?;
    tx.execute("DELETE FROM people WHERE id = ?1", [from.get()])?;
    crate::fts::refresh_photos(tx, &photos)?;
    Ok(())
}
