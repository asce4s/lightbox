//! Persisted edit stacks (SPEC §5.2 `edits`, §6.6). The stack itself is opaque JSON here.

use super::{dt_from_db, dt_to_db};
use crate::{
    Catalog,
    error::{CatalogError, Result},
};
use chrono::NaiveDateTime;
use lb_core::PhotoId;
use rusqlite::OptionalExtension;

/// A stored edit stack.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EditRecord {
    /// Serialized applied operations.
    pub ops_json: String,
    /// Serialized redo stack.
    pub redo_json: String,
    /// Last modification time.
    pub updated_at: NaiveDateTime,
}

fn edit_version(tx: &rusqlite::Transaction<'_>, id: PhotoId) -> Result<u32> {
    let v: Option<i64> = tx
        .query_row("UPDATE photos SET edit_version = edit_version + 1 WHERE id = ?1 RETURNING edit_version", [id.get()], |r| r.get(0))
        .optional()?;
    let v = v.ok_or(CatalogError::NotFound("photo"))?;
    Ok(u32::try_from(v).unwrap_or(u32::MAX))
}

impl Catalog {
    /// Reads a photo's edit stack, if it has one.
    pub fn get_edit(&self, photo_id: PhotoId) -> Result<Option<EditRecord>> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT ops_json, redo_json, updated_at FROM edits WHERE photo_id = ?1",
                [photo_id.get()],
                |r| {
                    Ok(EditRecord {
                        ops_json: r.get(0)?,
                        redo_json: r.get(1)?,
                        updated_at: dt_from_db(2, &r.get::<_, String>(2)?)?,
                    })
                },
            )
            .optional()?)
        })
    }

    /// Stores an edit stack and bumps `photos.edit_version` in the same transaction. Returns the
    /// new `edit_version`.
    pub fn set_edit(
        &self,
        photo_id: PhotoId,
        ops_json: String,
        redo_json: String,
        now: NaiveDateTime,
    ) -> Result<u32> {
        self.write_tx(move |tx| {
            let version = edit_version(tx, photo_id)?;
            tx.execute(
                "INSERT INTO edits (photo_id, ops_json, redo_json, updated_at) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(photo_id) DO UPDATE SET ops_json = excluded.ops_json,
                     redo_json = excluded.redo_json, updated_at = excluded.updated_at",
                (photo_id.get(), ops_json, redo_json, dt_to_db(now)),
            )?;
            Ok(version)
        })
    }

    /// Removes a photo's edit stack and bumps `edit_version`. Returns the new version.
    pub fn clear_edit(&self, photo_id: PhotoId) -> Result<u32> {
        self.write_tx(move |tx| {
            let version = edit_version(tx, photo_id)?;
            tx.execute("DELETE FROM edits WHERE photo_id = ?1", [photo_id.get()])?;
            Ok(version)
        })
    }
}
