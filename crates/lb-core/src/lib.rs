//! Domain types, ids, errors and the settings model. Performs no I/O.
#![warn(missing_docs)]

mod error;
mod ids;
mod photo;
mod query;

pub use error::ParseEnumError;
pub use ids::{AlbumId, FaceId, FolderId, JobId, PersonId, PhotoId, TagId};
pub use photo::{Format, GeoPoint, MediaType, Photo, PhotoStatus};
pub use query::{GroupBy, Selection, SortOrder};
