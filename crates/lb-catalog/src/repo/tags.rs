//! Tags and photo-tag links (SPEC §6.7.2).

use crate::{
    Catalog,
    error::{CatalogError, Result},
};
use lb_core::{PhotoId, Tag, TagId, TagUsage, normalize_tag_name};
use rusqlite::{OptionalExtension, Transaction};

/// Finds a tag by name, case-insensitively for all of Unicode (the column collation only folds
/// ASCII, so non-ASCII names fall back to a Rust-side comparison).
fn find_tag(tx: &Transaction<'_>, name: &str) -> Result<Option<Tag>> {
    let row = |r: &rusqlite::Row<'_>| {
        Ok(Tag {
            id: TagId::new(r.get(0)?),
            name: r.get(1)?,
        })
    };
    if let Some(tag) = tx
        .query_row(
            "SELECT id, name FROM tags WHERE name = ?1 COLLATE NOCASE",
            [name],
            row,
        )
        .optional()?
    {
        return Ok(Some(tag));
    }
    if name.is_ascii() {
        return Ok(None);
    }
    let wanted = name.to_lowercase();
    let mut stmt = tx.prepare_cached("SELECT id, name FROM tags")?;
    for tag in stmt.query_map([], row)? {
        let tag = tag?;
        if tag.name.to_lowercase() == wanted {
            return Ok(Some(tag));
        }
    }
    Ok(None)
}

fn get_or_create(tx: &Transaction<'_>, raw: &str) -> Result<Tag> {
    let name = normalize_tag_name(raw)?;
    if let Some(tag) = find_tag(tx, &name)? {
        return Ok(tag);
    }
    tx.execute("INSERT INTO tags (name) VALUES (?1)", [&name])?;
    Ok(Tag {
        id: TagId::new(tx.last_insert_rowid()),
        name,
    })
}

impl Catalog {
    /// Returns the tag with this (normalized, case-insensitive) name, creating it if needed.
    /// The first spelling stored wins.
    pub fn get_or_create_tag(&self, name: &str) -> Result<Tag> {
        let name = name.to_owned();
        self.write_tx(move |tx| get_or_create(tx, &name))
    }

    /// Finds a tag by name without creating it.
    pub fn find_tag(&self, name: &str) -> Result<Option<Tag>> {
        let name = normalize_tag_name(name)?;
        self.write_tx(move |tx| find_tag(tx, &name))
    }

    /// Tags every photo in `photos` with every name in `names` in one transaction (creating tags
    /// as needed). Existing links are kept. Returns the number of new links.
    pub fn add_tags(&self, photos: &[PhotoId], names: &[String]) -> Result<usize> {
        let (photos, names) = (photos.to_vec(), names.to_vec());
        self.write_tx(move |tx| {
            let tags = names
                .iter()
                .map(|n| get_or_create(tx, n))
                .collect::<Result<Vec<_>>>()?;
            let mut link = tx.prepare_cached(
                "INSERT OR IGNORE INTO photo_tags (photo_id, tag_id) VALUES (?1, ?2)",
            )?;
            let mut added = 0;
            for photo in &photos {
                for tag in &tags {
                    added += link.execute((photo.get(), tag.id.get()))?;
                }
            }
            Ok(added)
        })
    }

    /// Removes the given tags from the given photos; unknown names are ignored. Returns the number
    /// of links removed. Tags left without photos are kept.
    pub fn remove_tags(&self, photos: &[PhotoId], names: &[String]) -> Result<usize> {
        let (photos, names) = (photos.to_vec(), names.to_vec());
        self.write_tx(move |tx| {
            let mut ids = Vec::new();
            for n in &names {
                if let Some(tag) = find_tag(tx, &normalize_tag_name(n)?)? {
                    ids.push(tag.id);
                }
            }
            let mut unlink =
                tx.prepare_cached("DELETE FROM photo_tags WHERE photo_id = ?1 AND tag_id = ?2")?;
            let mut removed = 0;
            for photo in &photos {
                for tag in &ids {
                    removed += unlink.execute((photo.get(), tag.get()))?;
                }
            }
            Ok(removed)
        })
    }

    /// Tags attached to a photo, ordered by name.
    pub fn tags_for_photo(&self, photo: PhotoId) -> Result<Vec<Tag>> {
        self.read(|c| {
            let mut stmt = c.prepare_cached(
                "SELECT t.id, t.name FROM tags t JOIN photo_tags pt ON pt.tag_id = t.id
                 WHERE pt.photo_id = ?1 ORDER BY t.name COLLATE NOCASE, t.id",
            )?;
            Ok(stmt
                .query_map([photo.get()], |r| {
                    Ok(Tag {
                        id: TagId::new(r.get(0)?),
                        name: r.get(1)?,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Every tag with its photo count, ordered by name.
    pub fn list_tags(&self) -> Result<Vec<TagUsage>> {
        self.read(|c| {
            let mut stmt = c.prepare(
                "SELECT t.id, t.name, COUNT(pt.photo_id) FROM tags t
                 LEFT JOIN photo_tags pt ON pt.tag_id = t.id
                 GROUP BY t.id ORDER BY t.name COLLATE NOCASE, t.id",
            )?;
            let rows = stmt.query_map([], |r| {
                Ok(TagUsage {
                    tag: Tag {
                        id: TagId::new(r.get(0)?),
                        name: r.get(1)?,
                    },
                    count: r.get(2)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Renames a tag. When another tag already has the target name (case-insensitively) the two
    /// are merged into it; returns the surviving tag.
    pub fn rename_tag(&self, id: TagId, new_name: &str) -> Result<Tag> {
        let new_name = normalize_tag_name(new_name)?;
        self.write_tx(move |tx| {
            let exists: Option<i64> = tx
                .query_row("SELECT 1 FROM tags WHERE id = ?1", [id.get()], |r| r.get(0))
                .optional()?;
            if exists.is_none() {
                return Err(CatalogError::NotFound("tag"));
            }
            match find_tag(tx, &new_name)? {
                Some(target) if target.id != id => {
                    tx.execute(
                        "INSERT OR IGNORE INTO photo_tags (photo_id, tag_id)
                         SELECT photo_id, ?1 FROM photo_tags WHERE tag_id = ?2",
                        (target.id.get(), id.get()),
                    )?;
                    tx.execute("DELETE FROM tags WHERE id = ?1", [id.get()])?;
                    Ok(target)
                }
                _ => {
                    tx.execute(
                        "UPDATE tags SET name = ?1 WHERE id = ?2",
                        (&new_name, id.get()),
                    )?;
                    Ok(Tag { id, name: new_name })
                }
            }
        })
    }

    /// Deletes a tag and its links; returns whether it existed.
    pub fn delete_tag(&self, id: TagId) -> Result<bool> {
        self.write_tx(move |tx| Ok(tx.execute("DELETE FROM tags WHERE id = ?1", [id.get()])? > 0))
    }
}
