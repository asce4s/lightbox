//! The on-disk thumbnail cache: lookup, atomic writes and LRU garbage collection.

use crate::generate::{encode_jpeg, render};
use crate::index::{AccessIndex, INDEX_FILE};
use crate::{EditFn, Result, ThumbSize, cache_key};
use lb_media::{Decoder, DefaultDecoder};
use parking_lot::Mutex;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

const PART_SUFFIX: &str = ".part";

/// What to render. `edit_hash` and `edits` travel together: the hash identifies the edit
/// stack that `edits` applies.
pub struct ThumbRequest<'a> {
    /// The original image file.
    pub path: &'a Path,
    /// The photo's quick key (SPEC §6.2).
    pub quick_key: &'a str,
    /// Requested size.
    pub size: ThumbSize,
    /// Canonical hash of the edit stack; empty when unedited.
    pub edit_hash: &'a str,
    /// Applies the edit stack to the oriented proxy image.
    pub edits: Option<&'a EditFn>,
}

impl<'a> ThumbRequest<'a> {
    /// A request for the unedited image.
    pub fn new(path: &'a Path, quick_key: &'a str, size: ThumbSize) -> Self {
        Self {
            path,
            quick_key,
            size,
            edit_hash: "",
            edits: None,
        }
    }

    /// Renders with `edits` applied; `edit_hash` becomes part of the cache key.
    #[must_use]
    pub fn with_edits(mut self, edit_hash: &'a str, edits: &'a EditFn) -> Self {
        self.edit_hash = edit_hash;
        self.edits = Some(edits);
        self
    }

    /// The cache key this request resolves to.
    pub fn key(&self) -> String {
        cache_key(self.quick_key, self.edit_hash, self.size)
    }
}

/// A cached thumbnail file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Thumb {
    /// Absolute path of the JPEG.
    pub path: PathBuf,
    /// The cache key.
    pub key: String,
    /// True when the file already existed and nothing was decoded.
    pub cache_hit: bool,
}

/// Outcome of [`ThumbCache::gc`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct GcReport {
    /// Thumbnails deleted.
    pub evicted: usize,
    /// Bytes freed by evicting thumbnails.
    pub freed_bytes: u64,
    /// Bytes still cached.
    pub remaining_bytes: u64,
    /// Leftover partial temp files removed.
    pub stale_temp_files: usize,
}

/// Content-addressed thumbnail cache rooted at `{cache}/thumbs`.
pub struct ThumbCache {
    root: PathBuf,
    decoder: Arc<dyn Decoder>,
    index: Mutex<AccessIndex>,
    temp_counter: AtomicU64,
}

impl ThumbCache {
    /// Opens (creating if needed) the cache under `cache_dir` using the built-in decoder.
    pub fn open(cache_dir: &Path) -> Result<Self> {
        Self::with_decoder(cache_dir, Arc::new(DefaultDecoder))
    }

    /// Like [`ThumbCache::open`] with an injected decoder (tests count decodes through it).
    pub fn with_decoder(cache_dir: &Path, decoder: Arc<dyn Decoder>) -> Result<Self> {
        let root = cache_dir.join("thumbs");
        fs::create_dir_all(&root)?;
        let index = AccessIndex::load(&root);
        Ok(Self {
            root,
            decoder,
            index: Mutex::new(index),
            temp_counter: AtomicU64::new(0),
        })
    }

    /// Where the thumbnail for `key` lives (it may not exist).
    pub fn path_for(&self, key: &str, size: ThumbSize) -> PathBuf {
        let shard = key.get(..2).unwrap_or("00");
        self.root
            .join(size.px().to_string())
            .join(shard)
            .join(format!("{key}.jpg"))
    }

    /// The cached thumbnail for `req`, if present. Marks it as recently used.
    pub fn lookup(&self, req: &ThumbRequest<'_>) -> Option<Thumb> {
        let key = req.key();
        let path = self.path_for(&key, req.size);
        if path.is_file() {
            self.index.lock().touch(&key);
            Some(Thumb {
                path,
                key,
                cache_hit: true,
            })
        } else {
            None
        }
    }

