//! Repositories: typed access to every catalog table. All SQL in Lightbox lives here.

mod albums;
mod edits;
mod folders;
mod photos;
mod query;
mod roots;
mod scan;
mod settings;
mod tags;

#[cfg(test)]
mod test_util;
#[cfg(test)]
mod tests;

pub use edits::EditRecord;
pub use folders::NewFolder;
pub use photos::{PhotoHashes, PhotoUpsert, UpsertOutcome};
pub use query::{PhotoQuery, QueryRow, QuerySource, SqlFragment};
pub use scan::IndexedFile;

use crate::error::{CatalogError, Result};
use chrono::NaiveDateTime;
use rusqlite::types::Type;
use std::path::Path;
use std::str::FromStr;

const DB_DATETIME: &str = "%Y-%m-%dT%H:%M:%S";

/// Formats a timestamp as the ISO-8601 local string stored in the database (SPEC §5.2).
pub(crate) fn dt_to_db(dt: NaiveDateTime) -> String {
    dt.format(DB_DATETIME).to_string()
}

fn conversion_error(
    idx: usize,
    ty: Type,
    e: impl std::error::Error + Send + Sync + 'static,
) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(idx, ty, Box::new(e))
}

/// Parses a stored timestamp from column `idx`.
pub(crate) fn dt_from_db(idx: usize, text: &str) -> rusqlite::Result<NaiveDateTime> {
    NaiveDateTime::parse_from_str(text, DB_DATETIME)
        .map_err(|e| conversion_error(idx, Type::Text, e))
}

/// Parses an enum stored as text in column `idx`.
pub(crate) fn enum_from_db<T>(idx: usize, text: &str) -> rusqlite::Result<T>
where
    T: FromStr,
    T::Err: std::error::Error + Send + Sync + 'static,
{
    text.parse()
        .map_err(|e| conversion_error(idx, Type::Text, e))
}

/// Converts a stored integer into an unsigned/narrower type, failing the row on overflow.
pub(crate) fn int_from_db<T: TryFrom<i64>>(idx: usize, v: i64) -> rusqlite::Result<T>
where
    T::Error: std::error::Error + Send + Sync + 'static,
{
    T::try_from(v).map_err(|e| conversion_error(idx, Type::Integer, e))
}

pub(crate) fn path_str(path: &Path) -> Result<&str> {
    path.to_str()
        .ok_or_else(|| CatalogError::NonUtf8Path(path.to_path_buf()))
}

/// Builds `?,?,?` for `n` placeholders.
pub(crate) fn placeholders(n: usize) -> String {
    let mut s = String::with_capacity(n * 2);
    for i in 0..n {
        if i > 0 {
            s.push(',');
        }
        s.push('?');
    }
    s
}
