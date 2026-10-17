//! Content fingerprints (SPEC §5.2 `quick_key`, `content_hash`).

use crate::Result;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

/// Bytes sampled from the head and from the tail of a file for the quick key.
pub const QUICK_KEY_SAMPLE: u64 = 64 * 1024;

/// `blake3(size_le || first 64 KiB || last 64 KiB)` as hex, computed from the file.
///
/// For files of at most 128 KiB the head and tail overlap and the whole file is hashed once
/// (as head followed by the non-overlapping remainder), so equal files always get equal keys.
pub fn quick_key(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let size = file.metadata()?.len();
    let mut hasher = blake3::Hasher::new();
    hasher.update(&size.to_le_bytes());
    let head_len = size.min(QUICK_KEY_SAMPLE);
    let mut buf = vec![0_u8; usize::try_from(head_len).unwrap_or(0)];
    file.read_exact(&mut buf)?;
    hasher.update(&buf);
    let tail_len = (size - head_len).min(QUICK_KEY_SAMPLE);
    if tail_len > 0 {
        file.seek(SeekFrom::Start(size - tail_len))?;
        buf.resize(usize::try_from(tail_len).unwrap_or(0), 0);
        file.read_exact(&mut buf)?;
        hasher.update(&buf);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

/// The quick key of an in-memory file image; identical to [`quick_key`] on the same bytes.
pub fn quick_key_of_bytes(bytes: &[u8]) -> String {
    let size = bytes.len() as u64;
    let mut hasher = blake3::Hasher::new();
    hasher.update(&size.to_le_bytes());
    let sample = usize::try_from(QUICK_KEY_SAMPLE).unwrap_or(usize::MAX);
    let head = bytes.len().min(sample);
    hasher.update(&bytes[..head]);
    let tail = (bytes.len() - head).min(sample);
    hasher.update(&bytes[bytes.len() - tail..]);
    hasher.finalize().to_hex().to_string()
}

/// Streaming full-file blake3 hash as hex.
pub fn content_hash(path: &Path) -> Result<String> {
    let mut file = File::open(path)?;
    let mut hasher = blake3::Hasher::new();
    let mut buf = vec![0_u8; 1 << 20];
    loop {
        let n = file.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    Ok(hasher.finalize().to_hex().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn file_with(bytes: &[u8]) -> (tempfile::TempDir, std::path::PathBuf) {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("f.bin");
        File::create(&path).unwrap().write_all(bytes).unwrap();
        (dir, path)
    }

    fn pseudo(len: usize) -> Vec<u8> {
        (0..len).map(|i| (i.wrapping_mul(31) % 251) as u8).collect()
    }

    #[test]
    fn quick_key_stable() {
        for len in [
            0_usize, 1, 100, 65_535, 65_536, 65_537, 131_072, 131_073, 400_000,
        ] {
            let bytes = pseudo(len);
            let (_d, path) = file_with(&bytes);
            let a = quick_key(&path).unwrap();
            assert_eq!(a, quick_key(&path).unwrap(), "len {len}");
            assert_eq!(a, quick_key_of_bytes(&bytes), "len {len}");
            assert_eq!(a.len(), 64);
        }
        let (_d1, p1) = file_with(&pseudo(300_000));
        let mut changed = pseudo(300_000);
        changed[150_000] ^= 0xFF;
        let (_d2, p2) = file_with(&changed);
        assert_eq!(
            quick_key(&p1).unwrap(),
            quick_key(&p2).unwrap(),
            "bytes outside head and tail are not sampled"
        );
        let mut tail = pseudo(300_000);
        tail[299_999] ^= 1;
        let (_d3, p3) = file_with(&tail);
        assert_ne!(quick_key(&p1).unwrap(), quick_key(&p3).unwrap());
        let (_d4, p4) = file_with(&pseudo(300_001));
        assert_ne!(
            quick_key(&p1).unwrap(),
            quick_key(&p4).unwrap(),
            "size is part of the key"
        );
    }

    #[test]
    fn content_hash_matches_blake3_cli_vector() {
        let (_d, empty) = file_with(b"");
        assert_eq!(
            content_hash(&empty).unwrap(),
            "af1349b9f5f9a1a6a0404dea36dcc9499bcb25c9adc112b7cc9a93cae41f3262"
        );
        let (_d, abc) = file_with(b"abc");
        assert_eq!(
            content_hash(&abc).unwrap(),
            "6437b3ac38465133ffb63b75273a8db548c558465d79db03fd359c6cd5bd9d85"
        );
        let big = pseudo(3_000_000);
        let (_d, path) = file_with(&big);
        assert_eq!(
            content_hash(&path).unwrap(),
            blake3::hash(&big).to_hex().to_string(),
            "streaming equals one-shot across buffer boundaries"
        );
    }
}
