//! The parsed form of a `.picasa.ini` file.

use crate::filters::map_filters;
use crate::ini::{Section, read_sections};
use crate::rect::decode_rect64;
use lb_core::{EditOp, NormRect};
use std::collections::BTreeMap;

/// File names Picasa used for its per-folder metadata, in order of preference.
pub const PICASA_INI_NAMES: [&str; 2] = [".picasa.ini", "Picasa.ini"];

/// Content of the `[Picasa]` section.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FolderMeta {
    /// Folder name.
    pub name: Option<String>,
    /// Folder caption.
    pub description: Option<String>,
}

/// An album defined by a `[.album:token]` section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumDef {
    /// Token photos refer to.
    pub token: String,
    /// Album name.
    pub name: String,
    /// Creation date text as written (ISO 8601 with offset), if any.
    pub date: Option<String>,
}

/// A face box on a photo.
#[derive(Debug, Clone, PartialEq)]
pub struct FaceRef {
    /// Box normalized to the image.
    pub rect: NormRect,
    /// Name from `[Contacts2]`; `None` when the face is unnamed or the contact is unknown.
    pub person: Option<String>,
}

/// Metadata of one photo.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FileEntry {
    /// File name as written in the section header.
    pub name: String,
    /// `star=yes`.
    pub star: bool,
    /// Unescaped caption.
    pub caption: Option<String>,
    /// Album tokens.
    pub albums: Vec<String>,
    /// Keywords (tags).
    pub keywords: Vec<String>,
    /// Edit operations: the rotation first, then the filters in order.
    pub ops: Vec<EditOp>,
    /// Filters that could not be mapped.
    pub skipped_filters: Vec<String>,
    /// Faces.
    pub faces: Vec<FaceRef>,
}

/// A whole parsed file.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PicasaIni {
    /// Folder properties.
    pub folder: FolderMeta,
    /// Albums in file order.
    pub albums: Vec<AlbumDef>,
    /// Contact id to name.
    pub contacts: BTreeMap<String, String>,
    /// Per-photo metadata in file order.
    pub files: Vec<FileEntry>,
}

/// Resolves the escapes Picasa writes in captions: `\n` is a line break and `\\` a backslash.
pub fn unescape_caption(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    let mut chars = raw.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('\\') => out.push('\\'),
            Some(other) => {
                out.push('\\');
                out.push(other);
            }
            None => out.push('\\'),
        }
    }
    out
}

