//! The photo record and its value types (SPEC §5.2, §5.3).

use crate::{FolderId, PhotoId, error::string_enum};
use chrono::NaiveDateTime;
use std::path::PathBuf;

string_enum!(
    /// Whether a file is a still image or a video.
    MediaType, "media type", {
        Image => "image",
        Video => "video",
    }
);

string_enum!(
    /// Container/codec family of a media file, as stored in `photos.format`.
    Format, "format", {
        Jpeg => "jpeg",
        Png => "png",
        Gif => "gif",
        Webp => "webp",
        Tiff => "tiff",
        Bmp => "bmp",
        Heif => "heif",
        Raw => "raw",
        Mp4 => "mp4",
        Mov => "mov",
        Avi => "avi",
        Mkv => "mkv",
        Webm => "webm",
    }
);

impl Format {
    /// Whether this format is a still image or a video.
    pub fn media_type(self) -> MediaType {
        match self {
            Self::Mp4 | Self::Mov | Self::Avi | Self::Mkv | Self::Webm => MediaType::Video,
            _ => MediaType::Image,
        }
    }
}

string_enum!(
    /// Indexing state of a photo row.
    PhotoStatus, "photo status", {
        Ok => "ok",
        Unreadable => "unreadable",
        Missing => "missing",
    }
);

/// A WGS-84 position in signed decimal degrees.
#[derive(Debug, Clone, Copy, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct GeoPoint {
    /// Latitude, -90..=90 (north positive).
    pub lat: f64,
    /// Longitude, -180..=180 (east positive).
    pub lon: f64,
}

/// One indexed photo or video (abridged view of the `photos` row).
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Photo {
    /// Row id.
    pub id: PhotoId,
    /// Containing folder.
    pub folder_id: FolderId,
    /// Absolute path of the file.
    pub path: PathBuf,
    /// Image or video.
    pub media_type: MediaType,
    /// File format.
    pub format: Format,
    /// File size in bytes.
    pub size: u64,
    /// Width in pixels after EXIF orientation.
    pub width: Option<u32>,
    /// Height in pixels after EXIF orientation.
    pub height: Option<u32>,
    /// Capture time as naive local time.
    pub taken_at: Option<NaiveDateTime>,
    /// `"<make> <model>"` when known.
    pub camera: Option<String>,
    /// GPS position.
    pub gps: Option<GeoPoint>,
    /// User caption.
    pub caption: Option<String>,
    /// Starred flag.
    pub starred: bool,
    /// Rating 0..=5.
    pub rating: u8,
    /// Hidden from the library by default.
    pub hidden: bool,
    /// Increments on every edit-stack change; keys thumbnail invalidation.
    pub edit_version: u32,
    /// Indexing state.
    pub status: PhotoStatus,
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::NaiveDate;

    #[test]
    fn schema_check_values_match_enum_strings() {
        let strings = |all: &[&str], got: Vec<&str>| assert_eq!(all, got.as_slice());
        strings(
            &["image", "video"],
            MediaType::ALL.iter().map(|m| m.as_str()).collect(),
        );
        strings(
            &["ok", "unreadable", "missing"],
            PhotoStatus::ALL.iter().map(|m| m.as_str()).collect(),
        );
        for &f in Format::ALL {
            assert_eq!(f.as_str().parse::<Format>().unwrap(), f);
        }
        assert_eq!(Format::Jpeg.media_type(), MediaType::Image);
        assert_eq!(Format::Raw.media_type(), MediaType::Image);
        assert_eq!(Format::Mov.media_type(), MediaType::Video);
        assert!(
            "jpg".parse::<Format>().is_err(),
            "only canonical names parse"
        );
    }

    #[test]
    fn photo_serde_roundtrip() {
        let photo = Photo {
            id: PhotoId::new(1),
            folder_id: FolderId::new(2),
            path: PathBuf::from("/photos/café ☕.jpg"),
            media_type: MediaType::Image,
            format: Format::Jpeg,
            size: 123_456,
            width: Some(640),
            height: Some(480),
            taken_at: NaiveDate::from_ymd_opt(2021, 7, 1).and_then(|d| d.and_hms_opt(12, 0, 0)),
            camera: Some("Canon EOS R6".into()),
            gps: Some(GeoPoint {
                lat: 38.7223,
                lon: -9.1393,
            }),
            caption: None,
            starred: true,
            rating: 4,
            hidden: false,
            edit_version: 3,
            status: PhotoStatus::Ok,
        };
        let json = serde_json::to_string(&photo).unwrap();
        assert!(
            json.contains("\"id\":1") && json.contains("\"taken_at\":\"2021-07-01T12:00:00\""),
            "{json}"
        );
        assert_eq!(serde_json::from_str::<Photo>(&json).unwrap(), photo);
    }
}
