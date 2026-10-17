//! Integrity checks, daily backups with rotation, and restore (SPEC §10).

use crate::{Catalog, CatalogError, LATEST_SCHEMA_VERSION, Result};
use chrono::NaiveDate;
use rusqlite::{Connection, OpenFlags, backup::Backup};
use std::ffi::OsString;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// Number of daily backups kept by [`Catalog::daily_backup`] (SPEC §10).
pub const BACKUPS_KEPT: usize = 7;

const PREFIX: &str = "catalog-";
const SUFFIX: &str = ".sqlite3";
const TMP_SUFFIX: &str = ".tmp";
const PAGES_PER_STEP: i32 = 512;

/// A backup file found in the backups directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BackupInfo {
    /// Day the backup was taken (from the file name).
    pub date: NaiveDate,
    /// Full path of the backup file.
    pub path: PathBuf,
}

/// File name of the backup for `date`: `catalog-YYYY-MM-DD.sqlite3`.
pub fn backup_file_name(date: NaiveDate) -> String {
    format!("{PREFIX}{}{SUFFIX}", date.format("%Y-%m-%d"))
}

fn parse_backup_name(name: &str) -> Option<NaiveDate> {
    let stem = name.strip_prefix(PREFIX)?.strip_suffix(SUFFIX)?;
    let date = NaiveDate::parse_from_str(stem, "%Y-%m-%d").ok()?;
    (backup_file_name(date) == name).then_some(date)
}

/// Lists the backups in `dir`, newest first. Files that do not match the backup naming scheme
/// are ignored. A missing directory yields an empty list.
pub fn list_backups(dir: &Path) -> Result<Vec<BackupInfo>> {
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) => return Err(e.into()),
    };
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry?;
        let path = entry.path();
        let date = path
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(parse_backup_name);
        if let (Some(date), true) = (date, entry.file_type()?.is_file()) {
            found.push(BackupInfo { date, path });
        }
    }
    found.sort_by_key(|b| std::cmp::Reverse(b.date));
    Ok(found)
}

fn check_conn(conn: &Connection, pragma: &str) -> Result<Vec<String>> {
    let mut stmt = conn.prepare(pragma)?;
    let rows: Vec<String> = stmt
        .query_map([], |r| r.get(0))?
        .collect::<rusqlite::Result<_>>()?;
    Ok(if rows.len() == 1 && rows[0] == "ok" {
        Vec::new()
    } else {
        rows
    })
}

/// Runs `PRAGMA quick_check` on a database file without opening it as a catalog. Returns the
/// reported problems (empty when healthy). Fails if the file is not a readable SQLite database.
pub fn check_file(path: &Path) -> Result<Vec<String>> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    check_conn(&conn, "PRAGMA quick_check")
}

fn user_version(conn: &Connection) -> Result<u32> {
    Ok(conn.pragma_query_value(None, "user_version", |r| r.get(0))?)
}

fn append_suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name: OsString = path.as_os_str().to_owned();
    name.push(suffix);
    PathBuf::from(name)
}

fn sync_file(path: &Path) -> Result<()> {
    fs::File::open(path)?.sync_all()?;
    Ok(())
}

/// Checks that `path` is a healthy catalog database no newer than this build understands.
fn validate_backup(path: &Path) -> Result<()> {
    let conn = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)?;
    if let Some(problem) = check_conn(&conn, "PRAGMA quick_check")?.into_iter().next() {
        return Err(CatalogError::BackupInvalid(problem));
    }
    if user_version(&conn)? > LATEST_SCHEMA_VERSION {
        return Err(CatalogError::SchemaTooNew);
    }
    Ok(())
}

impl Catalog {
    /// Runs `PRAGMA quick_check` and returns the reported problems; empty means healthy.
    pub fn quick_check(&self) -> Result<Vec<String>> {
        self.read(|c| check_conn(c, "PRAGMA quick_check"))
    }

    /// Runs the full `PRAGMA integrity_check` and returns the reported problems; empty means
    /// healthy. Slower than [`Catalog::quick_check`] on large catalogs.
    pub fn integrity_check(&self) -> Result<Vec<String>> {
        self.read(|c| check_conn(c, "PRAGMA integrity_check"))
    }

