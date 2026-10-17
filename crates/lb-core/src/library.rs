//! Library structure types: roots, folders, albums, tags (SPEC §5.2, §6.1, §6.7).

use crate::{AlbumId, FolderId, PhotoId, RootId, TagId, error::string_enum};
use chrono::NaiveDateTime;
use std::path::PathBuf;

string_enum!(
    /// How the Folder Manager treats a root (SPEC §6.1).
    RootMode, "root mode", {
        Watch => "watch",
        Once => "once",
        Exclude => "exclude",
    }
);

/// A folder configured in the Folder Manager.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Root {
    /// Row id.
    pub id: RootId,
    /// Absolute, normalized path (SPEC §6.1.3).
    pub path: PathBuf,
    /// Scan/watch/exclude behaviour.
    pub mode: RootMode,
    /// When the root was added.
    pub added_at: NaiveDateTime,
}

/// An indexed directory under a root.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Folder {
    /// Row id.
    pub id: FolderId,
    /// Owning root.
    pub root_id: RootId,
    /// Absolute, normalized path.
    pub path: PathBuf,
    /// Parent folder; `None` for a root directory.
    pub parent_id: Option<FolderId>,
    /// Last path component.
    pub name: String,
    /// Folder description.
    pub caption: Option<String>,
    /// Earliest `taken_at` among the folder's photos.
    pub sort_date: Option<NaiveDateTime>,
}

/// A virtual, ordered collection of photos.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Album {
    /// Row id.
    pub id: AlbumId,
    /// Display name.
    pub name: String,
    /// Optional description.
    pub description: Option<String>,
    /// Cover photo, if chosen.
    pub cover_photo_id: Option<PhotoId>,
    /// Creation time.
    pub created_at: NaiveDateTime,
    /// Manual ordering among albums.
    pub sort_order: i64,
}

/// A free-form label attached to photos.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Tag {
    /// Row id.
    pub id: TagId,
    /// Normalized display name.
    pub name: String,
}

/// A tag with the number of photos carrying it.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct TagUsage {
    /// The tag.
    pub tag: Tag,
    /// Number of photos tagged.
    pub count: u32,
}

/// Maximum length of a tag name in characters (SPEC §6.7.2).
pub const MAX_TAG_NAME_CHARS: usize = 64;

/// Why a tag name was rejected.
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum InvalidTagName {
    /// Empty after trimming.
    #[error("tag name is empty")]
    Empty,
    /// Longer than [`MAX_TAG_NAME_CHARS`] characters.
    #[error("tag name is longer than {MAX_TAG_NAME_CHARS} characters")]
    TooLong,
    /// Contains a comma.
    #[error("tag name must not contain commas")]
    Comma,
}

/// Normalizes a user-entered tag: trims, collapses internal whitespace to single spaces,
/// rejects empty names, names over 64 characters and names containing commas (SPEC §6.7.2).
/// Case is preserved; uniqueness is case-insensitive and enforced by the catalog.
pub fn normalize_tag_name(raw: &str) -> Result<String, InvalidTagName> {
    if raw.contains(',') {
        return Err(InvalidTagName::Comma);
    }
    let name = raw.split_whitespace().collect::<Vec<_>>().join(" ");
    if name.is_empty() {
        return Err(InvalidTagName::Empty);
    }
    if name.chars().count() > MAX_TAG_NAME_CHARS {
        return Err(InvalidTagName::TooLong);
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_name_normalization_table() {
        let ok = [
            ("beach", "beach"),
            ("  Beach  ", "Beach"),
            ("Beach \t\n Trip", "Beach Trip"),
            ("café ☕", "café ☕"),
            ("\u{3000}写真\u{3000}", "写真"),
        ];
        for (input, expected) in ok {
            assert_eq!(
                normalize_tag_name(input).as_deref(),
                Ok(expected),
                "{input:?}"
            );
        }
        assert_eq!(normalize_tag_name(""), Err(InvalidTagName::Empty));
        assert_eq!(normalize_tag_name(" \t "), Err(InvalidTagName::Empty));
        assert_eq!(normalize_tag_name("a,b"), Err(InvalidTagName::Comma));
        assert_eq!(normalize_tag_name(","), Err(InvalidTagName::Comma));
        assert!(normalize_tag_name(&"x".repeat(64)).is_ok());
        assert_eq!(
            normalize_tag_name(&"x".repeat(65)),
            Err(InvalidTagName::TooLong)
        );
        assert!(
            normalize_tag_name(&"é".repeat(64)).is_ok(),
            "limit counts characters, not bytes"
        );
    }

    #[test]
    fn root_mode_strings_match_schema_check() {
        let all: Vec<_> = RootMode::ALL.iter().map(|m| m.as_str()).collect();
        assert_eq!(all, ["watch", "once", "exclude"]);
        assert!("always".parse::<RootMode>().is_err());
    }
}
