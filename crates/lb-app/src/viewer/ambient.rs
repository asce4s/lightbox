//! File cache for ambient backdrop textures (SPEC-VIEWER §V6.2).
//!
//! Files live at `{cache}/viewer/ambient/{blake3}.png`. Stage sizes are bucketed to multiples
//! of 32 px before rendering. At most 64 files are kept, evicted by an access tick.

use std::collections::HashMap;
use std::fs;
use std::io::Cursor;
use std::path::{Path, PathBuf};

use image::RgbaImage;
use lb_imaging::ambient::{AmbientParams, ambient_size, render_ambient};
use parking_lot::Mutex;

use crate::atomic::write_bytes_atomic;
use crate::error::Result;

const MAX_FILES: usize = 64;
const PARAMS_VERSION: u32 = 1;
const INDEX_FILE: &str = "access.idx";

/// On-disk ambient textures for one cache directory.
pub struct AmbientCache {
    dir: PathBuf,
    inner: Mutex<Index>,
}

struct Index {
    ticks: HashMap<String, u64>,
    next: u64,
    dirty: bool,
}

impl AmbientCache {
    /// Opens `{cache_root}/viewer/ambient`, creating nothing until the first render.
    #[must_use]
    pub fn open(cache_root: &Path) -> Self {
        let dir = cache_root.join("viewer").join("ambient");
        Self {
            inner: Mutex::new(Index::load(&dir)),
            dir,
        }
    }

    /// Returns the cached PNG for `source_key` at `stage`, rendering it when missing.
    ///
    /// `stage` is bucketed to multiples of 32 px. A second call with the same key and bucket
    /// reuses the file and does not render again.
    pub fn get_or_render(
        &self,
        source_key: &str,
        thumb: &RgbaImage,
        stage: (u32, u32),
    ) -> Result<PathBuf> {
        let params = AmbientParams::default();
        let stage = bucket_stage(stage);
        let (tw, th) = ambient_size(stage, params.out_scale);
        let key = cache_key(source_key, tw, th, &params);
        let path = self.dir.join(format!("{key}.png"));
        fs::create_dir_all(&self.dir)?;
        if path.is_file() {
            let mut index = self.inner.lock();
            index.touch(&key);
            index.flush(&self.dir)?;
            return Ok(path);
        }
        let image = render_ambient(thumb, stage, &params);
        let mut bytes = Vec::new();
        image::DynamicImage::ImageRgba8(image)
            .write_to(&mut Cursor::new(&mut bytes), image::ImageFormat::Png)
            .map_err(|e| crate::error::AppError::Invalid(format!("ambient encode failed: {e}")))?;
        write_bytes_atomic(&path, &bytes)?;
        let mut index = self.inner.lock();
        index.touch(&key);
        index.prune(&self.dir, MAX_FILES, &key)?;
        index.flush(&self.dir)?;
        Ok(path)
    }
}

/// Rounds each side to the nearest multiple of 32, and at least 32.
#[must_use]
pub fn bucket_stage(stage: (u32, u32)) -> (u32, u32) {
    (bucket_px(stage.0), bucket_px(stage.1))
}

fn bucket_px(v: u32) -> u32 {
    let rounded = (u64::from(v) + 16) / 32 * 32;
    u32::try_from(rounded).unwrap_or(u32::MAX).max(32)
}

fn cache_key(source_key: &str, w: u32, h: u32, p: &AmbientParams) -> String {
    let mut hasher = blake3::Hasher::new();
    hasher.update(source_key.as_bytes());
    hasher.update(&[0]);
    hasher.update(&w.to_le_bytes());
    hasher.update(&h.to_le_bytes());
    hasher.update(&PARAMS_VERSION.to_le_bytes());
    hasher.update(&p.tex_long_edge.to_le_bytes());
    hasher.update(&p.out_scale.to_le_bytes());
    hasher.update(&p.sigma.to_le_bytes());
    hasher.update(&p.saturation.to_le_bytes());
    hasher.update(&p.gain.to_le_bytes());
    hasher.update(&p.lift.to_le_bytes());
    hasher.update(&p.vignette.to_le_bytes());
    hasher.finalize().to_hex().to_string()
}

