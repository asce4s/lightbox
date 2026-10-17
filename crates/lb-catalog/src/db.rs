//! Connection management: one writer thread and a pool of read-only connections (SPEC §4.3).

use crate::error::{CatalogError, Result};
use crate::schema;
use crossbeam_channel::{Receiver, Sender};
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior};
use std::cell::Cell;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::thread::JoinHandle;
use std::time::Duration;

type WriteJob = Box<dyn FnOnce(&mut Connection) + Send>;

const BUSY_TIMEOUT: Duration = Duration::from_secs(5);

thread_local! {
    static ON_WRITER_THREAD: Cell<bool> = const { Cell::new(false) };
}

/// Handle to an open catalog database. Cheap to clone; all clones share one writer thread and
/// one reader pool.
///
/// * Writes go through [`Catalog::write`] / [`Catalog::write_tx`]: closures are queued to a single
///   thread that owns the only read-write connection, so writes are strictly serialized.
/// * Reads go through [`Catalog::read`] on a pool of read-only connections
///   (`min(4, cores)`), which run concurrently with writes thanks to WAL.
#[derive(Clone)]
pub struct Catalog {
    inner: Arc<Inner>,
}

impl std::fmt::Debug for Catalog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Catalog")
            .field("path", &self.inner.path)
            .finish_non_exhaustive()
    }
}

struct Inner {
    path: PathBuf,
    writer_tx: Option<Sender<WriteJob>>,
    writer_thread: Option<JoinHandle<()>>,
    pool_tx: Sender<Connection>,
    pool_rx: Receiver<Connection>,
    pool_size: usize,
}

impl Drop for Inner {
    fn drop(&mut self) {
        drop(self.writer_tx.take());
        if let Some(handle) = self.writer_thread.take()
            && handle.join().is_err()
        {
            tracing::error!("catalog writer thread panicked during shutdown");
        }
    }
}

struct PooledConnection<'a> {
    conn: Option<Connection>,
    pool: &'a Sender<Connection>,
}

impl Drop for PooledConnection<'_> {
    fn drop(&mut self) {
        if let Some(conn) = self.conn.take() {
            let _ = self.pool.send(conn);
        }
    }
}

fn configure(conn: &Connection) -> Result<()> {
    conn.busy_timeout(BUSY_TIMEOUT)?;
    conn.pragma_update(None, "foreign_keys", true)?;
    Ok(())
}

fn panic_message(payload: &(dyn std::any::Any + Send)) -> String {
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "non-string panic payload".to_owned())
}

impl Catalog {
    /// Opens (creating if needed) the catalog at `path`, applies pending migrations and starts
    /// the writer thread and reader pool. Parent directories are created.
    pub fn open(path: &Path) -> Result<Self> {
        if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
            std::fs::create_dir_all(parent)?;
        }
        let mut writer = Connection::open(path)?;
        configure(&writer)?;
        let mode: String =
            writer.pragma_update_and_check(None, "journal_mode", "WAL", |r| r.get(0))?;
        if !mode.eq_ignore_ascii_case("wal") {
            tracing::warn!(%mode, "catalog is not in WAL mode");
        }
        writer.pragma_update(None, "synchronous", "NORMAL")?;
        schema::migrate(&mut writer)?;

        let pool_size = std::thread::available_parallelism()
            .map_or(1, usize::from)
            .min(4);
        let (pool_tx, pool_rx) = crossbeam_channel::unbounded();
        for _ in 0..pool_size {
            let conn = Connection::open_with_flags(
                path,
                OpenFlags::SQLITE_OPEN_READ_ONLY | OpenFlags::SQLITE_OPEN_NO_MUTEX,
            )?;
            configure(&conn)?;
            conn.pragma_update(None, "query_only", true)?;
            let _ = pool_tx.send(conn);
        }

        let (writer_tx, writer_rx) = crossbeam_channel::unbounded::<WriteJob>();
        let writer_thread = std::thread::Builder::new()
            .name("lb-catalog-writer".into())
            .spawn(move || {
                ON_WRITER_THREAD.with(|f| f.set(true));
                let mut conn = writer;
                for job in writer_rx {
                    job(&mut conn);
                }
            })?;

        Ok(Self {
            inner: Arc::new(Inner {
                path: path.to_path_buf(),
                writer_tx: Some(writer_tx),
                writer_thread: Some(writer_thread),
                pool_tx,
                pool_rx,
                pool_size,
            }),
        })
    }

    /// Path of the database file.
    pub fn path(&self) -> &Path {
        &self.inner.path
    }

    /// Number of pooled read-only connections.
    pub fn reader_pool_size(&self) -> usize {
        self.inner.pool_size
    }

    /// Runs `f` on the writer thread with the read-write connection and returns its result.
    ///
    /// Calls are executed one at a time in submission order. A panic inside `f` is contained and
    /// reported as [`CatalogError::WriterPanicked`]. Calling `write` from inside `f` returns
    /// [`CatalogError::ReentrantWrite`] instead of deadlocking.
    pub fn write<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<T> + Send + 'static,
    {
        if ON_WRITER_THREAD.with(Cell::get) {
            return Err(CatalogError::ReentrantWrite);
        }
        let (reply_tx, reply_rx) = crossbeam_channel::bounded(1);
        let job: WriteJob = Box::new(move |conn| {
            let result = catch_unwind(AssertUnwindSafe(|| f(conn)))
                .unwrap_or_else(|p| Err(CatalogError::WriterPanicked(panic_message(&*p))));
            let _ = reply_tx.send(result);
        });
        let tx = self
            .inner
            .writer_tx
            .as_ref()
            .ok_or(CatalogError::WriterStopped)?;
        tx.send(job).map_err(|_| CatalogError::WriterStopped)?;
        reply_rx.recv().map_err(|_| CatalogError::WriterStopped)?
    }

    /// Like [`Catalog::write`] but wraps `f` in a transaction: committed on `Ok`, rolled back on
    /// `Err` or panic.
    pub fn write_tx<T, F>(&self, f: F) -> Result<T>
    where
        T: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> Result<T> + Send + 'static,
    {
        self.write(move |conn| {
            // IMMEDIATE takes the WAL write lock up front so the busy handler applies; a DEFERRED
            // read-then-write transaction fails instantly (~3% of runs) if a reader transiently
            // holds the lock while maintaining the wal-index.
            let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
            let value = f(&tx)?;
            tx.commit()?;
            Ok(value)
        })
    }

    /// Runs `f` on a pooled read-only connection, waiting if all are busy.
    ///
    /// Do not nest `read` calls deeper than [`Catalog::reader_pool_size`].
    pub fn read<T, F>(&self, f: F) -> Result<T>
    where
        F: FnOnce(&Connection) -> Result<T>,
    {
        let conn = self
            .inner
            .pool_rx
            .recv()
            .map_err(|_| CatalogError::WriterStopped)?;
        let guard = PooledConnection {
            conn: Some(conn),
            pool: &self.inner.pool_tx,
        };
        match guard.conn.as_ref() {
            Some(conn) => f(conn),
            None => Err(CatalogError::WriterStopped),
        }
    }
}

#[cfg(test)]
mod tests;
