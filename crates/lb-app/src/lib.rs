//! UI-agnostic service layer: commands, queries, events and view-models.
#![warn(missing_docs)]

mod albums;
mod atomic;
mod collage;
mod crash;
mod doctor;
mod dupes;
mod edits;
mod error;
mod events;
mod export;
#[cfg(feature = "faces")]
mod faces;
mod files;
mod history;
mod import;
mod library;
mod lightbox;
#[cfg(feature = "map")]
mod map;
#[cfg(feature = "video")]
mod movie;
mod paths;
mod photos;
mod picasa;
mod query;
mod rename;
mod search;
mod settings;
mod system;
mod tags;
mod thumbs;
#[cfg(feature = "video")]
mod video;
pub mod vm;

pub use albums::AlbumService;
pub use atomic::{TEMP_SUFFIX, write_atomic, write_bytes_atomic};
pub use collage::{
    CollageLayout, CollageOptions, CollageService, CollageSize, CreatedCollage, PREVIEW_EDGE,
};
pub use crash::{dismiss_crash_logs, install_panic_hook, pending_crash_logs, write_crash_log};
pub use crossbeam_channel::Receiver as EventReceiver;
pub use doctor::{DoctorReport, FACE_MODELS_ENV};
pub use dupes::{DEFAULT_SIMILARITY, DuplicateGroup, DuplicatesService};
pub use edits::{EditService, PreviewFile};
pub use error::{AppError, Result};
pub use events::{AppEvent, COALESCE_WINDOW, EventBus};
pub use export::{
    CollisionPolicy, ExportFormat, ExportHandle, ExportOptions, ExportReport, ExportService,
    ExportSize, ExportedFile, MetadataPolicy,
};
#[cfg(feature = "faces")]
pub use faces::{FaceScanReport, FaceService, MAX_RECLUSTER_FACES};
pub use files::{DeleteReport, MoveReport};
pub use history::{HISTORY_LIMIT, HistoryAction, HistoryEntry, HistoryService};
pub use import::{
    FileCopier, FolderNaming, IMPORT_TEMP_SUFFIX, ImportCandidate, ImportHandle, ImportOptions,
    ImportReport, ImportService, ImportedFile, StdCopier,
};
pub use lb_jobs::JobQueue;
pub use lb_scan::ScanSummary;
pub use lb_thumbs::ThumbSize;
pub use library::LibraryService;
pub use lightbox::{Lightbox, LightboxOptions};
#[cfg(feature = "map")]
pub use map::{ATTRIBUTION, CLUSTER_CELL_PX, MAX_ZOOM, MIN_ZOOM, MapService};
#[cfg(feature = "video")]
pub use movie::{MovieReport, MovieService, MovieSettings};
pub use paths::{AppPaths, LIGHTBOX_HOME_ENV};
pub use photos::PhotoService;
pub use photos::{MAX_CAPTION_CHARS, MAX_RATING};
pub use picasa::{PicasaImportService, PicasaOptions, PicasaReport};
pub use query::ViewSource;
pub use rename::{RenamePreview, RenameReport, expand_rename_pattern};
pub use search::{
    FilterChips, Group, GroupKey, MonthBucket, PhotoQueryResult, SearchService, ViewQuery,
};
pub use settings::{
    ExportSettings, FacesSettings, GridSettings, LibrarySettings, MapSettings, SETTINGS_KEYS,
    Settings, SettingsService, SlideshowSettings, ThemeChoice, ThumbChoice, ThumbSettings,
    Transition, VideoSettings,
};
pub use system::{SystemOpener, SystemTrash};
pub use tags::{AUTOCOMPLETE_LIMIT, TagService};
pub use thumbs::{ThumbService, ThumbTicket};
