//! UI-agnostic service layer: commands, queries, events and view-models.
#![warn(missing_docs)]

mod albums;
mod atomic;
mod crash;
mod doctor;
mod edits;
mod error;
mod events;
mod export;
mod files;
mod history;
mod library;
mod lightbox;
mod paths;
mod photos;
mod query;
mod search;
mod system;
mod tags;
mod thumbs;
pub mod vm;

pub use albums::AlbumService;
pub use atomic::{TEMP_SUFFIX, write_atomic, write_bytes_atomic};
pub use crash::{dismiss_crash_logs, install_panic_hook, pending_crash_logs, write_crash_log};
pub use doctor::{DoctorReport, FACE_MODELS_ENV};
pub use edits::EditService;
pub use error::{AppError, Result};
pub use events::{AppEvent, COALESCE_WINDOW, EventBus};
pub use export::{
    CollisionPolicy, ExportFormat, ExportHandle, ExportOptions, ExportReport, ExportService,
    ExportSize, ExportedFile, MetadataPolicy,
};
pub use files::{DeleteReport, MoveReport};
pub use history::{HISTORY_LIMIT, HistoryAction, HistoryEntry, HistoryService};
pub use lb_jobs::JobQueue;
pub use lb_scan::ScanSummary;
pub use lb_thumbs::ThumbSize;
pub use library::LibraryService;
pub use lightbox::{Lightbox, LightboxOptions};
pub use paths::{AppPaths, LIGHTBOX_HOME_ENV};
pub use photos::PhotoService;
pub use photos::{MAX_CAPTION_CHARS, MAX_RATING};
pub use query::ViewSource;
pub use search::{FilterChips, Group, GroupKey, PhotoQueryResult, SearchService, ViewQuery};
pub use system::{SystemOpener, SystemTrash};
pub use tags::{AUTOCOMPLETE_LIMIT, TagService};
pub use thumbs::{ThumbService, ThumbTicket};
