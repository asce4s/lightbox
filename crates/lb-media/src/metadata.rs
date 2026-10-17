//! EXIF access.

use std::fs::File;
use std::io::BufReader;
use std::path::Path;

fn read(path: &Path) -> Option<exif::Exif> {
    let file = File::open(path).ok()?;
    exif::Reader::new()
        .read_from_container(&mut BufReader::new(file))
        .ok()
}

/// EXIF orientation of the file (1..=8); 1 when absent or unreadable.
pub(crate) fn read_orientation(path: &Path) -> u8 {
    read(path)
        .and_then(|e| {
            e.get_field(exif::Tag::Orientation, exif::In::PRIMARY)
                .and_then(|f| f.value.get_uint(0))
        })
        .and_then(|v| u8::try_from(v).ok())
        .filter(|v| (1..=8).contains(v))
        .unwrap_or(1)
}
