//! Albums and their fractional-position ordering (SPEC §6.7.3).

use super::{dt_from_db, dt_to_db, placeholders};
use crate::{
    Catalog,
    error::{CatalogError, Result},
};
use chrono::NaiveDateTime;
use lb_core::{Album, AlbumId, PhotoId};
use rusqlite::{OptionalExtension, Row, Transaction};

const SELECT: &str =
    "SELECT id, name, description, cover_photo_id, created_at, sort_order FROM albums";

/// Gaps between neighbouring positions below this trigger renormalization (SPEC §6.7.3).
const MIN_GAP: f64 = 1e-9;

fn from_row(r: &Row<'_>) -> rusqlite::Result<Album> {
    Ok(Album {
        id: AlbumId::new(r.get(0)?),
        name: r.get(1)?,
        description: r.get(2)?,
        cover_photo_id: r.get::<_, Option<i64>>(3)?.map(PhotoId::new),
        created_at: dt_from_db(4, &r.get::<_, String>(4)?)?,
        sort_order: r.get(5)?,
    })
}

fn items_tx(tx: &Transaction<'_>, album: AlbumId) -> Result<Vec<(PhotoId, f64)>> {
    let mut stmt = tx.prepare_cached("SELECT photo_id, position FROM album_items WHERE album_id = ?1 ORDER BY position, photo_id")?;
    Ok(stmt
        .query_map([album.get()], |r| Ok((PhotoId::new(r.get(0)?), r.get(1)?)))?
        .collect::<rusqlite::Result<_>>()?)
}

fn renormalize_tx(tx: &Transaction<'_>, album: AlbumId) -> Result<()> {
    let mut update = tx.prepare_cached(
        "UPDATE album_items SET position = ?1 WHERE album_id = ?2 AND photo_id = ?3",
    )?;
    for (i, (photo, _)) in items_tx(tx, album)?.into_iter().enumerate() {
        update.execute((i as f64 + 1.0, album.get(), photo.get()))?;
    }
    Ok(())
}

fn require_album(tx: &Transaction<'_>, album: AlbumId) -> Result<()> {
    if tx
        .query_row("SELECT 1 FROM albums WHERE id = ?1", [album.get()], |_| {
            Ok(())
        })
        .optional()?
        .is_none()
    {
        return Err(CatalogError::NotFound("album"));
    }
    Ok(())
}

impl Catalog {
    /// Creates an album after all existing ones in manual order.
    pub fn create_album(
        &self,
        name: &str,
        description: Option<&str>,
        now: NaiveDateTime,
    ) -> Result<Album> {
        let (name, description) = (name.to_owned(), description.map(str::to_owned));
        self.write_tx(move |tx| {
            tx.execute(
                "INSERT INTO albums (name, description, created_at, sort_order)
                 VALUES (?1, ?2, ?3, (SELECT COALESCE(MAX(sort_order), 0) + 1 FROM albums))",
                (name, description, dt_to_db(now)),
            )?;
            Ok(tx.query_row(
                &format!("{SELECT} WHERE id = ?1"),
                [tx.last_insert_rowid()],
                from_row,
            )?)
        })
    }

    /// Looks up an album by id.
    pub fn get_album(&self, id: AlbumId) -> Result<Option<Album>> {
        self.read(|c| {
            Ok(
                c.query_row(&format!("{SELECT} WHERE id = ?1"), [id.get()], from_row)
                    .optional()?,
            )
        })
    }

