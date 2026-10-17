//! Folder scanning, the Folder Manager, the file-system watcher and move detection
//! (SPEC §6.1–§6.3).
#![warn(missing_docs)]

mod error;
mod rules;
mod scanner;

pub use error::{Result, ScanError};
pub use rules::{is_ignored_name, is_under_any};
pub use scanner::{ScanEvent, ScanOptions, ScanSummary, Scanner};
