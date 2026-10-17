//! Batch rename by pattern (SPEC §6.7.4).

use crate::error::{AppError, Result};
use crate::events::AppEvent;
use crate::photos::PhotoService;
use chrono::{DateTime, NaiveDateTime};
use lb_core::{Photo, PhotoId};
use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;
use std::path::PathBuf;

/// What one photo would be renamed to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RenamePreview {
    /// The photo.
    pub id: PhotoId,
    /// Current file name.
    pub from: String,
    /// New file name, when the pattern expanded to a valid one.
    pub to: Option<String>,
    /// Why this photo cannot be renamed.
    pub error: Option<String>,
}

/// Outcome of a batch rename.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RenameReport {
    /// Photos whose file name changed.
    pub renamed: Vec<PhotoId>,
    /// Photos that already had the target name.
    pub unchanged: Vec<PhotoId>,
    /// Photos that could not be renamed, with the reason.
    pub failed: Vec<(PhotoId, String)>,
}

fn bad(msg: impl Into<String>) -> AppError {
    AppError::Invalid(msg.into())
}

/// Expands `pattern` for one photo.
///
/// Tokens: `{name}` (file name without extension), `{n}` and `{n:03}` (the running number,
/// zero-padded to the given width), `{date}` and `{date:%Y-%m-%d}` (strftime format of the capture
/// date; default `%Y-%m-%d`). `{{` and `}}` produce literal braces.
pub fn expand_rename_pattern(
    pattern: &str,
    stem: &str,
    n: u64,
    date: NaiveDateTime,
) -> Result<String> {
    let mut out = String::new();
    let mut chars = pattern.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '{' if chars.peek() == Some(&'{') => {
                chars.next();
                out.push('{');
            }
            '}' if chars.peek() == Some(&'}') => {
                chars.next();
                out.push('}');
            }
            '}' => return Err(bad("unmatched `}` in the pattern")),
            '{' => {
                let mut token = String::new();
                loop {
                    match chars.next() {
                        Some('}') => break,
                        Some(ch) => token.push(ch),
                        None => return Err(bad("unclosed `{` in the pattern")),
                    }
                }
                let (key, arg) = match token.split_once(':') {
                    Some((k, a)) => (k, Some(a)),
                    None => (token.as_str(), None),
                };
                match (key, arg) {
                    ("name", None) => out.push_str(stem),
                    ("n", None) => {
                        let _ = write!(out, "{n}");
                    }
                    ("n", Some(width)) => {
                        let width: usize = width
                            .parse()
                            .map_err(|_| bad(format!("`{{n:{width}}}` needs a width like 03")))?;
                        if width > 12 {
                            return Err(bad("the number width is at most 12"));
                        }
                        let _ = write!(out, "{n:0width$}");
                    }
                    ("date", fmt) => {
                        let fmt = fmt.unwrap_or("%Y-%m-%d");
                        if fmt.is_empty() {
                            return Err(bad("`{date:}` needs a format"));
                        }
                        if write!(out, "{}", date.format(fmt)).is_err() {
                            return Err(bad(format!("`{fmt}` is not a valid date format")));
                        }
                    }
                    _ => return Err(bad(format!("unknown token `{{{token}}}`"))),
                }
            }
            c => out.push(c),
        }
    }
    Ok(out)
}

fn photo_date(photo: &Photo) -> NaiveDateTime {
    photo
        .taken_at
        .or_else(|| DateTime::from_timestamp(photo.file_mtime, 0).map(|d| d.naive_utc()))
        .unwrap_or_default()
}

struct Planned {
    photo: Photo,
    name: String,
    target: PathBuf,
}

/// A photo, its current file name and either its plan or the reason it has none.
type PlanRow = (PhotoId, String, std::result::Result<Planned, String>);

impl PhotoService {
    /// Plans every item; items that cannot be renamed carry their reason.
    fn plan_batch(&self, ids: &[PhotoId], pattern: &str, start: u64) -> Result<Vec<PlanRow>> {
        if ids.is_empty() {
            return Err(bad("nothing to rename"));
        }
        expand_rename_pattern(pattern, "x", start, NaiveDateTime::default())?;
        let mut rows = Vec::new();
        let mut targets: HashMap<PathBuf, usize> = HashMap::new();
        let mut sources = HashSet::new();
        for id in ids {
            if let Some(p) = self.get(*id)? {
                sources.insert(p.path);
            }
        }
        for (i, id) in ids.iter().enumerate() {
            let Some(photo) = self.get(*id)? else {
                rows.push((*id, String::new(), Err("photo not found".to_owned())));
                continue;
            };
            let from = photo
                .path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .to_owned();
            let stem = photo
                .path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default();
            let n = start + i as u64;
            let planned = expand_rename_pattern(pattern, stem, n, photo_date(&photo))
                .and_then(|new_name| {
                    let (photo, name, target) =
                        self.plan_rename_allowing(*id, &new_name, &sources)?;
                    Ok(Planned {
                        photo,
                        name,
                        target,
                    })
                })
                .map_err(|e| e.to_string());
            let planned = planned.and_then(|p| {
                if targets.insert(p.target.clone(), i).is_some() {
                    Err("another selected photo gets the same name".to_owned())
                } else {
                    Ok(p)
                }
            });
            rows.push((*id, from, planned));
        }
        Ok(rows)
    }

