//! Key/value settings stored as JSON text (SPEC §5.2 `settings`).

use crate::{Catalog, error::Result};
use rusqlite::OptionalExtension;

impl Catalog {
    /// Reads the raw JSON stored under `key`.
    pub fn get_setting(&self, key: &str) -> Result<Option<String>> {
        self.read(|c| {
            Ok(c.query_row(
                "SELECT value_json FROM settings WHERE key = ?1",
                [key],
                |r| r.get(0),
            )
            .optional()?)
        })
    }

    /// Stores raw JSON under `key`, replacing any previous value.
    pub fn set_setting(&self, key: &str, value_json: &str) -> Result<()> {
        let (key, value) = (key.to_owned(), value_json.to_owned());
        self.write_tx(move |tx| {
            tx.execute(
                "INSERT INTO settings (key, value_json) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value_json = excluded.value_json",
                (key, value),
            )?;
            Ok(())
        })
    }

    /// Removes a setting; returns whether it existed.
    pub fn delete_setting(&self, key: &str) -> Result<bool> {
        let key = key.to_owned();
        self.write_tx(move |tx| Ok(tx.execute("DELETE FROM settings WHERE key = ?1", [key])? > 0))
    }

    /// All settings as `(key, json)` pairs ordered by key.
    pub fn list_settings(&self) -> Result<Vec<(String, String)>> {
        self.read(|c| {
            let mut stmt = c.prepare("SELECT key, value_json FROM settings ORDER BY key")?;
            Ok(stmt
                .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
                .collect::<rusqlite::Result<_>>()?)
        })
    }
}
