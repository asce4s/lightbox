//! Compiles lexed terms into a [`QueryPlan`] (SPEC §6.8.1).
//!
//! User text never reaches the SQL string: filters produce `?` placeholders with values in
//! [`QueryPlan::params`], and bare words are quoted into an FTS5 expression that the catalog binds
//! as a single parameter.

use crate::lex::{Term, lex};
use chrono::{Datelike, NaiveDate};
use lb_core::{PHOTO_DATE_SQL, SqlValue};
use std::ops::Range;

/// A problem found while parsing, for the UI to underline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Diag {
    /// Byte range of the offending term in the query string.
    pub span: Range<usize>,
    /// Human-readable explanation.
    pub message: String,
}

/// The executable form of a search string.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct QueryPlan {
    /// Boolean SQL over `photos p` and `folders f` with anonymous `?` placeholders; empty when the
    /// query has no filters. Terms are joined with `AND`.
    pub sql_where: String,
    /// Values for the placeholders in `sql_where`, in order.
    pub params: Vec<SqlValue>,
    /// FTS5 `MATCH` expression for the positive bare words and phrases.
    pub fts: Option<String>,
    /// Problems the UI should show; the plan is still usable.
    pub diagnostics: Vec<Diag>,
    /// The query asks for hidden photos (`is:hidden`), so the caller must not exclude them.
    pub include_hidden: bool,
    /// The query asks for missing files (`is:missing`), so the caller must not exclude them.
    pub include_missing: bool,
}

impl QueryPlan {
    /// True when the plan does not narrow anything.
    pub fn is_empty(&self) -> bool {
        self.sql_where.is_empty() && self.fts.is_none()
    }
}

#[derive(Default)]
struct Builder {
    clauses: Vec<String>,
    params: Vec<SqlValue>,
    fts: Vec<String>,
    diagnostics: Vec<Diag>,
    include_hidden: bool,
    include_missing: bool,
}

/// What a filter compiled to.
struct Clause {
    sql: String,
    params: Vec<SqlValue>,
}

impl Clause {
    fn new(sql: &str, params: Vec<SqlValue>) -> Self {
        Self {
            sql: sql.to_owned(),
            params,
        }
    }
    fn bare(sql: &str) -> Self {
        Self::new(sql, Vec::new())
    }
}

/// Parses `src` into a plan. Never fails; unknown keys become plain words and malformed filters
/// are dropped with a diagnostic.
pub fn parse(src: &str) -> QueryPlan {
    let mut b = Builder::default();
    for term in lex(src) {
        b.term(&term);
    }
    QueryPlan {
        sql_where: b.clauses.join(" AND "),
        params: b.params,
        fts: if b.fts.is_empty() {
            None
        } else {
            Some(b.fts.join(" "))
        },
        diagnostics: b.diagnostics,
        include_hidden: b.include_hidden,
        include_missing: b.include_missing,
    }
}

const KNOWN_KEYS: &[&str] = &[
    "tag", "album", "folder", "camera", "lens", "person", "type", "ext", "caption", "name",
    "rating", "date", "year", "month", "starred", "hidden", "has", "is",
];

