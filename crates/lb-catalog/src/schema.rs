//! Append-only schema migrations (SPEC §5.2).

use crate::error::{CatalogError, Result};
use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

/// The schema version a freshly migrated database reports in `PRAGMA user_version`.
pub const LATEST_SCHEMA_VERSION: u32 = 2;

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![
        M::up(include_str!("../migrations/V1__init.sql")),
        M::up(include_str!("../migrations/V2__fts_contentless_delete.sql")),
    ])
}

/// Schema version below which the FTS index must be repopulated after migrating (V2 recreated it).
pub(crate) const FTS_REPOPULATE_BELOW: u32 = 2;

/// Brings `conn` to the latest schema, creating it on an empty database. Returns the schema
/// version the database had before migrating (0 for a new file).
pub(crate) fn migrate(conn: &mut Connection) -> Result<u32> {
    let before: u32 = conn.pragma_query_value(None, "user_version", |r| r.get(0))?;
    match migrations().to_latest(conn) {
        Ok(()) => Ok(before),
        Err(rusqlite_migration::Error::MigrationDefinition(
            rusqlite_migration::MigrationDefinitionError::DatabaseTooFarAhead,
        )) => Err(CatalogError::SchemaTooNew),
        Err(e) => Err(e.into()),
    }
}
