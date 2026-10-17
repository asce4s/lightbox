//! Thumbnail sizes and cache keys.

/// The three thumbnail sizes, named by long edge in pixels (SPEC §6.4).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum ThumbSize {
    /// 160 px.
    S,
    /// 320 px.
    M,
    /// 640 px.
    L,
}

impl ThumbSize {
    /// Every size, smallest first.
    pub const ALL: [ThumbSize; 3] = [ThumbSize::S, ThumbSize::M, ThumbSize::L];

    /// Long edge in pixels.
    pub fn px(self) -> u32 {
        match self {
            ThumbSize::S => 160,
            ThumbSize::M => 320,
            ThumbSize::L => 640,
        }
    }
}

/// `blake3(quick_key || edit_stack_hash || size)` as lowercase hex (SPEC §6.4). Unedited photos
/// use an empty `edit_hash`.
pub fn cache_key(quick_key: &str, edit_hash: &str, size: ThumbSize) -> String {
    let mut hasher = blake3::Hasher::new();
    for part in [quick_key.as_bytes(), edit_hash.as_bytes()] {
        hasher.update(&u64::try_from(part.len()).unwrap_or(u64::MAX).to_le_bytes());
        hasher.update(part);
    }
    hasher.update(&size.px().to_le_bytes());
    hasher.finalize().to_hex().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_differ_by_every_component() {
        let base = cache_key("q", "", ThumbSize::S);
        assert_eq!(base.len(), 64);
        assert_eq!(base, cache_key("q", "", ThumbSize::S));
        assert_ne!(base, cache_key("r", "", ThumbSize::S));
        assert_ne!(base, cache_key("q", "e", ThumbSize::S));
        assert_ne!(base, cache_key("q", "", ThumbSize::M));
        assert_ne!(
            cache_key("ab", "c", ThumbSize::S),
            cache_key("a", "bc", ThumbSize::S),
            "component boundaries are part of the hash"
        );
    }
}