fn like_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        if matches!(c, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn like_contains(text: &str) -> SqlValue {
    SqlValue::Text(format!("%{}%", like_escape(text)))
}

fn text(s: &str) -> SqlValue {
    SqlValue::Text(s.to_owned())
}

/// Quotes `word` as an FTS5 string; `prefix` adds a trailing `*`. Returns `None` when the text has
/// no letter or digit and therefore nothing to index.
fn fts_string(word: &str, prefix: bool) -> Option<String> {
    if !word.chars().any(char::is_alphanumeric) {
        return None;
    }
    let mut out = String::with_capacity(word.len() + 4);
    out.push('"');
    for c in word.chars() {
        if c == '"' {
            out.push('"');
        }
        out.push(c);
    }
    out.push('"');
    if prefix {
        out.push('*');
    }
    Some(out)
}

fn parse_bool(value: &str) -> Option<bool> {
    match value.to_ascii_lowercase().as_str() {
        "yes" | "true" | "1" | "on" => Some(true),
        "no" | "false" | "0" | "off" => Some(false),
        _ => None,
    }
}

/// Splits a leading comparison operator off `value`.
fn comparison(value: &str) -> (&'static str, &str) {
    for (token, op) in [
        (">=", ">="),
        ("<=", "<="),
        (">", ">"),
        ("<", "<"),
        ("=", "="),
    ] {
        if let Some(rest) = value.strip_prefix(token) {
            return (op, rest);
        }
    }
    ("=", value)
}

fn parse_int(s: &str) -> Option<i64> {
    if s.is_empty() || s.len() > 9 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

/// Inclusive start and exclusive end (as ISO strings) of a `YYYY`, `YYYY-MM` or `YYYY-MM-DD`.
fn date_bounds(spec: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = spec.split('-').collect();
    let num = |s: &str, len: usize| {
        if s.len() == len { parse_int(s) } else { None }
    };
    let year = i32::try_from(num(parts.first()?, 4)?).ok()?;
    let (start, end) = match parts.len() {
        1 => (
            NaiveDate::from_ymd_opt(year, 1, 1)?,
            NaiveDate::from_ymd_opt(year + 1, 1, 1),
        ),
        2 => {
            let month = u32::try_from(num(parts[1], 2)?).ok()?;
            let start = NaiveDate::from_ymd_opt(year, month, 1)?;
            let end = if month == 12 {
                NaiveDate::from_ymd_opt(year + 1, 1, 1)
            } else {
                NaiveDate::from_ymd_opt(year, month + 1, 1)
            };
            (start, end)
        }
        3 => {
            let month = u32::try_from(num(parts[1], 2)?).ok()?;
            let day = u32::try_from(num(parts[2], 2)?).ok()?;
            let start = NaiveDate::from_ymd_opt(year, month, day)?;
            (start, start.succ_opt())
        }
        _ => return None,
    };
    let fmt = |d: NaiveDate| format!("{:04}-{:02}-{:02}T00:00:00", d.year(), d.month(), d.day());
    Some((fmt(start), end.map_or_else(|| "~".to_owned(), fmt)))
}

fn month_number(value: &str) -> Option<i64> {
    if let Some(n) = parse_int(value) {
        return (1..=12).contains(&n).then_some(n);
    }
    const NAMES: [&str; 12] = [
        "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
    ];
    let lower = value.to_ascii_lowercase();
    let idx = NAMES.iter().position(|n| lower.starts_with(n))?;
    let full = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    (lower == NAMES[idx] || lower == full[idx]).then(|| idx as i64 + 1)
}

fn date_clause(value: &str) -> Option<Clause> {
    let d = PHOTO_DATE_SQL;
    let Some((a, b)) = value.split_once("..") else {
        let (start, end) = date_bounds(value)?;
        return Some(Clause::new(
            &format!("({d} >= ? AND {d} < ?)"),
            vec![SqlValue::Text(start), SqlValue::Text(end)],
        ));
    };
    let from = if a.is_empty() {
        None
    } else {
        Some(date_bounds(a)?.0)
    };
    let to = if b.is_empty() {
        None
    } else {
        Some(date_bounds(b)?.1)
    };
    match (from, to) {
        (None, None) => None,
        (Some(a), None) => Some(Clause::new(&format!("{d} >= ?"), vec![SqlValue::Text(a)])),
        (None, Some(b)) => Some(Clause::new(&format!("{d} < ?"), vec![SqlValue::Text(b)])),
        (Some(a), Some(b)) => Some(Clause::new(
            &format!("({d} >= ? AND {d} < ?)"),
            vec![SqlValue::Text(a), SqlValue::Text(b)],
        )),
    }
}

fn rating_clause(value: &str) -> Option<Clause> {
    if let Some((a, b)) = value.split_once("..") {
        let lo = if a.is_empty() { 0 } else { parse_int(a)? };
        let hi = if b.is_empty() { 5 } else { parse_int(b)? };
        return Some(Clause::new(
            "p.rating BETWEEN ? AND ?",
            vec![SqlValue::Int(lo), SqlValue::Int(hi)],
        ));
    }
    let (op, rest) = comparison(value);
    let n = parse_int(rest)?;
    Some(Clause::new(
        &format!("p.rating {op} ?"),
        vec![SqlValue::Int(n)],
    ))
}

const HAS_FACES: &str = "EXISTS (SELECT 1 FROM faces fa WHERE fa.photo_id = p.id)";
const HAS_EDITS: &str = "EXISTS (SELECT 1 FROM edits e WHERE e.photo_id = p.id)";
const HAS_TAGS: &str = "EXISTS (SELECT 1 FROM photo_tags pt WHERE pt.photo_id = p.id)";

impl Builder {
    fn diag(&mut self, term: &Term, message: impl Into<String>) {
        self.diagnostics.push(Diag {
            span: term.span.clone(),
            message: message.into(),
        });
    }

    fn push(&mut self, term: &Term, clause: Clause) {
        let sql = if term.negated {
            format!("NOT ({})", clause.sql)
        } else {
            format!("({})", clause.sql)
        };
        self.clauses.push(sql);
        self.params.extend(clause.params);
    }

    fn term(&mut self, term: &Term) {
        if term.unterminated {
            self.diag(term, "missing closing quote");
        }
        let Some(key) = term.key.as_deref() else {
            self.word(term, &term.text, term.quoted);
            return;
        };
        if !KNOWN_KEYS.contains(&key) {
            self.diag(term, format!("unknown filter \"{key}\""));
            let whole = format!("{key} {}", term.text);
            self.word(term, &whole, true);
            return;
        }
        if term.text.is_empty() {
            self.diag(term, format!("\"{key}:\" needs a value"));
            return;
        }
        let value = term.text.as_str();
        let clause = match key {
            "tag" => Some(Clause::new(
                "EXISTS (SELECT 1 FROM photo_tags pt JOIN tags t ON t.id = pt.tag_id WHERE pt.photo_id = p.id AND t.name = ? COLLATE NOCASE)",
                vec![text(value)],
            )),
            "album" => Some(Clause::new(
                "EXISTS (SELECT 1 FROM album_items ai JOIN albums al ON al.id = ai.album_id WHERE ai.photo_id = p.id AND al.name = ? COLLATE NOCASE)",
                vec![text(value)],
            )),
            "folder" => Some(Clause::new(
                "f.path LIKE ? ESCAPE '\\'",
                vec![like_contains(value)],
            )),
            "camera" => Some(Clause::new(
                "(COALESCE(p.camera_make, '') || ' ' || COALESCE(p.camera_model, '')) LIKE ? ESCAPE '\\'",
                vec![like_contains(value)],
            )),
            "lens" => Some(Clause::new(
                "p.lens LIKE ? ESCAPE '\\'",
                vec![like_contains(value)],
            )),
            "person" => Some(Clause::new(
                "EXISTS (SELECT 1 FROM faces fa JOIN people pe ON pe.id = fa.person_id WHERE fa.photo_id = p.id AND pe.name LIKE ? ESCAPE '\\')",
                vec![like_contains(value)],
            )),
            "caption" => Some(Clause::new(
                "p.caption LIKE ? ESCAPE '\\'",
                vec![like_contains(value)],
            )),
            "name" => Some(Clause::new(
                "p.file_name LIKE ? ESCAPE '\\'",
                vec![like_contains(value)],
            )),
            "type" => match value.to_ascii_lowercase().as_str() {
                "photo" | "image" => Some(Clause::new("p.media_type = ?", vec![text("image")])),
                "video" => Some(Clause::new("p.media_type = ?", vec![text("video")])),
                _ => None,
            },
            "ext" => {
                let ext = value.trim_start_matches('.').to_ascii_lowercase();
                (!ext.is_empty()).then(|| {
                    Clause::new(
                        "lower(p.file_name) LIKE ? ESCAPE '\\'",
                        vec![SqlValue::Text(format!("%.{}", like_escape(&ext)))],
                    )
                })
            }
            "rating" => rating_clause(value),
            "date" => date_clause(value),
            "year" => {
                if value.len() == 4 && parse_int(value).is_some() {
                    date_clause(value)
                } else {
                    None
                }
            }
            "month" => month_number(value).map(|m| {
                Clause::new(
                    &format!("CAST(strftime('%m', {PHOTO_DATE_SQL}) AS INTEGER) = ?"),
                    vec![SqlValue::Int(m)],
                )
            }),
            "starred" => parse_bool(value)
                .map(|v| Clause::new("p.starred = ?", vec![SqlValue::Int(i64::from(v))])),
            "hidden" => parse_bool(value).map(|v| {
                if v && !term.negated {
                    self.include_hidden = true;
                }
                Clause::new("p.hidden = ?", vec![SqlValue::Int(i64::from(v))])
            }),
            "has" => match value.to_ascii_lowercase().as_str() {
                "gps" => Some(Clause::bare(
                    "p.gps_lat IS NOT NULL AND p.gps_lon IS NOT NULL",
                )),
                "caption" => Some(Clause::bare("COALESCE(p.caption, '') <> ''")),
                "faces" => Some(Clause::bare(HAS_FACES)),
                "edits" => Some(Clause::bare(HAS_EDITS)),
                "tags" => Some(Clause::bare(HAS_TAGS)),
                _ => None,
            },
            "is" => self.is_clause(term, &value.to_ascii_lowercase()),
            _ => None,
        };
        match clause {
            Some(c) => self.push(term, c),
            None => self.diag(term, format!("invalid value \"{value}\" for \"{key}\"")),
        }
    }

    fn is_clause(&mut self, term: &Term, value: &str) -> Option<Clause> {
        Some(match value {
            "starred" => Clause::bare("p.starred = 1"),
            "hidden" => {
                if !term.negated {
                    self.include_hidden = true;
                }
                Clause::bare("p.hidden = 1")
            }
            "edited" => Clause::bare(HAS_EDITS),
            "video" => Clause::new("p.media_type = ?", vec![text("video")]),
            "photo" => Clause::new("p.media_type = ?", vec![text("image")]),
            "missing" => {
                if !term.negated {
                    self.include_missing = true;
                }
                Clause::new("p.status = ?", vec![text("missing")])
            }
            "unreadable" => Clause::new("p.status = ?", vec![text("unreadable")]),
            _ => return None,
        })
    }

    fn word(&mut self, term: &Term, word: &str, exact: bool) {
        let prefix = !exact && word.chars().count() >= 2;
        let Some(expr) = fts_string(word, prefix) else {
            self.diag(term, "nothing to search for");
            return;
        };
        if term.negated {
            self.clauses
                .push("p.id NOT IN (SELECT rowid FROM photos_fts WHERE photos_fts MATCH ?)".into());
            self.params.push(SqlValue::Text(expr));
        } else {
            self.fts.push(expr);
        }
    }
}
