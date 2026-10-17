//! Photo rows: batch upsert and lookups (SPEC §5.2 `photos`, §6.2.1).

use super::{dt_from_db, dt_to_db, enum_from_db, int_from_db, path_str, placeholders};
use crate::{Catalog, error::Result};
use chrono::NaiveDateTime;
use lb_core::{FolderId, Format, GeoPoint, MediaType, Photo, PhotoId, PhotoStatus, TakenAtSource};
use rusqlite::{Row, named_params};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

/// Scanner-managed columns of a photo row.
///
/// User-managed columns (`starred`, `rating`, `hidden`, `caption`, `edit_version`) are never
/// overwritten by an upsert, except that `import_caption` fills an empty caption.
#[derive(Debug, Clone, PartialEq)]
pub struct PhotoUpsert {
    /// Containing folder.
    pub folder_id: FolderId,
    /// File name within the folder (the upsert key together with `folder_id`).
    pub file_name: String,
    /// Image or video.
    pub media_type: MediaType,
    /// File format.
    pub format: Format,
    /// File size in bytes.
    pub file_size: u64,
    /// File modification time, unix seconds.
    pub file_mtime: i64,
    /// `blake3(size || head || tail)` hex fingerprint.
    pub quick_key: String,
    /// Width after EXIF orientation.
    pub width: Option<u32>,
    /// Height after EXIF orientation.
    pub height: Option<u32>,
    /// EXIF orientation of the stored file, 1..=8.
    pub orientation: u8,
    /// Capture time (naive local).
    pub taken_at: Option<NaiveDateTime>,
    /// Which rule produced `taken_at`.
    pub taken_at_src: Option<TakenAtSource>,
    /// EXIF `Make`.
    pub camera_make: Option<String>,
    /// EXIF `Model`.
    pub camera_model: Option<String>,
    /// EXIF `LensModel`.
    pub lens: Option<String>,
    /// ISO speed.
    pub iso: Option<u32>,
    /// Aperture f-number.
    pub f_number: Option<f64>,
    /// Exposure time in seconds.
    pub exposure_s: Option<f64>,
    /// Focal length in millimetres.
    pub focal_mm: Option<f64>,
    /// GPS latitude/longitude.
    pub gps: Option<GeoPoint>,
    /// GPS altitude in metres.
    pub gps_alt: Option<f64>,
    /// Video duration in milliseconds.
    pub duration_ms: Option<u64>,
    /// Caption found in file metadata; applied only while the stored caption is empty.
    pub import_caption: Option<String>,
    /// Indexing state.
    pub status: PhotoStatus,
    /// Why the file is unreadable.
    pub error: Option<String>,
}

impl PhotoUpsert {
    /// A readable photo with only the mandatory fields set; everything else is unknown.
    pub fn new(
        folder_id: FolderId,
        file_name: impl Into<String>,
        format: Format,
        file_size: u64,
        file_mtime: i64,
        quick_key: impl Into<String>,
    ) -> Self {
        Self {
            folder_id,
            file_name: file_name.into(),
            media_type: format.media_type(),
            format,
            file_size,
            file_mtime,
            quick_key: quick_key.into(),
            width: None,
            height: None,
            orientation: 1,
            taken_at: None,
            taken_at_src: None,
            camera_make: None,
            camera_model: None,
            lens: None,
            iso: None,
            f_number: None,
            exposure_s: None,
            focal_mm: None,
            gps: None,
            gps_alt: None,
            duration_ms: None,
            import_caption: None,
            status: PhotoStatus::Ok,
            error: None,
        }
    }
}

/// Result of upserting one row.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UpsertOutcome {
    /// Row id (stable across updates).
    pub id: PhotoId,
    /// `true` when the row was newly inserted, `false` when an existing row was updated.
    pub inserted: bool,
}

const UPSERT: &str = "
INSERT INTO photos (folder_id, file_name, media_type, format, file_size, file_mtime, quick_key,
    width, height, orientation, taken_at, taken_at_src, camera_make, camera_model, lens, iso,
    f_number, exposure_s, focal_mm, gps_lat, gps_lon, gps_alt, duration_ms, caption, status,
    error, indexed_at)
