//! Folder scanning, the Folder Manager, the file-system watcher and move detection
//! (SPEC §6.1–§6.3).
#![warn(missing_docs)]

mod error;
mod manager;
mod path;
mod rules;
mod scanner;
mod watcher;

pub use error::{Result, ScanError};
pub use manager::{FolderManager, RootChange, effective_root};
pub use path::{normalize_path, strip_verbatim_prefix};
pub use rules::{is_ignored_name, is_under_any};
pub use scanner::{ScanEvent, ScanOptions, ScanSummary, Scanner};
pub use watcher::{WatchConfig, WatchService};