    /// Shows what [`Self::batch_rename`] would do. The first photo gets `start`, the next
    /// `start + 1`, and so on, in the order of `ids`.
    pub fn preview_batch_rename(
        &self,
        ids: &[PhotoId],
        pattern: &str,
        start: u64,
    ) -> Result<Vec<RenamePreview>> {
        Ok(self
            .plan_batch(ids, pattern, start)?
            .into_iter()
            .map(|(id, from, planned)| match planned {
                Ok(p) => RenamePreview {
                    id,
                    from,
                    to: Some(p.name),
                    error: None,
                },
                Err(e) => RenamePreview {
                    id,
                    from,
                    to: None,
                    error: Some(e),
                },
            })
            .collect())
    }

    /// Renames the files of `ids` by `pattern`. Names that collide with files outside the batch
    /// or with each other are reported per photo and skipped; names that only swap places inside
    /// the batch are allowed.
    pub fn batch_rename(&self, ids: &[PhotoId], pattern: &str, start: u64) -> Result<RenameReport> {
        let rows = self.plan_batch(ids, pattern, start)?;
        let mut report = RenameReport::default();
        let mut work = Vec::new();
        for (id, _, planned) in rows {
            match planned {
                Ok(p) if p.target == p.photo.path => report.unchanged.push(id),
                Ok(p) => work.push(p),
                Err(e) => report.failed.push((id, e)),
            }
        }
        let sources: HashSet<PathBuf> = work.iter().map(|p| p.photo.path.clone()).collect();
        let (late, direct): (Vec<_>, Vec<_>) =
            work.into_iter().partition(|p| sources.contains(&p.target));

        let mut parked = Vec::new();
        for p in late {
            let tmp_name = format!(".lbrename-{}.tmp", p.photo.id.get());
            let tmp = p.photo.path.with_file_name(&tmp_name);
            if let Err(e) = std::fs::rename(&p.photo.path, &tmp) {
                report.failed.push((p.photo.id, e.to_string()));
                continue;
            }
            // The catalog keeps (folder, name) unique, so the row steps aside as well.
            let moved = self.lb.catalog().relocate_photo(
                p.photo.id,
                p.photo.folder_id,
                &tmp_name,
                self.lb.clock().now(),
            );
            match moved {
                Ok(()) => parked.push((p, tmp)),
                Err(e) => {
                    let _ = std::fs::rename(&tmp, &p.photo.path);
                    report.failed.push((p.photo.id, e.to_string()));
                }
            }
        }
        for p in direct {
            self.finish_rename(&p, &p.photo.path, &mut report);
        }
        for (p, tmp) in parked {
            self.finish_rename(&p, &tmp, &mut report);
        }
        if !report.renamed.is_empty() {
            self.lb.emit(AppEvent::PhotosChanged {
                ids: report.renamed.clone(),
            });
        }
        Ok(report)
    }

    /// Moves `current` (the file's present location) to the planned target and updates the
    /// catalog; on failure the file goes back to its original path.
    fn finish_rename(&self, p: &Planned, current: &std::path::Path, report: &mut RenameReport) {
        let id = p.photo.id;
        let parked = current != p.photo.path;
        let original_name = p
            .photo
            .path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let restore = |from: &std::path::Path| {
            let _ = std::fs::rename(from, &p.photo.path);
            if parked {
                let _ = self.lb.catalog().relocate_photo(
                    id,
                    p.photo.folder_id,
                    original_name,
                    self.lb.clock().now(),
                );
            }
        };
        if let Err(e) = std::fs::rename(current, &p.target) {
            restore(current);
            report.failed.push((id, e.to_string()));
            return;
        }
        let moved =
            self.lb
                .catalog()
                .relocate_photo(id, p.photo.folder_id, &p.name, self.lb.clock().now());
        if let Err(e) = moved {
            restore(&p.target);
            report.failed.push((id, e.to_string()));
            return;
        }
        report.renamed.push(id);
    }
}
