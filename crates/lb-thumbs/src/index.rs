//! The access-time sidecar index used by cache GC (SPEC §6.4). It stores a logical tick per
//! key instead of wall-clock time so eviction order is deterministic.

use crate::Result;
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};

pub(crate) const INDEX_FILE: &str = "access.idx";

#[derive(Debug)]
pub(crate) struct AccessIndex {
    path: PathBuf,
    ticks: HashMap<String, u64>,
    next: u64,
    dirty: bool,
}

impl AccessIndex {
    pub(crate) fn load(thumbs_dir: &Path) -> Self {
        let path = thumbs_dir.join(INDEX_FILE);
        let mut ticks = HashMap::new();
        let mut next = 1;
        if let Ok(text) = fs::read_to_string(&path) {
            for line in text.lines() {
                if let Some((key, tick)) = line.split_once('\t')
                    && let Ok(tick) = tick.parse::<u64>()
                {
                    next = next.max(tick + 1);
                    ticks.insert(key.to_owned(), tick);
                }
            }
        }
        Self {
            path,
            ticks,
            next,
            dirty: false,
        }
    }

    pub(crate) fn touch(&mut self, key: &str) {
        self.ticks.insert(key.to_owned(), self.next);
        self.next += 1;
        self.dirty = true;
    }

    pub(crate) fn tick(&self, key: &str) -> u64 {
        self.ticks.get(key).copied().unwrap_or(0)
    }

    pub(crate) fn forget(&mut self, key: &str) {
        if self.ticks.remove(key).is_some() {
            self.dirty = true;
        }
    }

    pub(crate) fn retain(&mut self, mut keep: impl FnMut(&str) -> bool) {
        let before = self.ticks.len();
        self.ticks.retain(|k, _| keep(k));
        self.dirty |= self.ticks.len() != before;
    }

    pub(crate) fn flush(&mut self) -> Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let mut entries: Vec<_> = self.ticks.iter().collect();
        entries.sort();
        let mut text = String::new();
        for (key, tick) in entries {
            text.push_str(&format!("{key}\t{tick}\n"));
        }
        let tmp = self.path.with_extension("idx.part");
        let mut file = fs::File::create(&tmp)?;
        file.write_all(text.as_bytes())?;
        file.sync_all()?;
        fs::rename(&tmp, &self.path)?;
        self.dirty = false;
        Ok(())
    }
}
