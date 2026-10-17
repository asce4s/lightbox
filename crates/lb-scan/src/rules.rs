//! Skip rules (SPEC §6.1: hidden and system directories).

use std::path::{Path, PathBuf};

const SYSTEM_NAMES: &[&str] = &["$recycle.bin", "system volume information", "@eadir"];

/// Whether a file or directory *name* is always ignored: dot-prefixed (hidden, which also
/// covers `.Trash*` and `.picasaoriginals`), `$RECYCLE.BIN`, `System Volume Information` and
/// `@eaDir`. Roots themselves are never checked against this rule.
pub fn is_ignored_name(name: &str) -> bool {
    name.starts_with('.') || SYSTEM_NAMES.iter().any(|s| name.eq_ignore_ascii_case(s))
}

/// Whether `path` is one of `dirs` or lies below one of them.
pub fn is_under_any(path: &Path, dirs: &[PathBuf]) -> bool {
    dirs.iter().any(|d| path.starts_with(d))
}

#[cfg(windows)]
pub(crate) fn has_hidden_attribute(meta: &std::fs::Metadata) -> bool {
    use std::os::windows::fs::MetadataExt;
    meta.file_attributes() & 0x2 != 0
}

#[cfg(not(windows))]
pub(crate) fn has_hidden_attribute(_meta: &std::fs::Metadata) -> bool {
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ignored_names_table() {
        for name in [
            ".hidden",
            ".DS_Store",
            ".Trash-1000",
            ".Trashes",
            ".picasaoriginals",
            "$RECYCLE.BIN",
            "$recycle.bin",
            "System Volume Information",
            "@eaDir",
            "@EADIR",
        ] {
            assert!(is_ignored_name(name), "{name}");
        }
        for name in [
            "photos",
            "2021",
            "café ☕",
            "eaDir",
            "my.hidden",
            "Trash",
            "IMG_0001.jpg",
        ] {
            assert!(!is_ignored_name(name), "{name}");
        }
    }

    #[test]
    fn under_any_is_component_wise() {
        let dirs = [PathBuf::from("/data/lightbox")];
        assert!(is_under_any(Path::new("/data/lightbox"), &dirs));
        assert!(is_under_any(Path::new("/data/lightbox/thumbs"), &dirs));
        assert!(!is_under_any(Path::new("/data/lightbox2"), &dirs));
        assert!(!is_under_any(Path::new("/data"), &dirs));
    }
}