    /// Writes a consistent snapshot of the catalog to `dest` using SQLite's online backup API.
    ///
    /// The snapshot is taken on the writer thread, so writes queue behind it. The file is built
    /// next to `dest` as `*.tmp`, verified with `quick_check`, synced and atomically renamed;
    /// it is a single file (rollback journal mode, no `-wal`/`-shm`).
    pub fn backup_to(&self, dest: &Path) -> Result<()> {
        let tmp = append_suffix(dest, TMP_SUFFIX);
        let dest = dest.to_path_buf();
        self.write(move |src| {
            let _ = fs::remove_file(&tmp);
            let outcome = (|| -> Result<()> {
                {
                    let mut dst = Connection::open(&tmp)?;
                    Backup::new(src, &mut dst)?.run_to_completion(
                        PAGES_PER_STEP,
                        Duration::ZERO,
                        None,
                    )?;
                    let _mode: String =
                        dst.pragma_update_and_check(None, "journal_mode", "DELETE", |r| r.get(0))?;
                    if let Some(problem) =
                        check_conn(&dst, "PRAGMA quick_check")?.into_iter().next()
                    {
                        return Err(CatalogError::BackupInvalid(problem));
                    }
                    dst.close().map_err(|(_, e)| e)?;
                }
                sync_file(&tmp)?;
                fs::rename(&tmp, &dest)?;
                Ok(())
            })();
            if outcome.is_err() {
                let _ = fs::remove_file(&tmp);
            }
            outcome
        })
    }

    /// Takes today's backup into `dir` unless one for `today` already exists, then deletes all but
    /// the newest `keep` backups. Returns the new backup's path, or `None` when today's backup
    /// was already there. Only files named `catalog-YYYY-MM-DD.sqlite3` are ever deleted.
    pub fn daily_backup(
        &self,
        dir: &Path,
        today: NaiveDate,
        keep: usize,
    ) -> Result<Option<PathBuf>> {
        fs::create_dir_all(dir)?;
        let target = dir.join(backup_file_name(today));
        let created = if target.exists() {
            None
        } else {
            self.backup_to(&target)?;
            Some(target)
        };
        for stale in list_backups(dir)?.into_iter().skip(keep.max(1)) {
            if let Err(e) = fs::remove_file(&stale.path) {
                tracing::warn!(path = %stale.path.display(), error = %e, "could not delete old catalog backup");
            }
        }
        remove_stray_temp_files(dir);
        Ok(created)
    }
}

fn remove_stray_temp_files(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let is_ours = name
            .to_str()
            .and_then(|n| n.strip_suffix(TMP_SUFFIX))
            .is_some_and(|stem| parse_backup_name(stem).is_some());
        if is_ours {
            let _ = fs::remove_file(entry.path());
        }
    }
}

/// Replaces the catalog at `catalog_path` with `backup`. The catalog must not be open.
///
/// The backup is validated first (`quick_check`, schema version). The current database and its
/// `-wal`/`-shm` files are moved aside to `<catalog>.corrupt`, `.corrupt-wal`, `.corrupt-shm`
/// (replacing older ones) so stale WAL frames can never be applied to the restored file, then the
/// backup is copied into place atomically.
pub fn restore_from_backup(catalog_path: &Path, backup: &Path) -> Result<()> {
    validate_backup(backup)?;
    let staging = append_suffix(catalog_path, ".restoring");
    let _ = fs::remove_file(&staging);
    let copied = fs::copy(backup, &staging)
        .map_err(CatalogError::from)
        .and_then(|_| {
            sync_file(&staging)?;
            validate_backup(&staging)
        });
    if let Err(e) = copied {
        let _ = fs::remove_file(&staging);
        return Err(e);
    }

    for suffix in ["", "-wal", "-shm"] {
        let current = append_suffix(catalog_path, suffix);
        if current.exists() {
            fs::rename(
                &current,
                append_suffix(catalog_path, &format!(".corrupt{suffix}")),
            )?;
        }
    }
    fs::rename(&staging, catalog_path)?;
    Ok(())
}

impl CatalogError {
    /// Whether this error means the database file is damaged or not a database, i.e. the user
    /// should be offered a restore from backup.
    pub fn is_corruption(&self) -> bool {
        matches!(
            self,
            CatalogError::BackupInvalid(_)
                | CatalogError::Sqlite(rusqlite::Error::SqliteFailure(
                    rusqlite::ffi::Error {
                        code: rusqlite::ErrorCode::DatabaseCorrupt
                            | rusqlite::ErrorCode::NotADatabase,
                        ..
                    },
                    _,
                ))
        )
    }
}
