//! Import of legacy `.picasa.ini` metadata (SPEC §6.17).

use crate::error::{AppError, Result};
use crate::events::AppEvent;
use crate::lightbox::Lightbox;
use lb_core::{Photo, PhotoId};
use lb_picasa::{FaceRef, FileEntry, PICASA_INI_NAMES, PicasaIni, parse_picasa_ini};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

/// Two face boxes closer than this (per coordinate, normalized) count as the same face.
const SAME_FACE_EPSILON: f32 = 0.002;

/// How an import treats values Lightbox already has.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct PicasaOptions {
    /// Replace existing captions, edits, folder captions and imported faces. Without it only
    /// empty values are filled and nothing is removed.
    pub overwrite: bool,
}

/// What an import changed (or, for a preview, would change).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PicasaReport {
    /// `.picasa.ini` files read.
    pub ini_files: usize,
    /// Photos the files refer to that are in the library.
    pub photos_matched: usize,
    /// Entries naming files that are not in the library.
    pub photos_missing: usize,
    /// Photos newly starred.
    pub starred: usize,
    /// Captions set.
    pub captions: usize,
    /// Photo-tag links added.
    pub tags_added: usize,
    /// Albums created.
    pub albums_created: usize,
    /// Photos added to albums.
    pub album_links: usize,
    /// Edit stacks set.
    pub edits: usize,
    /// Faces added.
    pub faces: usize,
    /// Folder captions set.
    pub folder_captions: usize,
    /// Filters that could not be mapped to an edit.
    pub skipped_filters: Vec<String>,
}

impl PicasaReport {
    /// Whether anything changed.
    pub fn is_empty(&self) -> bool {
        self.starred
            + self.captions
            + self.tags_added
            + self.albums_created
            + self.album_links
            + self.edits
            + self.faces
            + self.folder_captions
            == 0
    }
}

/// Reads `.picasa.ini` files and applies them to the catalog.
#[derive(Clone)]
pub struct PicasaImportService {
    lb: Lightbox,
}

impl Lightbox {
    /// Legacy Picasa metadata import.
    pub fn picasa(&self) -> PicasaImportService {
        PicasaImportService { lb: self.clone() }
    }
}

fn ini_in(dir: &Path) -> Option<PathBuf> {
    PICASA_INI_NAMES
        .iter()
        .map(|n| dir.join(n))
        .find(|p| p.is_file())
}

fn same_face(a: &lb_core::NormRect, b: &lb_core::NormRect) -> bool {
    (a.x - b.x).abs() < SAME_FACE_EPSILON
        && (a.y - b.y).abs() < SAME_FACE_EPSILON
        && (a.w - b.w).abs() < SAME_FACE_EPSILON
        && (a.h - b.h).abs() < SAME_FACE_EPSILON
}

#[derive(Default)]
struct Touched {
    photos: BTreeSet<PhotoId>,
    albums: bool,
    tags: bool,
    people: bool,
    folders: bool,
}

impl PicasaImportService {
    /// Counts what [`PicasaImportService::import`] would change without changing anything.
    pub fn preview(&self, folder: &Path, options: PicasaOptions) -> Result<PicasaReport> {
        self.run(folder, options, false)
    }

    /// Imports the `.picasa.ini` of `folder` and of every folder below it. Running it again
    /// changes nothing.
    pub fn import(&self, folder: &Path, options: PicasaOptions) -> Result<PicasaReport> {
        self.run(folder, options, true)
    }

