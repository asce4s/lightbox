//! Search, filter chips and the grouped result the grid renders (SPEC §6.8, §7.2).

use crate::error::Result;
use crate::lightbox::Lightbox;
use crate::query::ViewSource;
use chrono::{Datelike, NaiveDate};
use lb_catalog::{PhotoQuery, QueryRow, SqlFragment};
use lb_core::{FolderId, GroupBy, PhotoId, SortOrder};
use lb_search::QueryPlan;
use std::collections::HashMap;

/// Toggle chips of the filter bar; they combine with the search text by `AND`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilterChips {
    /// Only starred photos.
    pub starred: bool,
    /// Only videos.
    pub video: bool,
    /// Only photos with faces.
    pub faces: bool,
    /// Only geotagged photos.
    pub geotagged: bool,
    /// Only edited photos.
    pub edited: bool,
    /// Minimum rating.
    pub min_rating: Option<u8>,
    /// Inclusive date range; either side may be open.
    pub date: Option<(Option<NaiveDate>, Option<NaiveDate>)>,
}

impl FilterChips {
    /// True when no chip is on.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }

    /// The equivalent query text (shown in the chip tooltip and appended to the search string).
    pub fn to_query_text(&self) -> String {
        let mut parts: Vec<String> = Vec::new();
        if self.starred {
            parts.push("is:starred".into());
        }
        if self.video {
            parts.push("type:video".into());
        }
        if self.faces {
            parts.push("has:faces".into());
        }
        if self.geotagged {
            parts.push("has:gps".into());
        }
        if self.edited {
            parts.push("is:edited".into());
        }
        if let Some(n) = self.min_rating {
            parts.push(format!("rating:>={n}"));
        }
        if let Some((from, to)) = self.date {
            let fmt = |d: Option<NaiveDate>| d.map(|d| d.to_string()).unwrap_or_default();
            if from.is_some() || to.is_some() {
                parts.push(format!("date:{}..{}", fmt(from), fmt(to)));
            }
        }
        parts.join(" ")
    }
}

/// Everything that decides which photos the grid shows and how they are arranged.
#[derive(Debug, Clone, PartialEq)]
pub struct ViewQuery {
    /// Where photos come from.
    pub source: ViewSource,
    /// Search box text.
    pub search: String,
    /// Filter chips.
    pub chips: FilterChips,
    /// Header grouping.
    pub group_by: GroupBy,
    /// Order within the result.
    pub sort: SortOrder,
    /// Include hidden photos.
    pub show_hidden: bool,
}

impl ViewQuery {
    /// All photos of `source`, grouped by month, newest first.
    pub fn new(source: ViewSource) -> Self {
        Self {
            source,
            search: String::new(),
            chips: FilterChips::default(),
            group_by: GroupBy::Month,
            sort: SortOrder::DateTakenDesc,
            show_hidden: false,
        }
    }

    /// The search text followed by the chips' equivalent text.
    pub fn effective_text(&self) -> String {
        let chips = self.chips.to_query_text();
        match (self.search.trim(), chips.is_empty()) {
            (s, true) => s.to_owned(),
            ("", false) => chips,
            (s, false) => format!("{s} {chips}"),
        }
    }
}

/// What a group header stands for; stable across refreshes so scroll anchors survive.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GroupKey {
    /// One folder.
    Folder(FolderId),
    /// A calendar month.
    Month {
        /// Year.
        year: i32,
        /// Month, 1 to 12.
        month: u32,
    },
    /// A calendar year.
    Year(i32),
    /// Ungrouped.
    All,
}

/// A header and the photos below it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// Stable identity.
    pub key: GroupKey,
    /// Header text.
    pub title: String,
    /// Secondary header text.
    pub subtitle: String,
    /// Photos in display order.
    pub ids: Vec<PhotoId>,
}

/// Ids and headers for the grid; rows are fetched lazily with `photos().get_many`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PhotoQueryResult {
    /// Groups in display order.
    pub groups: Vec<Group>,
    /// Number of photos across all groups.
    pub total: usize,
    /// Calendar months of the result for the timeline scrubber (SPEC §9.3).
    pub timeline: Vec<MonthBucket>,
}

/// One calendar month on the timeline scrubber.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MonthBucket {
    /// Year.
    pub year: i32,
    /// Month, 1 to 12.
    pub month: u32,
    /// Photos taken in the month.
    pub count: usize,
    /// The month's photo that comes first in display order; dragging to the month scrolls here.
    pub first: PhotoId,
}

