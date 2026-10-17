//! SQLite schema, migrations, repositories and full-text search.
#![warn(missing_docs)]

mod db;
mod error;
mod schema;

pub use db::Catalog;
pub use error::{CatalogError, Result};
pub use schema::LATEST_SCHEMA_VERSION;
