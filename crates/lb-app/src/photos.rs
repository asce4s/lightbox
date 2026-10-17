//! Photo lookups, metadata edits and file operations (SPEC §6.7, §7).

use crate::error::Result;
use crate::lightbox::Lightbox;
use lb_core::{Photo, PhotoId};

/// Reads and modifies photos.
#[derive(Clone)]
pub struct PhotoService {
    pub(crate) lb: Lightbox,
}

impl Lightbox {
    /// Photo queries and organize actions.
    pub fn photos(&self) -> PhotoService {
        PhotoService { lb: self.clone() }
    }
}

impl PhotoService {
    /// One photo by id.
    pub fn get(&self, id: PhotoId) -> Result<Option<Photo>> {
        Ok(self.lb.catalog().get_photo(id)?)
    }

    /// Photos in the order of `ids`; unknown ids are skipped.
    pub fn get_many(&self, ids: &[PhotoId]) -> Result<Vec<Photo>> {
        Ok(self.lb.catalog().get_many(ids)?)
    }

    /// Ids of every photo in the library, ascending.
    pub fn all_ids(&self) -> Result<Vec<PhotoId>> {
        Ok(self.lb.catalog().all_photo_ids()?)
    }

    /// Every photo in the library, ascending by id.
    pub fn all(&self) -> Result<Vec<Photo>> {
        self.get_many(&self.all_ids()?)
    }
}
