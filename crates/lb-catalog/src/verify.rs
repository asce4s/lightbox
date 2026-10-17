//! Whole-database consistency checks (SPEC §12.6), used by the test invariants guard.

use crate::fts::{INSERT_COLUMNS, ROW_SELECT};
use crate::{Catalog, error::Result};
use rusqlite::Connection;
use std::collections::BTreeSet;

/// Most search words compared between the live index and a freshly built one.
const SAMPLE_WORDS: usize = 40;

fn pragma_rows(conn: &Connection, pragma: &str) -> Result<Vec<Vec<String>>> {
    let mut stmt = conn.prepare(pragma)?;
    let columns = stmt.column_count();
    let rows = stmt
        .query_map([], |r| {
            (0..columns)
                .map(|i| {
                    r.get::<_, rusqlite::types::Value>(i)
                        .map(|v| format!("{v:?}"))
                })
                .collect()
        })?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

fn words(conn: &Connection) -> Result<Vec<String>> {
    let mut found = BTreeSet::new();
    let mut stmt = conn.prepare(
        "SELECT file_name || ' ' || COALESCE(caption, '') FROM photos
         UNION ALL SELECT name FROM tags UNION ALL SELECT name FROM albums
         UNION ALL SELECT name FROM people",
    )?;
    let texts = stmt.query_map([], |r| r.get::<_, String>(0))?;
    for text in texts {
        for word in text?.split(|c: char| !c.is_alphanumeric()) {
            if word.chars().count() >= 2 {
                found.insert(word.to_lowercase());
            }
        }
    }
    let all: Vec<String> = found.into_iter().collect();
    let stride = all.len().div_ceil(SAMPLE_WORDS).max(1);
    Ok(all.into_iter().step_by(stride).collect())
}

fn matches(conn: &Connection, table: &str, word: &str) -> Result<Vec<i64>> {
    let name = table.rsplit('.').next().unwrap_or(table);
    let mut stmt = conn.prepare(&format!(
        "SELECT rowid FROM {table} WHERE {name} MATCH ?1 ORDER BY rowid"
    ))?;
    let expr = format!("\"{}\"*", word.replace('"', "\"\""));
    let rows = stmt
        .query_map([expr], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(rows)
}

fn fts_problems(conn: &Connection) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    conn.execute_batch(
        "DROP TABLE IF EXISTS temp.photos_fts_check;
         CREATE VIRTUAL TABLE temp.photos_fts_check USING fts5(
           file_name, caption, tags, folder_path, camera, people, album_names,
           content='', tokenize='unicode61 remove_diacritics 2');",
    )?;
    let check = (|| -> Result<()> {
        conn.execute(
            &format!(
                "{} {ROW_SELECT}",
                INSERT_COLUMNS.replace("INTO photos_fts", "INTO temp.photos_fts_check")
            ),
            [],
        )?;
        let live = pragma_rows(
            conn,
            "SELECT id, sz FROM main.photos_fts_docsize ORDER BY id",
        )?;
        let fresh = pragma_rows(
            conn,
            "SELECT id, sz FROM temp.photos_fts_check_docsize ORDER BY id",
        )?;
        if live != fresh {
            problems.push(format!(
                "full-text index differs from a rebuild ({} live rows, {} expected)",
                live.len(),
                fresh.len()
            ));
        }
        for word in words(conn)? {
            let a = matches(conn, "main.photos_fts", &word)?;
            let b = matches(conn, "temp.photos_fts_check", &word)?;
            if a != b {
                problems.push(format!(
                    "full-text search for `{word}` finds {a:?} but a rebuild finds {b:?}"
                ));
            }
        }
        Ok(())
    })();
    conn.execute_batch("DROP TABLE IF EXISTS temp.photos_fts_check;")?;
    check?;
    Ok(problems)
}

impl Catalog {
    /// Runs `PRAGMA integrity_check`, `PRAGMA foreign_key_check` and compares the incremental
    /// full-text index with one rebuilt from the tables for a sample of search words. Returns
    /// one line per problem; empty means the catalog is consistent. Runs on the writer thread
    /// because the comparison index is a temporary table.
    pub fn verify(&self) -> Result<Vec<String>> {
        self.write(|conn| {
            let mut problems = Vec::new();
            let integrity = pragma_rows(conn, "PRAGMA integrity_check")?;
            if integrity.len() != 1 || integrity[0] != ["Text(\"ok\")"] {
                problems.extend(integrity.into_iter().map(|r| format!("integrity: {r:?}")));
            }
            problems.extend(
                pragma_rows(conn, "PRAGMA foreign_key_check")?
                    .into_iter()
                    .map(|r| format!("foreign key: {r:?}")),
            );
            problems.extend(fts_problems(conn)?);
            Ok(problems)
        })
    }
}
