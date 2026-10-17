//! Queries and updates used by the scanner and the Folder Manager (SPEC §6.1–§6.3).

use super::{dt_to_db, enum_from_db, int_from_db, path_str};
use crate::{Catalog, error::Result};
use chrono::NaiveDateTime;
use lb_core::{FolderId, PhotoId, PhotoStatus, RootId};
use rusqlite::Row;
use std::path::{MAIN_SEPARATOR_STR, Path, PathBuf};

/// The few columns of a photo row the scanner compares against the file system.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexedFile {
    /// Photo id.
    pub id: PhotoId,
    /// Containing folder.
    pub folder_id: FolderId,
    /// Root owning the folder.
    pub root_id: RootId,
    /// Absolute file path.
    pub path: PathBuf,
    /// Size in bytes when last indexed.
    pub size: u64,
    /// Modification time (unix seconds) when last indexed.
    pub mtime: i64,
    /// Quick key when last indexed.
    pub quick_key: String,
    /// Indexing state.
    pub status: PhotoStatus,
}

fn indexed_from_row(r: &Row<'_>) -> rusqlite::Result<IndexedFile> {
    let dir: String = r.get(3)?;
    let name: String = r.get(4)?;
    Ok(IndexedFile {
        id: PhotoId::new(r.get(0)?),
        folder_id: FolderId::new(r.get(1)?),
        root_id: RootId::new(r.get(2)?),
        path: PathBuf::from(dir).join(name),
        size: int_from_db(5, r.get(5)?)?,
        mtime: r.get(6)?,
        quick_key: r.get(7)?,
        status: enum_from_db(8, &r.get::<_, String>(8)?)?,
    })
}

/// `(exact, prefix_len, prefix)` bind values selecting `dir` itself and everything below it.
/// Uses `substr` instead of `LIKE` so `%`/`_` in directory names need no escaping.
fn subtree(dir: &Path) -> Result<(String, i64, String)> {
    let exact = path_str(dir)?.to_owned();
    let prefix = if exact.ends_with(MAIN_SEPARATOR_STR) {
        exact.clone()
    } else {
        format!("{exact}{MAIN_SEPARATOR_STR}")
    };
    let len = i64::try_from(prefix.chars().count()).unwrap_or(i64::MAX);
    Ok((exact, len, prefix))
}

impl Catalog {
    /// Every indexed file (any status) in `dir` and its sub-directories.
    pub fn indexed_files_under(&self, dir: &Path) -> Result<Vec<IndexedFile>> {
        let (exact, len, prefix) = subtree(dir)?;
        self.read(|c| {
            let mut stmt = c.prepare_cached(
                "SELECT p.id, p.folder_id, f.root_id, f.path, p.file_name, p.file_size,
                        p.file_mtime, p.quick_key, p.status
                 FROM photos p JOIN folders f ON f.id = p.folder_id
                 WHERE f.path = ?1 OR substr(f.path, 1, ?2) = ?3
                 ORDER BY f.path, p.file_name",
            )?;
            Ok(stmt
                .query_map((exact, len, prefix), indexed_from_row)?
                .collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Sets `status` (and `error`) on the given photos; returns how many rows changed.
    pub fn set_photos_status(
        &self,
        ids: &[PhotoId],
        status: PhotoStatus,
        error: Option<&str>,
    ) -> Result<usize> {
        let ids = ids.to_vec();
        let error = error.map(str::to_owned);
        self.write_tx(move |tx| {
            let mut stmt =
                tx.prepare_cached("UPDATE photos SET status = ?1, error = ?2 WHERE id = ?3")?;
            let mut changed = 0;
            for id in &ids {
                changed += stmt.execute((status.as_str(), &error, id.get()))?;
            }
            Ok(changed)
        })
    }

    /// Deletes the folder at `dir` and every folder below it, together with their photos and
    /// dependent rows (files on disk are untouched). Returns the removed photo ids.
    pub fn remove_folder_tree(&self, dir: &Path) -> Result<Vec<PhotoId>> {
        let (exact, len, prefix) = subtree(dir)?;
        self.write_tx(move |tx| {
            let ids = {
                let mut stmt = tx.prepare(
                    "SELECT p.id FROM photos p JOIN folders f ON f.id = p.folder_id
                     WHERE f.path = ?1 OR substr(f.path, 1, ?2) = ?3 ORDER BY p.id",
                )?;
                stmt.query_map((&exact, len, &prefix), |r| Ok(PhotoId::new(r.get(0)?)))?
                    .collect::<rusqlite::Result<Vec<_>>>()?
            };
            tx.execute(
                "DELETE FROM folders WHERE path = ?1 OR substr(path, 1, ?2) = ?3",
                (&exact, len, &prefix),
            )?;
            Ok(ids)
        })
    }

    /// Recomputes `folders.sort_date` (earliest `taken_at` of the folder's photos) for a root.
    pub fn refresh_folder_sort_dates(&self, root: RootId) -> Result<()> {
        self.write_tx(move |tx| {
            tx.execute(
                "UPDATE folders SET sort_date =
                   (SELECT MIN(taken_at) FROM photos WHERE photos.folder_id = folders.id)
                 WHERE root_id = ?1",
                [root.get()],
            )?;
            Ok(())
        })
    }

    /// Moves a photo row to another folder and/or file name, keeping its id and every
    /// user-managed field (SPEC §6.3.2). `mtime` and `indexed_at` are refreshed.
    pub fn relocate_photo(
        &self,
        id: PhotoId,
        folder_id: FolderId,
        file_name: &str,
        now: NaiveDateTime,
    ) -> Result<()> {
        let file_name = file_name.to_owned();
        self.write_tx(move |tx| {
            let changed = tx.execute(
                "UPDATE photos SET folder_id = ?1, file_name = ?2, status = 'ok', error = NULL,
                                   indexed_at = ?3 WHERE id = ?4",
                (folder_id.get(), &file_name, dt_to_db(now), id.get()),
            )?;
            if changed == 0 {
                return Err(crate::CatalogError::NotFound("photo"));
            }
            crate::fts::refresh_photos(tx, &[id])?;
            Ok(())
        })
    }
}
