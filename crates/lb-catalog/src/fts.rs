//! Maintenance of the contentless `photos_fts` index (SPEC §5.2).
//!
//! Every repository function that changes a source field (file name, caption, tags, folder path,
//! camera, people, album names) calls [`refresh_photos`] with the affected photos inside its own
//! transaction. Row deletion is handled by the `photos_fts_after_delete` trigger.
//! [`Catalog::rebuild_fts`] recomputes the whole index with the same row query.

use crate::{Catalog, error::Result, repo::placeholders};
use lb_core::{AlbumId, PhotoId, TagId};
use rusqlite::Transaction;

/// Produces one FTS row per photo: `(rowid, file_name, caption, tags, folder_path, camera,
/// people, album_names)`. Group concatenations are ordered so the output is deterministic.
const ROW_SELECT: &str = "
SELECT p.id,
       p.file_name,
       COALESCE(p.caption, ''),
       COALESCE((SELECT group_concat(name, ' ') FROM (
           SELECT t.name AS name FROM photo_tags pt JOIN tags t ON t.id = pt.tag_id
           WHERE pt.photo_id = p.id ORDER BY t.name, t.id)), ''),
       f.path,
       trim(COALESCE(p.camera_make, '') || ' ' || COALESCE(p.camera_model, '')),
       COALESCE((SELECT group_concat(name, ' ') FROM (
           SELECT DISTINCT pe.name AS name FROM faces fa JOIN people pe ON pe.id = fa.person_id
           WHERE fa.photo_id = p.id ORDER BY pe.name)), ''),
       COALESCE((SELECT group_concat(name, ' ') FROM (
           SELECT al.name AS name FROM album_items ai JOIN albums al ON al.id = ai.album_id
           WHERE ai.photo_id = p.id ORDER BY al.name, al.id)), '')
FROM photos p JOIN folders f ON f.id = p.folder_id";

const INSERT_COLUMNS: &str = "INSERT INTO photos_fts (rowid, file_name, caption, tags, folder_path, camera, people, album_names)";

const CHUNK: usize = 500;

/// Recomputes the index rows of `ids`. Unknown ids are removed from the index.
pub(crate) fn refresh_photos(tx: &Transaction<'_>, ids: &[PhotoId]) -> Result<()> {
    for chunk in ids.chunks(CHUNK) {
        let marks = placeholders(chunk.len());
        let params = || rusqlite::params_from_iter(chunk.iter().map(|i| i.get()));
        tx.execute(
            &format!("DELETE FROM photos_fts WHERE rowid IN ({marks})"),
            params(),
        )?;
        tx.execute(
            &format!("{INSERT_COLUMNS} {ROW_SELECT} WHERE p.id IN ({marks})"),
            params(),
        )?;
    }
    Ok(())
}

/// Drops and recomputes every index row.
pub(crate) fn rebuild(tx: &Transaction<'_>) -> Result<()> {
    tx.execute(
        "INSERT INTO photos_fts (photos_fts) VALUES ('delete-all')",
        [],
    )?;
    tx.execute(&format!("{INSERT_COLUMNS} {ROW_SELECT}"), [])?;
    Ok(())
}

pub(crate) fn photos_in_album(tx: &Transaction<'_>, album: AlbumId) -> Result<Vec<PhotoId>> {
    let mut stmt = tx.prepare_cached("SELECT photo_id FROM album_items WHERE album_id = ?1")?;
    Ok(stmt
        .query_map([album.get()], |r| Ok(PhotoId::new(r.get(0)?)))?
        .collect::<rusqlite::Result<_>>()?)
}

pub(crate) fn photos_with_tag(tx: &Transaction<'_>, tag: TagId) -> Result<Vec<PhotoId>> {
    let mut stmt = tx.prepare_cached("SELECT photo_id FROM photo_tags WHERE tag_id = ?1")?;
    Ok(stmt
        .query_map([tag.get()], |r| Ok(PhotoId::new(r.get(0)?)))?
        .collect::<rusqlite::Result<_>>()?)
}

impl Catalog {
    /// Recomputes the full-text index from the tables. Incremental maintenance should already keep
    /// it identical; this repairs drift and populates the index after schema migrations.
    pub fn rebuild_fts(&self) -> Result<()> {
        self.write_tx(rebuild)
    }
}
