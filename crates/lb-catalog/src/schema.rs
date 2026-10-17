//! Append-only schema migrations (SPEC §5.2).

use crate::error::{CatalogError, Result};
use rusqlite::Connection;
use rusqlite_migration::{M, Migrations};

/// The schema version a freshly migrated database reports in `PRAGMA user_version`.
pub const LATEST_SCHEMA_VERSION: u32 = 1;

fn migrations() -> Migrations<'static> {
    Migrations::new(vec![M::up(include_str!("../migrations/V1__init.sql"))])
}

/// Brings `conn` to the latest schema, creating it on an empty database.
pub(crate) fn migrate(conn: &mut Connection) -> Result<()> {
    match migrations().to_latest(conn) {
        Ok(()) => Ok(()),
        Err(rusqlite_migration::Error::MigrationDefinition(
            rusqlite_migration::MigrationDefinitionError::DatabaseTooFarAhead,
        )) => Err(CatalogError::SchemaTooNew),
        Err(e) => Err(e.into()),
    }
}
