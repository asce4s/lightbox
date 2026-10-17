//! Path normalization for roots (SPEC §6.1.3).

use crate::{Result, ScanError};
use std::path::{Path, PathBuf};

/// Removes the Windows verbatim prefix that `canonicalize` adds: `\\?\C:\x` becomes `C:\x` and
/// `\\?\UNC\server\share` becomes `\\server\share`. Other strings pass through unchanged.
pub fn strip_verbatim_prefix(path: &str) -> String {
    if let Some(rest) = path.strip_prefix(r"\\?\UNC\") {
        return format!(r"\\{rest}");
    }
    if let Some(rest) = path.strip_prefix(r"\\?\") {
        let drive = rest.as_bytes();
        if drive.len() >= 2 && drive[0].is_ascii_alphabetic() && drive[1] == b':' {
            return rest.to_owned();
        }
    }
    path.to_owned()
}

/// Normalizes a user-supplied directory: absolute, `.`/`..` resolved, symlinks resolved, no
/// trailing separator and no `\\?\` prefix (SPEC §6.1.3). The path must exist.
///
/// Comparison of normalized paths is exact: `canonicalize` already returns the real spelling
/// on case-insensitive volumes.
pub fn normalize_path(path: &Path) -> Result<PathBuf> {
    let canonical = path.canonicalize().map_err(|source| ScanError::Io {
        path: path.to_owned(),
        source,
    })?;
    let text = canonical
        .to_str()
        .ok_or_else(|| ScanError::NonUtf8Path(canonical.clone()))?;
    Ok(PathBuf::from(strip_verbatim_prefix(text)))
}