fn split_list(value: &str) -> Vec<String> {
    value
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn non_empty(value: Option<&str>) -> Option<String> {
    value
        .map(str::trim)
        .filter(|v| !v.is_empty())
        .map(str::to_owned)
}

fn parse_faces(value: &str, contacts: &BTreeMap<String, String>) -> Vec<FaceRef> {
    value
        .split(';')
        .filter_map(|item| {
            let item = item.trim();
            let inner = item.strip_prefix("rect64(")?;
            let (hex, rest) = inner.split_once(')')?;
            let rect = decode_rect64(hex)?;
            let contact = rest.trim_start_matches(',').trim().to_lowercase();
            let person = contacts.get(&contact).cloned();
            Some(FaceRef { rect, person })
        })
        .collect()
}

fn parse_rotation(value: &str) -> Option<EditOp> {
    let n: i64 = value
        .trim()
        .strip_prefix("rotate(")?
        .strip_suffix(')')?
        .trim()
        .parse()
        .ok()?;
    match n.rem_euclid(4) {
        1 => Some(EditOp::RotateQuarter { turns: 1 }),
        2 => Some(EditOp::RotateQuarter { turns: 2 }),
        3 => Some(EditOp::RotateQuarter { turns: -1 }),
        _ => None,
    }
}

fn file_entry(section: &Section, contacts: &BTreeMap<String, String>) -> FileEntry {
    let mut entry = FileEntry {
        name: section.name.clone(),
        star: section
            .get("star")
            .is_some_and(|v| v.eq_ignore_ascii_case("yes")),
        caption: non_empty(section.get("caption")).map(|c| unescape_caption(&c)),
        albums: section.get("albums").map(split_list).unwrap_or_default(),
        keywords: section.get("keywords").map(split_list).unwrap_or_default(),
        ..FileEntry::default()
    };
    if let Some(rotate) = section.get("rotate").and_then(parse_rotation) {
        entry.ops.push(rotate);
    }
    if let Some(filters) = section.get("filters") {
        let mapped = map_filters(filters);
        entry.ops.extend(mapped.ops);
        entry.skipped_filters = mapped.skipped;
    }
    if let Some(faces) = section.get("faces") {
        entry.faces = parse_faces(faces, contacts);
    }
    entry
}

/// Parses the text of a `.picasa.ini` (or `Picasa.ini`) file. Unknown sections and keys are
/// ignored; malformed values are skipped rather than failing the whole file.
pub fn parse_picasa_ini(text: &str) -> PicasaIni {
    let sections = read_sections(text);
    let mut ini = PicasaIni::default();
    for section in &sections {
        let lower = section.name.to_ascii_lowercase();
        if lower == "contacts2" || lower == "contacts" {
            for (id, value) in &section.entries {
                let name = value.split(';').next().unwrap_or("").trim();
                if !name.is_empty() {
                    ini.contacts.insert(id.to_lowercase(), name.to_owned());
                }
            }
        }
    }
    for section in &sections {
        let lower = section.name.to_ascii_lowercase();
        if lower == "picasa" {
            ini.folder = FolderMeta {
                name: non_empty(section.get("name")),
                description: non_empty(section.get("description")).map(|d| unescape_caption(&d)),
            };
        } else if lower == "contacts2" || lower == "contacts" {
        } else if lower.starts_with(".album:") {
            let token = non_empty(section.get("token"))
                .unwrap_or_else(|| section.name[".album:".len()..].to_owned());
            if let Some(name) = non_empty(section.get("name")) {
                ini.albums.push(AlbumDef {
                    token,
                    name,
                    date: non_empty(section.get("date")),
                });
            }
        } else if !section.name.starts_with('.') {
            ini.files.push(file_entry(section, &ini.contacts));
        }
    }
    ini
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn caption_escapes() {
        assert_eq!(unescape_caption(r"a\nb\\c\x"), "a\nb\\c\\x");
        assert_eq!(unescape_caption("plain"), "plain");
        assert_eq!(unescape_caption("end\\"), "end\\");
    }

    #[test]
    fn rotation_maps_to_quarter_turns() {
        assert_eq!(
            parse_rotation("rotate(1)"),
            Some(EditOp::RotateQuarter { turns: 1 })
        );
        assert_eq!(
            parse_rotation("rotate(3)"),
            Some(EditOp::RotateQuarter { turns: -1 })
        );
        assert_eq!(
            parse_rotation("rotate(2)"),
            Some(EditOp::RotateQuarter { turns: 2 })
        );
        assert_eq!(parse_rotation("rotate(0)"), None);
        assert_eq!(parse_rotation("spin(1)"), None);
    }

    #[test]
    fn faces_resolve_contacts_and_unnamed_markers() {
        let ini = parse_picasa_ini(
            "[Contacts2]\n4F3E2D1C=Ana;;\n[a.jpg]\nfaces=rect64(3f845bcb59418507),4f3e2d1c;rect64(9eb15e89b6b584c1),ffffffffffffffff;rect64(0),x\n",
        );
        let faces = &ini.files[0].faces;
        assert_eq!(faces.len(), 2, "the empty rectangle is dropped");
        assert_eq!(faces[0].person.as_deref(), Some("Ana"));
        assert_eq!(faces[1].person, None);
    }

    #[test]
    fn legacy_and_odd_files_do_not_fail() {
        let ini = parse_picasa_ini("");
        assert_eq!(ini, PicasaIni::default());
        let ini = parse_picasa_ini(
            "[picasa]\nname=X\n[.album:t]\ntoken=t\n[.something]\nx=1\n[b.jpg]\nstar=YES\n",
        );
        assert_eq!(ini.folder.name.as_deref(), Some("X"));
        assert!(ini.albums.is_empty(), "albums without a name are dropped");
        assert_eq!(ini.files.len(), 1);
        assert!(ini.files[0].star);
    }
}
