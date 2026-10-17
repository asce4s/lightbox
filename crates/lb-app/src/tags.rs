//! Tags (SPEC §6.7.2).

use crate::error::{AppError, Result};
use crate::events::AppEvent;
use crate::history::HistoryEntry;
use crate::lightbox::Lightbox;
use lb_core::{PhotoId, Tag, TagId, TagUsage, normalize_tag_name};

/// Number of suggestions [`TagService::autocomplete`] returns.
pub const AUTOCOMPLETE_LIMIT: usize = 10;

/// Creates, assigns, renames and deletes tags.
#[derive(Clone)]
pub struct TagService {
    lb: Lightbox,
}

impl Lightbox {
    /// Tag management.
    pub fn tags(&self) -> TagService {
        TagService { lb: self.clone() }
    }
}

fn normalize_all(names: &[String]) -> Result<Vec<String>> {
    let mut out: Vec<String> = Vec::new();
    for raw in names {
        let name = normalize_tag_name(raw).map_err(|e| AppError::Invalid(e.to_string()))?;
        if !out.iter().any(|n| n.to_lowercase() == name.to_lowercase()) {
            out.push(name);
        }
    }
    Ok(out)
}

/// `(tag name, photos affected)` pairs.
type Plan = Vec<(String, Vec<PhotoId>)>;

impl TagService {
    fn plan(&self, ids: &[PhotoId], names: &[String], has_tag: bool) -> Result<Plan> {
        let catalog = self.lb.catalog();
        let mut per_photo = Vec::new();
        for &id in ids {
            let tags: Vec<String> = catalog
                .tags_for_photo(id)?
                .into_iter()
                .map(|t| t.name.to_lowercase())
                .collect();
            per_photo.push((id, tags));
        }
        Ok(names
            .iter()
            .map(|name| {
                let key = name.to_lowercase();
                let affected = per_photo
                    .iter()
                    .filter(|(_, tags)| tags.contains(&key) == has_tag)
                    .map(|(id, _)| *id)
                    .collect();
                (name.clone(), affected)
            })
            .filter(|(_, affected): &(String, Vec<PhotoId>)| !affected.is_empty())
            .collect())
    }

    fn entry(label: String, plan: Plan, add: bool) -> HistoryEntry {
        fn apply(c: &lb_catalog::Catalog, plan: &Plan, add: bool) -> Result<Vec<AppEvent>> {
            let mut touched = Vec::new();
            for (name, ids) in plan {
                if add {
                    c.add_tags(ids, std::slice::from_ref(name))?;
                } else {
                    c.remove_tags(ids, std::slice::from_ref(name))?;
                }
                touched.extend(ids.iter().copied());
            }
            touched.sort();
            touched.dedup();
            Ok(vec![
                AppEvent::TagsChanged,
                AppEvent::PhotosChanged { ids: touched },
            ])
        }
        let (undo_plan, redo_plan) = (plan.clone(), plan);
        HistoryEntry::new(
            label,
            move |c| apply(c, &undo_plan, !add),
            move |c| apply(c, &redo_plan, add),
        )
    }

    /// Adds every tag in `names` to every photo in `ids`; one undo step.
    pub fn add(&self, ids: &[PhotoId], names: &[String]) -> Result<usize> {
        let names = normalize_all(names)?;
        let plan = self.plan(ids, &names, false)?;
        let count: usize = plan.iter().map(|(_, ids)| ids.len()).sum();
        if count == 0 {
            // Still make sure the tags exist so they show up in the list.
            for n in &names {
                self.lb.catalog().get_or_create_tag(n)?;
            }
            return Ok(0);
        }
        self.lb
            .history()
            .execute(Self::entry(format!("Tag {} photos", ids.len()), plan, true))?;
        Ok(count)
    }

    /// Removes every tag in `names` from every photo in `ids`; one undo step.
    pub fn remove(&self, ids: &[PhotoId], names: &[String]) -> Result<usize> {
        let names = normalize_all(names)?;
        let plan = self.plan(ids, &names, true)?;
        let count: usize = plan.iter().map(|(_, ids)| ids.len()).sum();
        if count == 0 {
            return Ok(0);
        }
        self.lb.history().execute(Self::entry(
            format!("Untag {} photos", ids.len()),
            plan,
            false,
        ))?;
        Ok(count)
    }

    /// Tags of one photo, ordered by name.
    pub fn for_photo(&self, id: PhotoId) -> Result<Vec<Tag>> {
        Ok(self.lb.catalog().tags_for_photo(id)?)
    }

    /// Every tag with its photo count, ordered by name.
    pub fn list(&self) -> Result<Vec<TagUsage>> {
        Ok(self.lb.catalog().list_tags()?)
    }

    /// Existing tags starting with `prefix` (case-insensitive), most used first, at most 10.
    pub fn autocomplete(&self, prefix: &str) -> Result<Vec<TagUsage>> {
        let wanted = prefix.trim().to_lowercase();
        let mut matches: Vec<TagUsage> = self
            .list()?
            .into_iter()
            .filter(|u| u.tag.name.to_lowercase().starts_with(&wanted))
            .collect();
        matches.sort_by(|a, b| {
            b.count
                .cmp(&a.count)
                .then_with(|| a.tag.name.to_lowercase().cmp(&b.tag.name.to_lowercase()))
        });
        matches.truncate(AUTOCOMPLETE_LIMIT);
        Ok(matches)
    }

    /// Renames a tag. If another tag already has the new name the two merge; the surviving tag
    /// is returned. Not undoable.
    pub fn rename(&self, id: TagId, new_name: &str) -> Result<Tag> {
        let tag = self
            .lb
            .catalog()
            .rename_tag(id, new_name)
            .map_err(|e| match e {
                lb_catalog::CatalogError::InvalidTagName(i) => AppError::Invalid(i.to_string()),
                other => other.into(),
            })?;
        self.lb.emit(AppEvent::TagsChanged);
        Ok(tag)
    }

    /// Deletes a tag from every photo. Not undoable.
    pub fn delete(&self, id: TagId) -> Result<()> {
        if !self.lb.catalog().delete_tag(id)? {
            return Err(AppError::NotFound("tag"));
        }
        self.lb.emit(AppEvent::TagsChanged);
        Ok(())
    }
}
