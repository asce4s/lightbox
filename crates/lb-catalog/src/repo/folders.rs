//! Indexed directories (SPEC §5.2 `folders`).

use super::{dt_from_db, path_str};
use crate::{Catalog, error::Result};
use lb_core::{Folder, FolderId, RootId};
use rusqlite::{OptionalExtension, Row};
use std::path::{Path, PathBuf};

const SELECT: &str = "SELECT id, root_id, path, parent_id, name, caption, sort_date FROM folders";

/// Input for [`Catalog::ensure_folder`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewFolder {
    /// Owning root.
    pub root_id: RootId,
    /// Absolute, normalized path.
    pub path: PathBuf,
    /// Parent folder, `None` for the root directory itself.
    pub parent_id: Option<FolderId>,
}

fn from_row(r: &Row<'_>) -> rusqlite::Result<Folder> {
    let sort_date = r.get::<_, Option<String>>(6)?;
    Ok(Folder {
        id: FolderId::new(r.get(0)?),
        root_id: RootId::new(r.get(1)?),
        path: PathBuf::from(r.get::<_, String>(2)?),
        parent_id: r.get::<_, Option<i64>>(3)?.map(FolderId::new),
        name: r.get(4)?,
        caption: r.get(5)?,
        sort_date: sort_date.map(|s| dt_from_db(6, &s)).transpose()?,
    })
}

pub(super) fn folder_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.to_string_lossy().into_owned(),
        |n| n.to_string_lossy().into_owned(),
    )
}

impl Catalog {
    /// Returns the folder at `new.path`, creating it if needed. Existing folders keep their
    /// caption; root and parent are refreshed.
    pub fn ensure_folder(&self, new: NewFolder) -> Result<Folder> {
        let path = path_str(&new.path)?.to_owned();
        let name = folder_name(&new.path);
        self.write_tx(move |tx| {
            tx.execute(
                "INSERT INTO folders (root_id, path, parent_id, name) VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(path) DO UPDATE SET root_id = excluded.root_id,
                     parent_id = excluded.parent_id, name = excluded.name",
                (
                    new.root_id.get(),
                    &path,
                    new.parent_id.map(FolderId::get),
                    name,
                ),
            )?;
            Ok(tx.query_row(&format!("{SELECT} WHERE path = ?1"), [&path], from_row)?)
        })
    }

    /// Looks up a folder by id.
    pub fn get_folder(&self, id: FolderId) -> Result<Option<Folder>> {
        self.read(|c| {
            Ok(
                c.query_row(&format!("{SELECT} WHERE id = ?1"), [id.get()], from_row)
                    .optional()?,
            )
        })
    }

    /// Looks up a folder by normalized path.
    pub fn folder_by_path(&self, path: &Path) -> Result<Option<Folder>> {
        let path = path_str(path)?.to_owned();
        self.read(|c| {
            Ok(
                c.query_row(&format!("{SELECT} WHERE path = ?1"), [path], from_row)
                    .optional()?,
            )
        })
    }

    /// All folders under a root ordered by path.
    pub fn folders_in_root(&self, root_id: RootId) -> Result<Vec<Folder>> {
        self.read(|c| {
            let mut stmt = c.prepare(&format!("{SELECT} WHERE root_id = ?1 ORDER BY path"))?;
            Ok(stmt
                .query_map([root_id.get()], from_row)?
                .collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Sets or clears a folder's caption; returns whether the folder existed.
    pub fn set_folder_caption(&self, id: FolderId, caption: Option<String>) -> Result<bool> {
        self.write_tx(move |tx| {
            Ok(tx.execute(
                "UPDATE folders SET caption = ?1 WHERE id = ?2",
                (caption, id.get()),
            )? > 0)
        })
    }
}
