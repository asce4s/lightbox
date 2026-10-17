//! Folder Manager roots (SPEC §6.1).

use super::{dt_from_db, dt_to_db, enum_from_db, path_str};
use crate::{Catalog, error::Result};
use chrono::NaiveDateTime;
use lb_core::{Root, RootId, RootMode};
use rusqlite::{OptionalExtension, Row};
use std::path::{Path, PathBuf};

const SELECT: &str = "SELECT id, path, mode, added_at FROM roots";

fn from_row(r: &Row<'_>) -> rusqlite::Result<Root> {
    Ok(Root {
        id: RootId::new(r.get(0)?),
        path: PathBuf::from(r.get::<_, String>(1)?),
        mode: enum_from_db(2, &r.get::<_, String>(2)?)?,
        added_at: dt_from_db(3, &r.get::<_, String>(3)?)?,
    })
}

impl Catalog {
    /// Adds a root, or updates the mode of the existing root with the same path (its `added_at`
    /// is kept). `path` must already be normalized (SPEC §6.1.3).
    pub fn add_root(&self, path: &Path, mode: RootMode, now: NaiveDateTime) -> Result<Root> {
        let path = path_str(path)?.to_owned();
        self.write_tx(move |tx| {
            tx.execute(
                "INSERT INTO roots (path, mode, added_at) VALUES (?1, ?2, ?3)
                 ON CONFLICT(path) DO UPDATE SET mode = excluded.mode",
                (&path, mode.as_str(), dt_to_db(now)),
            )?;
            Ok(tx.query_row(&format!("{SELECT} WHERE path = ?1"), [&path], from_row)?)
        })
    }

    /// Looks up a root by its normalized path.
    pub fn root_by_path(&self, path: &Path) -> Result<Option<Root>> {
        let path = path_str(path)?.to_owned();
        self.read(|c| {
            Ok(
                c.query_row(&format!("{SELECT} WHERE path = ?1"), [path], from_row)
                    .optional()?,
            )
        })
    }

    /// All roots ordered by path.
    pub fn list_roots(&self) -> Result<Vec<Root>> {
        self.read(|c| {
            let mut stmt = c.prepare(&format!("{SELECT} ORDER BY path"))?;
            Ok(stmt
                .query_map([], from_row)?
                .collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Changes a root's mode; returns whether the root existed.
    pub fn set_root_mode(&self, id: RootId, mode: RootMode) -> Result<bool> {
        self.write_tx(move |tx| {
            Ok(tx.execute(
                "UPDATE roots SET mode = ?1 WHERE id = ?2",
                (mode.as_str(), id.get()),
            )? > 0)
        })
    }

    /// Deletes a root together with its folders, photos and dependent rows (files are untouched).
    /// Returns whether the root existed.
    pub fn remove_root(&self, id: RootId) -> Result<bool> {
        self.write_tx(move |tx| Ok(tx.execute("DELETE FROM roots WHERE id = ?1", [id.get()])? > 0))
    }
}
