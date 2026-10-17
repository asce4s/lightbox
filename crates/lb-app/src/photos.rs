//! Photo lookups, metadata edits and file operations (SPEC §6.7, §7).

use crate::error::{AppError, Result};
use crate::events::AppEvent;
use crate::history::HistoryEntry;
use crate::lightbox::Lightbox;
use crate::query::ViewSource;
use lb_catalog::{Catalog, PhotoQuery};
use lb_core::{Photo, PhotoId, SortOrder};
use std::sync::Arc;

/// Longest caption accepted, in characters (SPEC §6.7.1).
pub const MAX_CAPTION_CHARS: usize = 2000;
/// Highest star rating.
pub const MAX_RATING: u8 = 5;

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

type Setter<T> = Arc<dyn Fn(&Catalog, &[PhotoId], T) -> lb_catalog::Result<usize> + Send + Sync>;

impl PhotoService {
    /// Lists the ids of `source` in the default order, hiding hidden and missing photos unless
    /// `show_hidden` is set.
    pub fn list_ids(&self, source: &ViewSource, show_hidden: bool) -> Result<Vec<PhotoId>> {
        let mut q = PhotoQuery::new(source.to_catalog());
        q.show_hidden = show_hidden;
        q.sort = SortOrder::DateTakenAsc;
        Ok(self
            .lb
            .catalog()
            .query_photos(&q)?
            .into_iter()
            .map(|r| r.id)
            .collect())
    }

    /// Changes one per-photo value as a single undoable step. Photos that already hold the value
    /// are left alone; nothing is recorded when no photo changes. Returns how many changed.
    fn change<T>(
        &self,
        ids: &[PhotoId],
        label: String,
        new: T,
        read: impl Fn(&Photo) -> T,
        write: Setter<T>,
    ) -> Result<usize>
    where
        T: Clone + PartialEq + Send + Sync + 'static,
    {
        let photos = self.get_many(ids)?;
        let mut seen = std::collections::HashSet::new();
        let before: Vec<(PhotoId, T)> = photos
            .iter()
            .filter(|p| seen.insert(p.id) && read(p) != new)
            .map(|p| (p.id, read(p)))
            .collect();
        if before.is_empty() {
            return Ok(0);
        }
        let changed: Vec<PhotoId> = before.iter().map(|(id, _)| *id).collect();
        let (undo_before, undo_write, undo_ids) = (before.clone(), write.clone(), changed.clone());
        let (redo_new, redo_ids) = (new, changed.clone());
        let entry = HistoryEntry::new(
            label,
            move |c| {
                let mut groups: Vec<(T, Vec<PhotoId>)> = Vec::new();
                for (id, old) in &undo_before {
                    match groups.iter_mut().find(|(v, _)| v == old) {
                        Some((_, g)) => g.push(*id),
                        None => groups.push((old.clone(), vec![*id])),
                    }
                }
                for (value, group) in groups {
                    undo_write(c, &group, value)?;
                }
                Ok(vec![AppEvent::PhotosChanged {
                    ids: undo_ids.clone(),
                }])
            },
            move |c| {
                write(c, &redo_ids, redo_new.clone())?;
                Ok(vec![AppEvent::PhotosChanged {
                    ids: redo_ids.clone(),
                }])
            },
        );
        self.lb.history().execute(entry)?;
        Ok(changed.len())
    }

    /// Stars or unstars every photo in `ids` in one undoable step.
    pub fn set_starred(&self, ids: &[PhotoId], starred: bool) -> Result<usize> {
        let label = format!(
            "{} {}",
            if starred { "Star" } else { "Unstar" },
            noun(ids.len())
        );
        self.change(
            ids,
            label,
            starred,
            |p| p.starred,
            Arc::new(|c, ids, v| c.set_starred(ids, v)),
        )
    }

    /// Picasa-style star toggle for a selection: if every photo is starred they all become
    /// unstarred, otherwise they all become starred. Returns the new state.
    pub fn toggle_star(&self, ids: &[PhotoId]) -> Result<bool> {
        let photos = self.get_many(ids)?;
        let target = !(photos.iter().all(|p| p.starred) && !photos.is_empty());
        self.set_starred(ids, target)?;
        Ok(target)
    }

    /// Sets the 0–5 rating of every photo in `ids`. Values above 5 are rejected.
    pub fn set_rating(&self, ids: &[PhotoId], rating: u8) -> Result<usize> {
        if rating > MAX_RATING {
            return Err(AppError::Invalid(format!(
                "rating must be between 0 and {MAX_RATING}, got {rating}"
            )));
        }
        self.change(
            ids,
            format!("Rate {} {rating}", noun(ids.len())),
            rating,
            |p| p.rating,
            Arc::new(|c, ids, v| c.set_rating(ids, v)),
        )
    }

    /// Hides or unhides photos (Picasa "Hide").
    pub fn set_hidden(&self, ids: &[PhotoId], hidden: bool) -> Result<usize> {
        let label = format!(
            "{} {}",
            if hidden { "Hide" } else { "Unhide" },
            noun(ids.len())
        );
        self.change(
            ids,
            label,
            hidden,
            |p| p.hidden,
            Arc::new(|c, ids, v| c.set_hidden(ids, v)),
        )
    }

    /// Sets the caption of every photo in `ids`. Surrounding whitespace is trimmed and an empty
    /// caption clears it; more than 2000 characters is rejected.
    pub fn set_caption(&self, ids: &[PhotoId], caption: Option<&str>) -> Result<usize> {
        let caption = caption
            .map(str::trim)
            .filter(|c| !c.is_empty())
            .map(str::to_owned);
        if let Some(c) = &caption
            && c.chars().count() > MAX_CAPTION_CHARS
        {
            return Err(AppError::Invalid(format!(
                "caption is longer than {MAX_CAPTION_CHARS} characters"
            )));
        }
        self.change(
            ids,
            format!("Caption {}", noun(ids.len())),
            caption,
            |p| p.caption.clone(),
            Arc::new(|c, ids, v| c.set_caption(ids, v)),
        )
    }
}

fn noun(n: usize) -> String {
    if n == 1 {
        "1 photo".into()
    } else {
        format!("{n} photos")
    }
}