impl PhotoQueryResult {
    /// Every id in display order.
    pub fn ids(&self) -> Vec<PhotoId> {
        self.groups
            .iter()
            .flat_map(|g| g.ids.iter().copied())
            .collect()
    }
}

/// Parses search text and runs view queries.
#[derive(Clone)]
pub struct SearchService {
    lb: Lightbox,
}

impl Lightbox {
    /// Search and grid queries.
    pub fn search(&self) -> SearchService {
        SearchService { lb: self.clone() }
    }
}

fn photo_count(n: usize) -> String {
    if n == 1 {
        "1 photo".into()
    } else {
        format!("{n} photos")
    }
}

fn month_title(year: i32, month: u32) -> String {
    NaiveDate::from_ymd_opt(year, month, 1)
        .map(|d| d.format("%B %Y").to_string())
        .unwrap_or_else(|| format!("{year}-{month:02}"))
}

impl SearchService {
    /// Parses `text` without running it, for live diagnostics.
    pub fn parse(&self, text: &str) -> QueryPlan {
        lb_search::parse(text)
    }

    /// Runs `view` and groups the result.
    pub fn query(&self, view: &ViewQuery) -> Result<PhotoQueryResult> {
        let plan = lb_search::parse(&view.effective_text());
        let mut q = PhotoQuery::new(view.source.to_catalog());
        q.show_hidden = view.show_hidden || plan.include_hidden;
        q.show_missing = plan.include_missing;
        q.sort = view.sort;
        if !plan.sql_where.is_empty() {
            q.extra = Some(SqlFragment {
                sql: plan.sql_where,
                params: plan.params,
            });
        }
        q.fts = plan.fts;
        let rows = self.lb.catalog().query_photos(&q)?;
        self.group(rows, view.group_by)
    }

    fn group(&self, rows: Vec<QueryRow>, by: GroupBy) -> Result<PhotoQueryResult> {
        let total = rows.len();
        let dates: HashMap<PhotoId, (i32, u32)> = rows
            .iter()
            .map(|r| (r.id, (r.date.year(), r.date.month())))
            .collect();
        let mut groups: Vec<Group> = Vec::new();
        let mut index: HashMap<GroupKey, usize> = HashMap::new();
        for row in rows {
            let key = match by {
                GroupBy::Folder => GroupKey::Folder(row.folder_id),
                GroupBy::Month => GroupKey::Month {
                    year: row.date.year(),
                    month: row.date.month(),
                },
                GroupBy::Year => GroupKey::Year(row.date.year()),
                GroupBy::None => GroupKey::All,
            };
            let slot = *index.entry(key).or_insert_with(|| {
                groups.push(Group {
                    key,
                    title: String::new(),
                    subtitle: String::new(),
                    ids: Vec::new(),
                });
                groups.len() - 1
            });
            groups[slot].ids.push(row.id);
        }
        for g in &mut groups {
            let count = photo_count(g.ids.len());
            match g.key {
                GroupKey::Folder(id) => {
                    let folder = self.lb.catalog().get_folder(id)?;
                    g.title = folder
                        .as_ref()
                        .map_or_else(|| "Unknown folder".into(), |f| f.name.clone());
                    g.subtitle = folder.map_or(count, |f| f.path.display().to_string());
                }
                GroupKey::Month { year, month } => {
                    g.title = month_title(year, month);
                    g.subtitle = count;
                }
                GroupKey::Year(year) => {
                    g.title = year.to_string();
                    g.subtitle = count;
                }
                GroupKey::All => {
                    g.title = "All photos".into();
                    g.subtitle = count;
                }
            }
        }
        let timeline = timeline_of(&groups, &dates);
        Ok(PhotoQueryResult {
            groups,
            total,
            timeline,
        })
    }
}

/// Months in the order their first photo appears in the grid.
fn timeline_of(groups: &[Group], dates: &HashMap<PhotoId, (i32, u32)>) -> Vec<MonthBucket> {
    let mut buckets: Vec<MonthBucket> = Vec::new();
    let mut slot: HashMap<(i32, u32), usize> = HashMap::new();
    for id in groups.iter().flat_map(|g| g.ids.iter()) {
        let Some(&(year, month)) = dates.get(id) else {
            continue;
        };
        match slot.get(&(year, month)) {
            Some(&i) => buckets[i].count += 1,
            None => {
                slot.insert((year, month), buckets.len());
                buckets.push(MonthBucket {
                    year,
                    month,
                    count: 1,
                    first: *id,
                });
            }
        }
    }
    buckets
}