VALUES (:folder_id, :file_name, :media_type, :format, :file_size, :file_mtime, :quick_key,
    :width, :height, :orientation, :taken_at, :taken_at_src, :camera_make, :camera_model, :lens,
    :iso, :f_number, :exposure_s, :focal_mm, :gps_lat, :gps_lon, :gps_alt, :duration_ms,
    :caption, :status, :error, :indexed_at)
ON CONFLICT(folder_id, file_name) DO UPDATE SET
    media_type = excluded.media_type, format = excluded.format, file_size = excluded.file_size,
    file_mtime = excluded.file_mtime,
    content_hash = CASE WHEN photos.quick_key = excluded.quick_key THEN photos.content_hash END,
    phash = CASE WHEN photos.quick_key = excluded.quick_key THEN photos.phash END,
    quick_key = excluded.quick_key, width = excluded.width, height = excluded.height,
    orientation = excluded.orientation, taken_at = excluded.taken_at,
    taken_at_src = excluded.taken_at_src, camera_make = excluded.camera_make,
    camera_model = excluded.camera_model, lens = excluded.lens, iso = excluded.iso,
    f_number = excluded.f_number, exposure_s = excluded.exposure_s,
    focal_mm = excluded.focal_mm, gps_lat = excluded.gps_lat, gps_lon = excluded.gps_lon,
    gps_alt = excluded.gps_alt, duration_ms = excluded.duration_ms,
    caption = CASE WHEN photos.caption IS NULL OR photos.caption = ''
                   THEN excluded.caption ELSE photos.caption END,
    status = excluded.status, error = excluded.error, indexed_at = excluded.indexed_at
RETURNING id";

const SELECT: &str = "
SELECT p.id, p.folder_id, f.path, p.file_name, p.media_type, p.format, p.file_size, p.file_mtime,
       p.quick_key, p.content_hash, p.orientation, p.width, p.height, p.taken_at, p.taken_at_src,
       p.camera_make, p.camera_model, p.gps_lat, p.gps_lon, p.caption, p.starred, p.rating,
       p.hidden, p.edit_version, p.status, p.error, p.duration_ms
FROM photos p JOIN folders f ON f.id = p.folder_id";

/// Folder path hits `folders.path` (UNIQUE); the file name hits `photos(folder_id, file_name)`.
const BY_PATH: &str = "
SELECT p.id, p.folder_id, f.path, p.file_name, p.media_type, p.format, p.file_size, p.file_mtime,
       p.quick_key, p.content_hash, p.orientation, p.width, p.height, p.taken_at, p.taken_at_src,
       p.camera_make, p.camera_model, p.gps_lat, p.gps_lon, p.caption, p.starred, p.rating,
       p.hidden, p.edit_version, p.status, p.error, p.duration_ms
FROM folders f
JOIN photos p ON p.folder_id = f.id AND p.file_name = ?2
WHERE f.path = ?1";

/// Rows per `IN (...)` query; keeps well under SQLite's variable limit.
const CHUNK: usize = 500;

fn join_camera(make: Option<String>, model: Option<String>) -> Option<String> {
    let make = make.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
    let model = model.map(|s| s.trim().to_owned()).filter(|s| !s.is_empty());
    match (make, model) {
        (Some(make), Some(model)) if model.to_lowercase().starts_with(&make.to_lowercase()) => {
            Some(model)
        }
        (Some(make), Some(model)) => Some(format!("{make} {model}")),
        (Some(one), None) | (None, Some(one)) => Some(one),
        (None, None) => None,
    }
}

