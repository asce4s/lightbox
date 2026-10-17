//! Domain types, ids, errors and the settings model. Performs no I/O.
#![warn(missing_docs)]

mod clock;
mod edit;
mod error;
mod ids;
mod library;
mod photo;
mod ports;
mod query;

pub use clock::{Clock, SystemClock};
#[cfg(feature = "test-strategies")]
pub use edit::strategies;
pub use edit::{EditError, EditOp, EditStack, NormRect, RetouchSpot};
pub use error::ParseEnumError;
pub use ids::{AlbumId, FaceId, FolderId, JobId, PersonId, PhotoId, RootId, TagId};
pub use library::{
    Album, Folder, InvalidTagName, MAX_TAG_NAME_CHARS, Root, RootMode, Tag, TagUsage,
    normalize_tag_name,
};
pub use photo::{Format, GeoPoint, MediaType, Photo, PhotoStatus, TakenAtSource};
pub use ports::{Opener, TrashBackend};
pub use query::{GroupBy, Selection, SortOrder, SqlValue};
