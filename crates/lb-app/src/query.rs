//! What the grid shows: the source of photos and how they are listed (SPEC §7.2).

use lb_catalog::QuerySource;
use lb_core::{AlbumId, FolderId, PhotoId};

/// Where the photos of a view come from.
#[derive(Debug, Clone, PartialEq)]
pub enum ViewSource {
    /// The whole library.
    AllPhotos,
    /// One folder.
    Folder(FolderId),
    /// One album, in album order.
    Album(AlbumId),
    /// Starred photos.
    Starred,
    /// Edited photos, most recently edited first.
    RecentlyEdited,
    /// An explicit list, e.g. the photo tray or a duplicate set.
    Ids(Vec<PhotoId>),
}

impl ViewSource {
    pub(crate) fn to_catalog(&self) -> QuerySource {
        match self {
            Self::AllPhotos => QuerySource::All,
            Self::Folder(id) => QuerySource::Folder(*id),
            Self::Album(id) => QuerySource::Album(*id),
            Self::Starred => QuerySource::Starred,
            Self::RecentlyEdited => QuerySource::RecentlyEdited,
            Self::Ids(ids) => QuerySource::Ids(ids.clone()),
        }
    }
}