fn from_row(r: &Row<'_>) -> rusqlite::Result<Photo> {
    let folder_path: String = r.get(2)?;
    let file_name: String = r.get(3)?;
    let taken_at = r
        .get::<_, Option<String>>(13)?
        .map(|s| dt_from_db(13, &s))
        .transpose()?;
    let taken_at_src = r
        .get::<_, Option<String>>(14)?
        .map(|s| enum_from_db(14, &s))
        .transpose()?;
    let gps = match (r.get::<_, Option<f64>>(17)?, r.get::<_, Option<f64>>(18)?) {
        (Some(lat), Some(lon)) => Some(GeoPoint { lat, lon }),
        _ => None,
    };
    Ok(Photo {
        id: PhotoId::new(r.get(0)?),
        folder_id: FolderId::new(r.get(1)?),
        path: PathBuf::from(folder_path).join(file_name),
        media_type: enum_from_db(4, &r.get::<_, String>(4)?)?,
        format: enum_from_db(5, &r.get::<_, String>(5)?)?,
        size: int_from_db(6, r.get(6)?)?,
        file_mtime: r.get(7)?,
        quick_key: r.get(8)?,
        content_hash: r.get(9)?,
        orientation: int_from_db(10, r.get(10)?)?,
        width: r
            .get::<_, Option<i64>>(11)?
            .map(|v| int_from_db(11, v))
            .transpose()?,
        height: r
            .get::<_, Option<i64>>(12)?
            .map(|v| int_from_db(12, v))
            .transpose()?,
        taken_at,
        taken_at_src,
        camera: join_camera(r.get(15)?, r.get(16)?),
        gps,
        duration_ms: r
            .get::<_, Option<i64>>(26)?
            .map(|v| int_from_db(26, v))
            .transpose()?,
        caption: r.get(19)?,
        starred: r.get(20)?,
        rating: int_from_db(21, r.get(21)?)?,
        hidden: r.get(22)?,
        edit_version: int_from_db(23, r.get(23)?)?,
        status: enum_from_db(24, &r.get::<_, String>(24)?)?,
        error: r.get(25)?,
    })
}

/// Stored hashes of one photo.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PhotoHashes {
    /// The photo.
    pub id: PhotoId,
    /// Full-file blake3 hash, when computed.
    pub content_hash: Option<String>,
    /// 64-bit perceptual hash, when computed.
    pub phash: Option<u64>,
}

impl Catalog {
    /// Inserts or updates `rows` in one transaction (SPEC §6.2.1 step 4) and returns one outcome
    /// per input row, in order. `now` becomes `indexed_at`.
    ///
    /// Updating changes only scanner-managed columns; the stored `content_hash`/`phash` are
    /// cleared when the `quick_key` changed.
    pub fn upsert_photos(
        &self,
        rows: Vec<PhotoUpsert>,
        now: NaiveDateTime,
    ) -> Result<Vec<UpsertOutcome>> {
        self.write_tx(move |tx| {
            let indexed_at = dt_to_db(now);
            let mut exists = tx.prepare_cached("SELECT 1 FROM photos WHERE folder_id = ?1 AND file_name = ?2")?;
            let mut upsert = tx.prepare_cached(UPSERT)?;
            let mut out = Vec::with_capacity(rows.len());
            for row in &rows {
                let inserted = !exists.exists((row.folder_id.get(), &row.file_name))?;
                let id: i64 = upsert.query_row(
                    named_params! {
                        ":folder_id": row.folder_id.get(),
                        ":file_name": row.file_name,
                        ":media_type": row.media_type.as_str(),
                        ":format": row.format.as_str(),
                        ":file_size": i64::try_from(row.file_size).unwrap_or(i64::MAX),
                        ":file_mtime": row.file_mtime,
                        ":quick_key": row.quick_key,
                        ":width": row.width,
                        ":height": row.height,
                        ":orientation": row.orientation,
                        ":taken_at": row.taken_at.map(dt_to_db),
                        ":taken_at_src": row.taken_at_src.map(TakenAtSource::as_str),
                        ":camera_make": row.camera_make,
                        ":camera_model": row.camera_model,
                        ":lens": row.lens,
                        ":iso": row.iso,
                        ":f_number": row.f_number,
                        ":exposure_s": row.exposure_s,
                        ":focal_mm": row.focal_mm,
                        ":gps_lat": row.gps.map(|g| g.lat),
                        ":gps_lon": row.gps.map(|g| g.lon),
                        ":gps_alt": row.gps_alt,
                        ":duration_ms": row.duration_ms.map(|v| i64::try_from(v).unwrap_or(i64::MAX)),
                        ":caption": row.import_caption,
                        ":status": row.status.as_str(),
                        ":error": row.error,
                        ":indexed_at": indexed_at,
                    },
                    |r| r.get(0),
                )?;
                out.push(UpsertOutcome { id: PhotoId::new(id), inserted });
            }
            let ids: Vec<PhotoId> = out.iter().map(|o| o.id).collect();
            crate::fts::refresh_photos(tx, &ids)?;
            Ok(out)
        })
    }

