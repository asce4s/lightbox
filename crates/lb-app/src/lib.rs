//! UI-agnostic service layer: commands, queries, events and view-models.
#![warn(missing_docs)]

mod error;
mod events;
mod history;
mod library;
mod lightbox;
mod paths;
mod photos;

pub use error::{AppError, Result};
pub use events::{AppEvent, COALESCE_WINDOW, EventBus};
pub use history::{HISTORY_LIMIT, HistoryAction, HistoryEntry, HistoryService};
pub use library::LibraryService;
pub use lightbox::{Lightbox, LightboxOptions};
pub use paths::{AppPaths, LIGHTBOX_HOME_ENV};
pub use photos::PhotoService;
