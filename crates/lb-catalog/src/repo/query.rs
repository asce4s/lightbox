//! Photo listing for the grid: source, visibility, search fragment and ordering (SPEC §7.2).
//!
//! The planner in `lb-search` supplies `extra` (a `WHERE` fragment with bound parameters) and
//! `fts` (an FTS5 `MATCH` expression); both are executed here so no SQL leaves this crate.

use super::dt_from_db;
use crate::{Catalog, error::Result};
use chrono::NaiveDateTime;
use lb_core::{AlbumId, FolderId, PhotoId, SortOrder, SqlValue};
use rusqlite::types::Value;

/// The set of photos a query starts from.
#[derive(Debug, Clone, PartialEq)]
pub enum QuerySource {
    /// Every photo.
    All,
    /// Photos directly inside one folder.
    Folder(FolderId),
    /// Members of an album, in album order (the `sort` field is ignored).
    Album(AlbumId),
    /// Starred photos.
    Starred,
    /// Photos that have an edit stack, most recently edited first (the `sort` field is ignored).
    RecentlyEdited,
    /// An explicit list of photos (tray, duplicates, imports, people).
    Ids(Vec<PhotoId>),
}

/// A `WHERE` fragment over `photos p` and `folders f` with its bound parameters. Placeholders
/// are anonymous `?`.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SqlFragment {
    /// Boolean SQL expression.
    pub sql: String,
    /// Values bound to the `?` placeholders, in order.
    pub params: Vec<SqlValue>,
}

/// What to list.
#[derive(Debug, Clone, PartialEq)]
pub struct PhotoQuery {
    /// Starting set.
    pub source: QuerySource,
    /// Include photos flagged hidden.
    pub show_hidden: bool,
    /// Include photos whose file is missing.
    pub show_missing: bool,
    /// Planner output narrowing the source.
    pub extra: Option<SqlFragment>,
    /// FTS5 `MATCH` expression produced by the planner.
    pub fts: Option<String>,
    /// Order of the result.
    pub sort: SortOrder,
}

impl PhotoQuery {
    /// Everything visible by default (no hidden, no missing), newest first.
    pub fn new(source: QuerySource) -> Self {
        Self {
            source,
            show_hidden: false,
            show_missing: false,
            extra: None,
            fts: None,
            sort: SortOrder::DateTakenDesc,
        }
    }
}

/// One result row: just what grouping needs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct QueryRow {
    /// The photo.
    pub id: PhotoId,
    /// Its folder.
    pub folder_id: FolderId,
    /// Capture time, or the file's modification time when the capture time is unknown.
    pub date: NaiveDateTime,
}

const DATE_EXPR: &str = lb_core::PHOTO_DATE_SQL;

fn to_value(v: &SqlValue) -> Value {
    match v {
        SqlValue::Int(i) => Value::Integer(*i),
        SqlValue::Real(r) => Value::Real(*r),
        SqlValue::Text(t) => Value::Text(t.clone()),
    }
}

fn order_by(q: &PhotoQuery) -> String {
    match (&q.source, q.sort) {
        (QuerySource::Album(_), _) => "ai.position, p.id".into(),
        (QuerySource::RecentlyEdited, _) => "e.updated_at DESC, p.id DESC".into(),
        (_, SortOrder::DateTakenAsc) => {
            format!("{DATE_EXPR} ASC, p.file_name COLLATE NOCASE, p.id")
        }
        (_, SortOrder::DateTakenDesc) => {
            format!("{DATE_EXPR} DESC, p.file_name COLLATE NOCASE, p.id")
        }
        (_, SortOrder::FileNameAsc) => "p.file_name COLLATE NOCASE, f.path, p.id".into(),
        (_, SortOrder::FileSizeDesc) => "p.file_size DESC, p.id".into(),
        (_, SortOrder::ModifiedDesc) => "p.file_mtime DESC, p.id".into(),
    }
}

impl Catalog {
    /// Lists photos matching `query` in its order.
    pub fn query_photos(&self, query: &PhotoQuery) -> Result<Vec<QueryRow>> {
        let mut params: Vec<Value> = Vec::new();
        let mut joins = String::new();
        let mut wheres: Vec<String> = Vec::new();
        match &query.source {
            QuerySource::All => {}
            QuerySource::Folder(id) => {
                wheres.push("p.folder_id = ?".into());
                params.push(Value::Integer(id.get()));
            }
            QuerySource::Album(id) => {
                joins.push_str(" JOIN album_items ai ON ai.photo_id = p.id AND ai.album_id = ?");
                params.push(Value::Integer(id.get()));
            }
            QuerySource::Starred => wheres.push("p.starred = 1".into()),
            QuerySource::RecentlyEdited => {
                joins.push_str(" JOIN edits e ON e.photo_id = p.id");
            }
            QuerySource::Ids(ids) => {
                wheres.push("p.id IN (SELECT value FROM json_each(?))".into());
                let json = format!(
                    "[{}]",
                    ids.iter()
                        .map(|i| i.get().to_string())
                        .collect::<Vec<_>>()
                        .join(",")
                );
                params.push(Value::Text(json));
            }
        }
        if !query.show_hidden {
            wheres.push("p.hidden = 0".into());
        }
        if !query.show_missing {
            wheres.push("p.status <> 'missing'".into());
        }
        if let Some(extra) = &query.extra {
            wheres.push(format!("({})", extra.sql));
            params.extend(extra.params.iter().map(to_value));
        }
        if let Some(fts) = &query.fts {
            wheres.push("p.id IN (SELECT rowid FROM photos_fts WHERE photos_fts MATCH ?)".into());
            params.push(Value::Text(fts.clone()));
        }
        let where_sql = if wheres.is_empty() {
            String::new()
        } else {
            format!(" WHERE {}", wheres.join(" AND "))
        };
        let sql = format!(
            "SELECT p.id, p.folder_id, {DATE_EXPR} FROM photos p JOIN folders f ON f.id = p.folder_id{joins}{where_sql} ORDER BY {}",
            order_by(query)
        );
        self.read(|c| {
            let mut stmt = c.prepare(&sql)?;
            let rows = stmt.query_map(rusqlite::params_from_iter(params), |r| {
                Ok(QueryRow {
                    id: PhotoId::new(r.get(0)?),
                    folder_id: FolderId::new(r.get(1)?),
                    date: dt_from_db(2, &r.get::<_, String>(2)?)?,
                })
            })?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
    }

    /// Re-inserts album members with exact positions (used to undo a removal).
    pub fn restore_album_items(&self, album: AlbumId, items: &[(PhotoId, f64)]) -> Result<usize> {
        let items = items.to_vec();
        self.write_tx(move |tx| {
            let mut insert = tx.prepare_cached("INSERT OR IGNORE INTO album_items (album_id, photo_id, position) VALUES (?1, ?2, ?3)")?;
            let mut added = 0;
            for (photo, pos) in &items {
                added += insert.execute((album.get(), photo.get(), pos))?;
            }
            drop(insert);
            let ids: Vec<PhotoId> = items.iter().map(|(p, _)| *p).collect();
            crate::fts::refresh_photos(tx, &ids)?;
            Ok(added)
        })
    }
}