    /// All albums ordered by `sort_order`, then name (SPEC §6.7.3).
    pub fn list_albums(&self) -> Result<Vec<Album>> {
        self.read(|c| {
            let mut stmt = c.prepare(&format!(
                "{SELECT} ORDER BY sort_order, name COLLATE NOCASE, id"
            ))?;
            Ok(stmt
                .query_map([], from_row)?
                .collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Renames an album; returns whether it existed.
    pub fn rename_album(&self, id: AlbumId, name: &str) -> Result<bool> {
        let name = name.to_owned();
        self.write_tx(move |tx| {
            let changed = tx.execute(
                "UPDATE albums SET name = ?1 WHERE id = ?2",
                (name, id.get()),
            )? > 0;
            crate::fts::refresh_photos(tx, &crate::fts::photos_in_album(tx, id)?)?;
            Ok(changed)
        })
    }

    /// Deletes an album and its membership rows; the photos themselves are untouched.
    pub fn delete_album(&self, id: AlbumId) -> Result<bool> {
        self.write_tx(move |tx| {
            let members = crate::fts::photos_in_album(tx, id)?;
            let existed = tx.execute("DELETE FROM albums WHERE id = ?1", [id.get()])? > 0;
            crate::fts::refresh_photos(tx, &members)?;
            Ok(existed)
        })
    }

    /// Sets or clears the cover photo.
    pub fn set_album_cover(&self, id: AlbumId, cover: Option<PhotoId>) -> Result<bool> {
        self.write_tx(move |tx| {
            Ok(tx.execute(
                "UPDATE albums SET cover_photo_id = ?1 WHERE id = ?2",
                (cover.map(PhotoId::get), id.get()),
            )? > 0)
        })
    }

    /// Copies an album with all its items (same positions) under a new name.
    pub fn duplicate_album(
        &self,
        id: AlbumId,
        new_name: &str,
        now: NaiveDateTime,
    ) -> Result<Album> {
        let new_name = new_name.to_owned();
        self.write_tx(move |tx| {
            require_album(tx, id)?;
            tx.execute(
                "INSERT INTO albums (name, description, cover_photo_id, created_at, sort_order)
                 SELECT ?1, description, cover_photo_id, ?2, (SELECT MAX(sort_order) + 1 FROM albums)
                 FROM albums WHERE id = ?3",
                (new_name, dt_to_db(now), id.get()),
            )?;
            let new_id = tx.last_insert_rowid();
            tx.execute(
                "INSERT INTO album_items (album_id, photo_id, position)
                 SELECT ?1, photo_id, position FROM album_items WHERE album_id = ?2",
                (new_id, id.get()),
            )?;
            crate::fts::refresh_photos(tx, &crate::fts::photos_in_album(tx, AlbumId::new(new_id))?)?;
            Ok(tx.query_row(&format!("{SELECT} WHERE id = ?1"), [new_id], from_row)?)
        })
    }

    /// Appends photos to the end of an album in the given order, skipping ones already present.
    /// Returns how many were added.
    pub fn add_to_album(&self, album: AlbumId, photos: &[PhotoId]) -> Result<usize> {
        let photos = photos.to_vec();
        self.write_tx(move |tx| {
            require_album(tx, album)?;
            let mut next: f64 = tx.query_row(
                "SELECT COALESCE(MAX(position), 0) + 1 FROM album_items WHERE album_id = ?1",
                [album.get()],
                |r| r.get(0),
            )?;
            let mut insert = tx.prepare_cached("INSERT OR IGNORE INTO album_items (album_id, photo_id, position) VALUES (?1, ?2, ?3)")?;
            let mut added = 0;
            for photo in &photos {
                if insert.execute((album.get(), photo.get(), next))? > 0 {
                    added += 1;
                    next += 1.0;
                }
            }
            drop(insert);
            if added > 0 {
                crate::fts::refresh_photos(tx, &photos)?;
            }
            Ok(added)
        })
    }

    /// Removes photos from an album; returns how many were members.
    pub fn remove_from_album(&self, album: AlbumId, photos: &[PhotoId]) -> Result<usize> {
        let photos = photos.to_vec();
        self.write_tx(move |tx| {
            let mut removed = 0;
            for chunk in photos.chunks(500) {
                let sql = format!(
                    "DELETE FROM album_items WHERE album_id = ? AND photo_id IN ({})",
                    placeholders(chunk.len())
                );
                let params = std::iter::once(album.get()).chain(chunk.iter().map(|p| p.get()));
                removed += tx.execute(&sql, rusqlite::params_from_iter(params))?;
            }
            if removed > 0 {
                crate::fts::refresh_photos(tx, &photos)?;
            }
            Ok(removed)
        })
    }

    /// Album members with their positions, in album order.
    pub fn album_items(&self, album: AlbumId) -> Result<Vec<(PhotoId, f64)>> {
        self.read(|c| {
            let mut stmt = c.prepare_cached("SELECT photo_id, position FROM album_items WHERE album_id = ?1 ORDER BY position, photo_id")?;
            Ok(stmt.query_map([album.get()], |r| Ok((PhotoId::new(r.get(0)?), r.get(1)?)))?.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Album members in album order.
    pub fn album_photo_ids(&self, album: AlbumId) -> Result<Vec<PhotoId>> {
        Ok(self
            .album_items(album)?
            .into_iter()
            .map(|(id, _)| id)
            .collect())
    }

    /// Moves `photo` to just before `before` (or to the end when `None`), touching only the moved
    /// row's position: it takes the midpoint between its new neighbours. When any gap between
    /// neighbouring positions falls below 1e-9 the whole album is renormalized to `1.0, 2.0, ...`.
    /// Returns whether renormalization happened.
    pub fn move_in_album(
        &self,
        album: AlbumId,
        photo: PhotoId,
        before: Option<PhotoId>,
    ) -> Result<bool> {
        self.write_tx(move |tx| {
            let others: Vec<(PhotoId, f64)> = items_tx(tx, album)?
                .into_iter()
                .filter(|(id, _)| *id != photo)
                .collect();
            let position = match before {
                Some(target) => {
                    let idx = others
                        .iter()
                        .position(|(id, _)| *id == target)
                        .ok_or(CatalogError::NotFound("album item"))?;
                    let next = others[idx].1;
                    match idx.checked_sub(1).map(|i| others[i].1) {
                        Some(prev) => (prev + next) / 2.0,
                        None => next - 1.0,
                    }
                }
                None => others.last().map_or(1.0, |(_, p)| p + 1.0),
            };
            let changed = tx.execute(
                "UPDATE album_items SET position = ?1 WHERE album_id = ?2 AND photo_id = ?3",
                (position, album.get(), photo.get()),
            )?;
            if changed == 0 {
                return Err(CatalogError::NotFound("album item"));
            }
            let positions: Vec<f64> = items_tx(tx, album)?.into_iter().map(|(_, p)| p).collect();
            let crowded = positions.windows(2).any(|w| w[1] - w[0] < MIN_GAP);
            if crowded {
                renormalize_tx(tx, album)?;
            }
            Ok(crowded)
        })
    }

    /// Rewrites the album's positions as `1.0, 2.0, ...` keeping the current order.
    pub fn renormalize_album(&self, album: AlbumId) -> Result<()> {
        self.write_tx(move |tx| renormalize_tx(tx, album))
    }
}
