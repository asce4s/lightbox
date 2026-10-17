//! UI-agnostic service layer: commands, queries, events and view-models.
#![warn(missing_docs)]

mod albums;
mod edits;
mod error;
mod events;
mod history;
mod library;
mod lightbox;
mod paths;
mod photos;
mod query;
mod tags;
mod thumbs;

pub use albums::AlbumService;
pub use edits::EditService;
pub use error::{AppError, Result};
pub use events::{AppEvent, COALESCE_WINDOW, EventBus};
pub use history::{HISTORY_LIMIT, HistoryAction, HistoryEntry, HistoryService};
pub use library::LibraryService;
pub use lightbox::{Lightbox, LightboxOptions};
pub use paths::{AppPaths, LIGHTBOX_HOME_ENV};
pub use photos::PhotoService;
pub use photos::{MAX_CAPTION_CHARS, MAX_RATING};
pub use query::ViewSource;
pub use tags::{AUTOCOMPLETE_LIMIT, TagService};
pub use thumbs::{ThumbService, ThumbTicket};
