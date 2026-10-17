//! One-call metadata extraction for still images.

use crate::{
    ExifData, Probe, Result,
    taken_at::{DateSources, TakenAt, mtime_to_naive_local, resolve_taken_at},
};
use std::path::Path;

/// Everything the scanner stores about a readable still image.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageMetadata {
    /// Format and oriented dimensions.
    pub probe: Probe,
    /// EXIF fields.
    pub exif: ExifData,
    /// Resolved capture time.
    pub taken_at: TakenAt,
}

/// Probes `path` and reads its EXIF and capture time. `file_name` feeds the file-name date
/// patterns; `mtime_unix` is the fallback capture time.
///
/// # Errors
/// Fails exactly when [`crate::probe`] does (empty, truncated or unrecognised files).
pub fn extract_image(path: &Path, file_name: &str, mtime_unix: i64) -> Result<ImageMetadata> {
    let probe = crate::probe(path)?;
    let exif = crate::read_exif(path);
    let taken_at = fallback_taken_at(
        &DateSources {
            exif_original: exif.date_time_original,
            exif_digitized: exif.date_time_digitized,
            video_creation: None,
        },
        file_name,
        mtime_unix,
    );
    Ok(ImageMetadata {
        probe,
        exif,
        taken_at,
    })
}

/// [`resolve_taken_at`] with the file mtime given as unix seconds. Also used for files that
/// could not be decoded, which still get a capture time.
pub fn fallback_taken_at(sources: &DateSources, file_name: &str, mtime_unix: i64) -> TakenAt {
    let mtime = mtime_to_naive_local(mtime_unix).unwrap_or_default();
    resolve_taken_at(sources, file_name, mtime)
}