    /// Sets (or clears with `None`) the caption of every photo in `ids` in one transaction and
    /// updates the search index. Returns the number of photos changed. The caption is stored
    /// verbatim; length and whitespace rules belong to the caller (SPEC §6.7.1).
    pub fn set_caption(&self, ids: &[PhotoId], caption: Option<String>) -> Result<usize> {
        let ids = ids.to_vec();
        self.write_tx(move |tx| {
            let mut changed = 0;
            let mut update = tx.prepare_cached("UPDATE photos SET caption = ?1 WHERE id = ?2")?;
            for id in &ids {
                changed += update.execute((&caption, id.get()))?;
            }
            drop(update);
            crate::fts::refresh_photos(tx, &ids)?;
            Ok(changed)
        })
    }

    /// Sets the starred flag of every photo in `ids` in one transaction; returns rows changed.
    pub fn set_starred(&self, ids: &[PhotoId], starred: bool) -> Result<usize> {
        self.set_flag("starred", ids, i64::from(starred))
    }

    /// Sets the hidden flag of every photo in `ids` in one transaction; returns rows changed.
    pub fn set_hidden(&self, ids: &[PhotoId], hidden: bool) -> Result<usize> {
        self.set_flag("hidden", ids, i64::from(hidden))
    }

    /// Sets (or with `None` clears) the position of every photo in `ids` in one transaction;
    /// returns rows changed. The altitude is dropped because it described the old position.
    pub fn set_gps(&self, ids: &[PhotoId], gps: Option<GeoPoint>) -> Result<usize> {
        let ids = ids.to_vec();
        self.write_tx(move |tx| {
            let mut stmt = tx.prepare_cached(
                "UPDATE photos SET gps_lat = ?1, gps_lon = ?2, gps_alt = NULL WHERE id = ?3",
            )?;
            let mut changed = 0;
            for id in &ids {
                changed += stmt.execute((gps.map(|g| g.lat), gps.map(|g| g.lon), id.get()))?;
            }
            Ok(changed)
        })
    }

    /// Sets the rating (0..=5) of every photo in `ids` in one transaction. A value above 5
    /// violates the schema CHECK and fails the whole batch; returns rows changed.
    pub fn set_rating(&self, ids: &[PhotoId], rating: u8) -> Result<usize> {
        self.set_flag("rating", ids, i64::from(rating))
    }

    fn set_flag(&self, column: &'static str, ids: &[PhotoId], value: i64) -> Result<usize> {
        let ids = ids.to_vec();
        self.write_tx(move |tx| {
            let mut stmt =
                tx.prepare_cached(&format!("UPDATE photos SET {column} = ?1 WHERE id = ?2"))?;
            let mut changed = 0;
            for id in &ids {
                changed += stmt.execute((value, id.get()))?;
            }
            Ok(changed)
        })
    }

    /// Fetches photos by id. The result follows the order of `ids` (duplicates repeated); ids
    /// that do not exist are skipped.
    pub fn get_many(&self, ids: &[PhotoId]) -> Result<Vec<Photo>> {
        let mut found: HashMap<PhotoId, Photo> = HashMap::with_capacity(ids.len());
        self.read(|c| {
            for chunk in ids.chunks(CHUNK) {
                let sql = format!("{SELECT} WHERE p.id IN ({})", placeholders(chunk.len()));
                let mut stmt = c.prepare_cached(&sql)?;
                let rows = stmt.query_map(
                    rusqlite::params_from_iter(chunk.iter().map(|i| i.get())),
                    from_row,
                )?;
                for photo in rows {
                    let photo = photo?;
                    found.insert(photo.id, photo);
                }
            }
            Ok(())
        })?;
        Ok(ids.iter().filter_map(|id| found.get(id).cloned()).collect())
    }

