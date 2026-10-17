//! SQLite schema, migrations, repositories and full-text search.
#![warn(missing_docs)]

mod backup;
mod db;
mod error;
mod fts;
mod repo;
mod schema;

pub use backup::{
    BACKUPS_KEPT, BackupInfo, backup_file_name, check_file, list_backups, restore_from_backup,
};
pub use db::Catalog;
pub use error::{CatalogError, Result};
pub use repo::{
    EditRecord, IndexedFile, NewFolder, PhotoHashes, PhotoQuery, PhotoUpsert, QueryRow,
    QuerySource, SqlFragment, UpsertOutcome,
};
pub use schema::LATEST_SCHEMA_VERSION;
