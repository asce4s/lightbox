//! Faces and people (SPEC §6.15).

use crate::edit::NormRect;
use crate::error::ParseEnumError;
use crate::ids::{FaceId, PersonId, PhotoId};
use std::str::FromStr;

/// How a face entered the catalog.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum FaceSource {
    /// Found by the detector.
    Detected,
    /// Drawn by the user.
    Manual,
    /// Imported from a `.picasa.ini`.
    Picasa,
}

impl FaceSource {
    /// The value stored in the `source` column.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Detected => "detected",
            Self::Manual => "manual",
            Self::Picasa => "picasa",
        }
    }
}

impl FromStr for FaceSource {
    type Err = ParseEnumError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "detected" => Ok(Self::Detected),
            "manual" => Ok(Self::Manual),
            "picasa" => Ok(Self::Picasa),
            _ => Err(ParseEnumError::new("FaceSource", s)),
        }
    }
}

/// A face box in a photo.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Face {
    /// Row id.
    pub id: FaceId,
    /// The photo it is in.
    pub photo_id: PhotoId,
    /// Box normalized to the upright image.
    pub rect: NormRect,
    /// Detector confidence (1.0 for manual and imported faces).
    pub score: f32,
    /// Cluster of similar faces, if clustered.
    pub cluster_id: Option<i64>,
    /// Person the face belongs to, if named.
    pub person_id: Option<PersonId>,
    /// Where the face came from.
    pub source: FaceSource,
}

/// A named person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    /// Row id.
    pub id: PersonId,
    /// Display name.
    pub name: String,
    /// Hidden from the People list.
    pub hidden: bool,
}

/// Who a group of faces is: a named person or a still unnamed cluster.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum PeopleKey {
    /// A named person.
    Person(PersonId),
    /// An unnamed cluster.
    Cluster(i64),
}

/// One entry of the People list.
#[derive(Debug, Clone, PartialEq)]
pub struct PeopleEntry {
    /// Identity of the entry.
    pub key: PeopleKey,
    /// The person's name; `None` for unnamed clusters.
    pub name: Option<String>,
    /// Hidden person.
    pub hidden: bool,
    /// Number of faces.
    pub face_count: usize,
    /// Number of distinct photos.
    pub photo_count: usize,
    /// The best face to show as a thumbnail.
    pub cover: Face,
}