    /// Stores the full-file hash of a photo (SPEC §6.9, §6.11).
    pub fn set_content_hash(&self, id: PhotoId, hash: &str) -> Result<()> {
        let hash = hash.to_owned();
        self.write(move |c| {
            c.execute(
                "UPDATE photos SET content_hash = ?1 WHERE id = ?2",
                (hash, id.get()),
            )?;
            Ok(())
        })
    }

    /// Stores the 64-bit perceptual hash of a photo (SPEC §6.11).
    pub fn set_phash(&self, id: PhotoId, phash: u64) -> Result<()> {
        self.write(move |c| {
            c.execute(
                "UPDATE photos SET phash = ?1 WHERE id = ?2",
                (i64::from_ne_bytes(phash.to_ne_bytes()), id.get()),
            )?;
            Ok(())
        })
    }

    /// Stored hashes of every readable image (videos and unreadable or missing files excluded), ordered
    /// by id; hashes not computed yet are `None`.
    pub fn photo_hashes(&self) -> Result<Vec<PhotoHashes>> {
        self.read(|c| {
            let mut stmt = c.prepare_cached(
                "SELECT id, content_hash, phash FROM photos
                 WHERE status = 'ok' AND media_type = 'image' ORDER BY id",
            )?;
            Ok(stmt
                .query_map([], |r| {
                    let phash: Option<i64> = r.get(2)?;
                    Ok(PhotoHashes {
                        id: PhotoId::new(r.get(0)?),
                        content_hash: r.get(1)?,
                        phash: phash.map(|v| u64::from_ne_bytes(v.to_ne_bytes())),
                    })
                })?
                .collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Fetches one photo by id.
    pub fn get_photo(&self, id: PhotoId) -> Result<Option<Photo>> {
        Ok(self.get_many(&[id])?.into_iter().next())
    }

    /// Finds the photo stored at an absolute file path.
    pub fn photo_by_path(&self, path: &Path) -> Result<Option<Photo>> {
        let (Some(parent), Some(name)) = (path.parent(), path.file_name()) else {
            return Ok(None);
        };
        let (parent, name) = (
            path_str(parent)?.to_owned(),
            path_str(Path::new(name))?.to_owned(),
        );
        self.read(|c| {
            let mut stmt = c.prepare_cached(BY_PATH)?;
            let mut rows = stmt.query_map((parent, name), from_row)?;
            Ok(rows.next().transpose()?)
        })
    }

    /// `EXPLAIN QUERY PLAN` of [`Self::photo_by_path`]. Both the folder path and the file name
    /// are looked up through their unique indexes (SPEC-VIEWER §V8.2).
    pub fn photo_by_path_plan(&self) -> Result<Vec<String>> {
        self.read(|c| {
            let mut stmt = c.prepare(&format!("EXPLAIN QUERY PLAN {BY_PATH}"))?;
            let rows = stmt.query_map(("", ""), |r| r.get::<_, String>(3))?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// All photos with the given quick key (duplicates and moved files), ordered by id.
    pub fn photos_by_quick_key(&self, quick_key: &str) -> Result<Vec<Photo>> {
        self.read(|c| {
            let mut stmt =
                c.prepare_cached(&format!("{SELECT} WHERE p.quick_key = ?1 ORDER BY p.id"))?;
            Ok(stmt
                .query_map([quick_key], from_row)?
                .collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Ids of every photo, ascending.
    pub fn all_photo_ids(&self) -> Result<Vec<PhotoId>> {
        self.read(|c| {
            let mut stmt = c.prepare_cached("SELECT id FROM photos ORDER BY id")?;
            Ok(stmt
                .query_map([], |r| Ok(PhotoId::new(r.get(0)?)))?
                .collect::<rusqlite::Result<_>>()?)
        })
    }
}