impl Index {
    fn load(dir: &Path) -> Self {
        let mut ticks = HashMap::new();
        let mut next = 1_u64;
        if let Ok(text) = fs::read_to_string(dir.join(INDEX_FILE)) {
            for line in text.lines() {
                if let Some((key, tick)) = line.split_once('\t')
                    && let Ok(tick) = tick.parse::<u64>()
                {
                    next = next.max(tick.saturating_add(1));
                    ticks.insert(key.to_owned(), tick);
                }
            }
        }
        Self {
            ticks,
            next,
            dirty: false,
        }
    }

    fn touch(&mut self, key: &str) {
        self.ticks.insert(key.to_owned(), self.next);
        self.next = self.next.saturating_add(1);
        self.dirty = true;
    }

    fn prune(&mut self, dir: &Path, limit: usize, keep: &str) -> std::io::Result<()> {
        let entries = fs::read_dir(dir)?;
        let mut files: Vec<(u64, String)> = entries
            .flatten()
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let key = name.strip_suffix(".png")?.to_owned();
                Some((self.ticks.get(&key).copied().unwrap_or(0), key))
            })
            .collect();
        if files.len() <= limit {
            return Ok(());
        }
        files.sort_by(|a, b| a.0.cmp(&b.0).then_with(|| a.1.cmp(&b.1)));
        let excess = files.len() - limit;
        let mut removed = 0;
        for (_, key) in files {
            if removed >= excess {
                break;
            }
            if key == keep {
                continue;
            }
            let _ = fs::remove_file(dir.join(format!("{key}.png")));
            self.ticks.remove(&key);
            self.dirty = true;
            removed += 1;
        }
        Ok(())
    }

    fn flush(&mut self, dir: &Path) -> std::io::Result<()> {
        if !self.dirty {
            return Ok(());
        }
        let mut entries: Vec<_> = self.ticks.iter().collect();
        entries.sort_by(|a, b| a.0.cmp(b.0));
        let mut text = String::new();
        for (key, tick) in entries {
            text.push_str(key);
            text.push('\t');
            text.push_str(&tick.to_string());
            text.push('\n');
        }
        write_bytes_atomic(&dir.join(INDEX_FILE), text.as_bytes())?;
        self.dirty = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Rgba;
    use std::sync::atomic::{AtomicU64, Ordering};

    static TEMP: AtomicU64 = AtomicU64::new(0);

    fn scratch() -> PathBuf {
        let n = TEMP.fetch_add(1, Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("lb-ambient-{}-{n}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        dir
    }

    fn thumb(c: u8) -> RgbaImage {
        RgbaImage::from_pixel(8, 8, Rgba([c, 40, 80, 255]))
    }

    #[test]
    fn ambient_cache_reuses_file_and_prunes_to_64() {
        let dir = scratch();
        let cache = AmbientCache::open(&dir);
        let first = cache
            .get_or_render("photo-a", &thumb(10), (1000, 700))
            .unwrap();
        assert!(first.starts_with(dir.join("viewer").join("ambient")));
        let png = fs::read(&first).unwrap();
        assert!(png.starts_with(b"\x89PNG"));
        fs::write(&first, b"sentinel").unwrap();
        let again = cache
            .get_or_render("photo-a", &thumb(10), (1000, 700))
            .unwrap();
        assert_eq!(again, first);
        assert_eq!(fs::read(&again).unwrap(), b"sentinel");

        for i in 0..70 {
            cache
                .get_or_render(&format!("photo-{i}"), &thumb(i as u8), (64, 64))
                .unwrap();
        }
        let pngs: Vec<_> = fs::read_dir(dir.join("viewer").join("ambient"))
            .unwrap()
            .flatten()
            .filter(|e| e.path().extension().is_some_and(|x| x == "png"))
            .collect();
        assert!(pngs.len() <= 64, "kept {}", pngs.len());
        let last = cache
            .get_or_render("photo-69", &thumb(69), (64, 64))
            .unwrap();
        assert!(last.is_file(), "the newest texture stays");
        let _ = fs::remove_dir_all(&dir);
    }
}
