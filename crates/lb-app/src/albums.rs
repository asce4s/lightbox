//! Albums (SPEC §6.7.3).

use crate::error::{AppError, Result};
use crate::events::AppEvent;
use crate::history::HistoryEntry;
use crate::lightbox::Lightbox;
use lb_core::{Album, AlbumId, PhotoId};

/// Creates and organizes albums.
#[derive(Clone)]
pub struct AlbumService {
    lb: Lightbox,
}

impl Lightbox {
    /// Album management.
    pub fn albums(&self) -> AlbumService {
        AlbumService { lb: self.clone() }
    }
}

fn clean_name(name: &str) -> Result<String> {
    let name = name.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(AppError::Invalid("album name is empty".into()));
    }
    Ok(name)
}

impl AlbumService {
    fn changed(&self) {
        self.lb.emit(AppEvent::AlbumsChanged);
    }

    /// Creates an album.
    pub fn create(&self, name: &str, description: Option<&str>) -> Result<Album> {
        let album = self.lb.catalog().create_album(
            &clean_name(name)?,
            description,
            self.lb.clock().now(),
        )?;
        self.changed();
        Ok(album)
    }

    /// One album.
    pub fn get(&self, id: AlbumId) -> Result<Option<Album>> {
        Ok(self.lb.catalog().get_album(id)?)
    }

    /// Albums in manual order, then by name.
    pub fn list(&self) -> Result<Vec<Album>> {
        Ok(self.lb.catalog().list_albums()?)
    }

    /// Renames an album.
    pub fn rename(&self, id: AlbumId, name: &str) -> Result<()> {
        if !self.lb.catalog().rename_album(id, &clean_name(name)?)? {
            return Err(AppError::NotFound("album"));
        }
        self.changed();
        Ok(())
    }

    /// Deletes an album. Its photos are untouched.
    pub fn delete(&self, id: AlbumId) -> Result<()> {
        if !self.lb.catalog().delete_album(id)? {
            return Err(AppError::NotFound("album"));
        }
        self.changed();
        Ok(())
    }

    /// Copies an album with its items under `new_name`.
    pub fn duplicate(&self, id: AlbumId, new_name: &str) -> Result<Album> {
        let album =
            self.lb
                .catalog()
                .duplicate_album(id, &clean_name(new_name)?, self.lb.clock().now())?;
        self.changed();
        Ok(album)
    }

    /// Sets or clears the cover photo.
    pub fn set_cover(&self, id: AlbumId, cover: Option<PhotoId>) -> Result<()> {
        if !self.lb.catalog().set_album_cover(id, cover)? {
            return Err(AppError::NotFound("album"));
        }
        self.changed();
        Ok(())
    }

    /// Members in album order.
    pub fn photo_ids(&self, id: AlbumId) -> Result<Vec<PhotoId>> {
        Ok(self.lb.catalog().album_photo_ids(id)?)
    }

    /// Appends photos not already in the album; one undo step. Returns how many were added.
    pub fn add_photos(&self, album: AlbumId, ids: &[PhotoId]) -> Result<usize> {
        self.get(album)?.ok_or(AppError::NotFound("album"))?;
        let present: std::collections::HashSet<_> = self.photo_ids(album)?.into_iter().collect();
        let mut seen = std::collections::HashSet::new();
        let fresh: Vec<PhotoId> = ids
            .iter()
            .copied()
            .filter(|id| !present.contains(id) && seen.insert(*id))
            .collect();
        if fresh.is_empty() {
            return Ok(0);
        }
        let (undo_ids, redo_ids) = (fresh.clone(), fresh.clone());
        self.lb.history().execute(HistoryEntry::new(
            format!("Add {} photos to album", fresh.len()),
            move |c| {
                c.remove_from_album(album, &undo_ids)?;
                Ok(vec![AppEvent::AlbumsChanged])
            },
            move |c| {
                c.add_to_album(album, &redo_ids)?;
                Ok(vec![AppEvent::AlbumsChanged])
            },
        ))?;
        Ok(fresh.len())
    }

    /// Removes photos from the album; one undo step restoring their positions.
    pub fn remove_photos(&self, album: AlbumId, ids: &[PhotoId]) -> Result<usize> {
        let items = self.lb.catalog().album_items(album)?;
        let removed: Vec<(PhotoId, f64)> = items
            .into_iter()
            .filter(|(id, _)| ids.contains(id))
            .collect();
        if removed.is_empty() {
            return Ok(0);
        }
        let removed_ids: Vec<PhotoId> = removed.iter().map(|(id, _)| *id).collect();
        let count = removed.len();
        self.lb.history().execute(HistoryEntry::new(
            format!("Remove {count} photos from album"),
            move |c| {
                c.restore_album_items(album, &removed)?;
                Ok(vec![AppEvent::AlbumsChanged])
            },
            move |c| {
                c.remove_from_album(album, &removed_ids)?;
                Ok(vec![AppEvent::AlbumsChanged])
            },
        ))?;
        Ok(count)
    }

    /// Moves `photo` to just before `before` (or to the end); one undo step.
    pub fn move_photo(
        &self,
        album: AlbumId,
        photo: PhotoId,
        before: Option<PhotoId>,
    ) -> Result<()> {
        let order = self.photo_ids(album)?;
        let index = order
            .iter()
            .position(|p| *p == photo)
            .ok_or(AppError::NotFound("album item"))?;
        let old_next = order.get(index + 1).copied();
        self.lb.history().execute(HistoryEntry::new(
            "Reorder album",
            move |c| {
                c.move_in_album(album, photo, old_next)?;
                Ok(vec![AppEvent::AlbumsChanged])
            },
            move |c| {
                c.move_in_album(album, photo, before)?;
                Ok(vec![AppEvent::AlbumsChanged])
            },
        ))
    }
}