    /// Returns the thumbnail for `req`, rendering and caching it on a miss. Unreadable sources
    /// return an error and leave nothing in the cache.
    pub fn get_or_create(&self, req: &ThumbRequest<'_>) -> Result<Thumb> {
        if let Some(hit) = self.lookup(req) {
            return Ok(hit);
        }
        let key = req.key();
        let img = render(self.decoder.as_ref(), req.path, req.size, req.edits)?;
        let bytes = encode_jpeg(&img)?;
        let path = self.path_for(&key, req.size);
        self.write_atomic(&path, &bytes)?;
        self.index.lock().touch(&key);
        Ok(Thumb {
            path,
            key,
            cache_hit: false,
        })
    }

    fn write_atomic(&self, dest: &Path, bytes: &[u8]) -> Result<()> {
        let dir = dest.parent().unwrap_or(&self.root);
        fs::create_dir_all(dir)?;
        let n = self.temp_counter.fetch_add(1, Ordering::Relaxed);
        let tmp = dir.join(format!(".tmp-{}-{n}{PART_SUFFIX}", std::process::id()));
        let written = (|| -> std::io::Result<()> {
            let mut file = fs::File::create(&tmp)?;
            file.write_all(bytes)?;
            file.sync_all()?;
            fs::rename(&tmp, dest)
        })();
        if written.is_err() {
            let _ = fs::remove_file(&tmp);
        }
        Ok(written?)
    }

    /// Removes the cached thumbnails for `key` at every size.
    pub fn remove(&self, key: &str) {
        for size in ThumbSize::ALL {
            let _ = fs::remove_file(self.path_for(key, size));
        }
        self.index.lock().forget(key);
    }

    /// Total bytes of cached thumbnails.
    pub fn total_bytes(&self) -> u64 {
        scan(&self.root).files.iter().map(|f| f.bytes).sum()
    }

    /// Deletes least-recently-accessed thumbnails until the cache holds at most `limit_bytes`,
    /// and removes partial temp files left by an interrupted write. Run on startup.
    pub fn gc(&self, limit_bytes: u64) -> Result<GcReport> {
        let found = scan(&self.root);
        let mut report = GcReport::default();
        for stale in &found.stale {
            if fs::remove_file(stale).is_ok() {
                report.stale_temp_files += 1;
            }
        }
        let mut index = self.index.lock();
        let mut files = found.files;
        let mut total: u64 = files.iter().map(|f| f.bytes).sum();
        files.sort_by(|a, b| {
            index
                .tick(&a.key)
                .cmp(&index.tick(&b.key))
                .then_with(|| a.key.cmp(&b.key))
        });
        let mut keep: std::collections::HashSet<String> =
            files.iter().map(|f| f.key.clone()).collect();
        for file in &files {
            if total <= limit_bytes {
                break;
            }
            if fs::remove_file(&file.path).is_ok() {
                total -= file.bytes;
                report.evicted += 1;
                report.freed_bytes += file.bytes;
                keep.remove(&file.key);
            }
        }
        report.remaining_bytes = total;
        index.retain(|k| keep.contains(k));
        index.flush()?;
        Ok(report)
    }

    /// Persists the access index. Called on shutdown and after GC; also attempted on drop.
    pub fn flush(&self) -> Result<()> {
        self.index.lock().flush()
    }
}

impl Drop for ThumbCache {
    fn drop(&mut self) {
        if let Err(e) = self.index.lock().flush() {
            tracing::warn!(error = %e, "could not persist thumbnail access index");
        }
    }
}

struct CachedFile {
    key: String,
    path: PathBuf,
    bytes: u64,
}

#[derive(Default)]
struct Scan {
    files: Vec<CachedFile>,
    stale: Vec<PathBuf>,
}

fn scan(root: &Path) -> Scan {
    let mut out = Scan::default();
    let mut stack = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(meta) = entry.metadata() else { continue };
            let name = entry.file_name().to_string_lossy().into_owned();
            if meta.is_dir() {
                stack.push(path);
            } else if name.ends_with(PART_SUFFIX) {
                out.stale.push(path);
            } else if let Some(key) = name.strip_suffix(".jpg") {
                out.files.push(CachedFile {
                    key: key.to_owned(),
                    path,
                    bytes: meta.len(),
                });
            } else if name != INDEX_FILE {
                tracing::debug!(%name, "ignoring unknown file in thumbnail cache");
            }
        }
    }
    out
}