    fn run(&self, folder: &Path, options: PicasaOptions, apply: bool) -> Result<PicasaReport> {
        if !folder.is_dir() {
            return Err(AppError::Invalid(format!(
                "{} is not a folder",
                folder.display()
            )));
        }
        let mut report = PicasaReport::default();
        let mut touched = Touched::default();
        let mut dirs: Vec<PathBuf> = walkdir::WalkDir::new(folder)
            .into_iter()
            .filter_entry(|e| e.file_name() != ".picasaoriginals")
            .filter_map(|e| e.ok())
            .filter(|e| e.file_type().is_dir())
            .map(walkdir::DirEntry::into_path)
            .collect();
        dirs.sort();
        let mut created_albums: BTreeMap<String, Option<lb_core::AlbumId>> = BTreeMap::new();
        for dir in dirs {
            let Some(ini_path) = ini_in(&dir) else {
                continue;
            };
            let text = match std::fs::read(&ini_path) {
                Ok(bytes) => String::from_utf8_lossy(&bytes).into_owned(),
                Err(e) => {
                    tracing::warn!(path = %ini_path.display(), error = %e, "unreadable picasa ini skipped");
                    continue;
                }
            };
            report.ini_files += 1;
            let ini = parse_picasa_ini(&text);
            self.apply_folder(
                &dir,
                &ini,
                options,
                apply,
                &mut report,
                &mut touched,
                &mut created_albums,
            )?;
        }
        if apply {
            self.announce(&touched);
        }
        report.skipped_filters.sort();
        report.skipped_filters.dedup();
        Ok(report)
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_folder(
        &self,
        dir: &Path,
        ini: &PicasaIni,
        options: PicasaOptions,
        apply: bool,
        report: &mut PicasaReport,
        touched: &mut Touched,
        albums: &mut BTreeMap<String, Option<lb_core::AlbumId>>,
    ) -> Result<()> {
        let catalog = self.lb.catalog();
        if let Some(description) = &ini.folder.description
            && let Some(folder) = catalog.folder_by_path(dir)?
        {
            let current = folder.caption.as_deref().map(str::trim).unwrap_or("");
            let wanted = current.is_empty() || (options.overwrite && current != description);
            if wanted && folder.caption.as_deref() != Some(description.as_str()) {
                report.folder_captions += 1;
                if apply {
                    catalog.set_folder_caption(folder.id, Some(description.clone()))?;
                    touched.folders = true;
                }
            }
        }
        let listing: BTreeMap<String, String> = std::fs::read_dir(dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter_map(|e| e.file_name().into_string().ok())
                    .map(|n| (n.to_lowercase(), n))
                    .collect()
            })
            .unwrap_or_default();
        for entry in &ini.files {
            let real = listing
                .get(&entry.name.to_lowercase())
                .map_or(entry.name.as_str(), String::as_str);
            let photo = catalog
                .photo_by_path(&dir.join(real))?
                .filter(|p| p.status != lb_core::PhotoStatus::Missing);
            let Some(photo) = photo else {
                report.photos_missing += 1;
                continue;
            };
            report.photos_matched += 1;
            report
                .skipped_filters
                .extend(entry.skipped_filters.iter().cloned());
            self.apply_entry(&photo, entry, ini, options, apply, report, touched, albums)?;
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn apply_entry(
        &self,
        photo: &Photo,
        entry: &FileEntry,
        ini: &PicasaIni,
        options: PicasaOptions,
        apply: bool,
        report: &mut PicasaReport,
        touched: &mut Touched,
        albums: &mut BTreeMap<String, Option<lb_core::AlbumId>>,
    ) -> Result<()> {
        let catalog = self.lb.catalog();
        let id = photo.id;
        let mut changed = false;

        if entry.star && !photo.starred {
            report.starred += 1;
            changed = true;
            if apply {
                catalog.set_starred(&[id], true)?;
            }
        }

        if let Some(caption) = &entry.caption {
            let current = photo.caption.as_deref().map(str::trim).unwrap_or("");
            if (current.is_empty()
                || (options.overwrite && photo.caption.as_deref() != Some(caption)))
                && photo.caption.as_deref() != Some(caption)
            {
                report.captions += 1;
                changed = true;
                if apply {
                    catalog.set_caption(&[id], Some(caption.clone()))?;
                }
            }
        }

        if !entry.keywords.is_empty() {
            let have: BTreeSet<String> = catalog
                .tags_for_photo(id)?
                .into_iter()
                .map(|t| t.name.to_lowercase())
                .collect();
            let fresh: Vec<String> = entry
                .keywords
                .iter()
                .filter(|k| !have.contains(&k.to_lowercase()))
                .cloned()
                .collect();
            if !fresh.is_empty() {
                let added = if apply {
                    catalog.add_tags(&[id], &fresh)?
                } else {
                    fresh.len()
                };
                report.tags_added += added;
                if added > 0 {
                    changed = true;
                    touched.tags = true;
                }
            }
        }

        for token in &entry.albums {
            let Some(def) = ini.albums.iter().find(|a| a.token == *token) else {
                continue;
            };
            let key = def.name.to_lowercase();
            if !albums.contains_key(&key) {
                let existing = catalog
                    .list_albums()?
                    .into_iter()
                    .find(|a| a.name.to_lowercase() == key);
                let album = match existing {
                    Some(a) => Some(a.id),
                    None => {
                        report.albums_created += 1;
                        if apply {
                            touched.albums = true;
                            Some(
                                catalog
                                    .create_album(&def.name, None, self.lb.clock().now())?
                                    .id,
                            )
                        } else {
                            None
                        }
                    }
                };
                albums.insert(key.clone(), album);
            }
            match albums.get(&key).copied().flatten() {
                Some(album) => {
                    let present = catalog.album_photo_ids(album)?.contains(&id);
                    if !present {
                        report.album_links += 1;
                        changed = true;
                        if apply {
                            catalog.add_to_album(album, &[id])?;
                            touched.albums = true;
                        }
                    }
                }
                None => {
                    report.album_links += 1;
                    changed = true;
                }
            }
        }

        if !entry.ops.is_empty() {
            let current = self.lb.edits().stack(id)?;
            let validated: Vec<_> = entry
                .ops
                .iter()
                .filter_map(|op| op.validate().ok())
                .collect();
            if (current.ops.is_empty() || options.overwrite) && current.ops != validated {
                report.edits += 1;
                changed = true;
                if apply {
                    self.lb.edits().replace_stack(id, validated)?;
                }
            }
        }

        if !entry.faces.is_empty() {
            changed |= self.apply_faces(id, &entry.faces, options, apply, report, touched)?;
        }

        if changed {
            touched.photos.insert(id);
        }
        Ok(())
    }

    fn apply_faces(
        &self,
        photo: PhotoId,
        faces: &[FaceRef],
        options: PicasaOptions,
        apply: bool,
        report: &mut PicasaReport,
        touched: &mut Touched,
    ) -> Result<bool> {
        let catalog = self.lb.catalog();
        let existing: Vec<_> = catalog
            .faces_of_photo(photo)?
            .into_iter()
            .filter(|f| f.source == lb_core::FaceSource::Picasa)
            .collect();
        let named = |face: &FaceRef| -> Result<Option<lb_core::PersonId>> {
            face.person
                .as_deref()
                .map(|n| catalog.find_person(n))
                .transpose()
                .map(|p| p.flatten().map(|p| p.id))
                .map_err(Into::into)
        };
        let identical = existing.len() == faces.len()
            && faces.iter().all(|f| {
                existing.iter().any(|e| {
                    same_face(&e.rect, &f.rect)
                        && match (&f.person, e.person_id) {
                            (None, None) => true,
                            (Some(_), Some(_)) => named(f).ok().flatten() == e.person_id,
                            _ => false,
                        }
                })
            });
        if identical {
            return Ok(false);
        }
        let mut changed = false;
        let keep: Vec<_> = if options.overwrite {
            if apply {
                catalog.remove_picasa_faces(photo)?;
            }
            Vec::new()
        } else {
            existing
        };
        for face in faces {
            if keep.iter().any(|e| same_face(&e.rect, &face.rect)) {
                continue;
            }
            report.faces += 1;
            changed = true;
            if apply {
                let person = face
                    .person
                    .as_deref()
                    .map(|n| catalog.get_or_create_person(n))
                    .transpose()?
                    .map(|p| p.id);
                catalog.add_picasa_face(photo, face.rect, person)?;
                touched.people = true;
            }
        }
        Ok(changed)
    }

    fn announce(&self, touched: &Touched) {
        if !touched.photos.is_empty() {
            self.lb.emit(AppEvent::PhotosChanged {
                ids: touched.photos.iter().copied().collect(),
            });
        }
        if touched.albums {
            self.lb.emit(AppEvent::AlbumsChanged);
        }
        if touched.tags {
            self.lb.emit(AppEvent::TagsChanged);
        }
        if touched.people {
            self.lb.emit(AppEvent::PeopleChanged);
        }
        if touched.folders {
            self.lb.emit(AppEvent::FoldersChanged);
        }
    }
}
