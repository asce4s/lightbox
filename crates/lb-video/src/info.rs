//! Parsing `ffprobe -print_format json` output (SPEC §6.16).

use crate::VideoError;
use chrono::{DateTime, NaiveDateTime};
use lb_core::GeoPoint;
use serde_json::Value;

/// What ffprobe reports about a video, before any rotation is applied to the size.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoInfo {
    /// Coded width.
    pub width: u32,
    /// Coded height.
    pub height: u32,
    /// Rotation in degrees clockwise (0, 90, 180 or 270) the player applies.
    pub rotation: u16,
    /// Duration in milliseconds.
    pub duration_ms: u64,
    /// Creation time in UTC as written in the container.
    pub creation_time_utc: Option<NaiveDateTime>,
    /// ISO 6709 location tag.
    pub location: Option<GeoPoint>,
}

impl VideoInfo {
    /// Width and height as displayed: swapped for quarter-turn rotations.
    pub fn display_size(&self) -> (u32, u32) {
        if self.rotation == 90 || self.rotation == 270 {
            (self.height, self.width)
        } else {
            (self.width, self.height)
        }
    }
}

fn seconds_to_ms(value: &Value) -> Option<u64> {
    let secs: f64 = match value {
        Value::String(s) => s.trim().parse().ok()?,
        Value::Number(n) => n.as_f64()?,
        _ => return None,
    };
    (secs.is_finite() && secs >= 0.0).then(|| (secs * 1000.0).round() as u64)
}

fn rotation_of(stream: &Value) -> u16 {
    let from_side_data = stream
        .get("side_data_list")
        .and_then(Value::as_array)
        .and_then(|list| list.iter().find_map(|d| d.get("rotation")))
        .and_then(Value::as_f64);
    let from_tag = || {
        stream
            .pointer("/tags/rotate")
            .and_then(Value::as_str)
            .and_then(|s| s.trim().parse::<f64>().ok())
    };
    match from_side_data.or_else(from_tag) {
        // ffprobe reports the display matrix counter-clockwise; players rotate clockwise.
        Some(deg) if from_side_data.is_some() => (-deg).rem_euclid(360.0).round() as u16 % 360,
        Some(deg) => deg.rem_euclid(360.0).round() as u16 % 360,
        None => 0,
    }
}

fn tag(value: &Value, pred: impl Fn(&str) -> bool) -> Option<&str> {
    value
        .get("tags")
        .and_then(Value::as_object)?
        .iter()
        .find(|(k, _)| pred(&k.to_ascii_lowercase()))
        .and_then(|(_, v)| v.as_str())
}

/// Parses ffprobe's JSON.
///
/// # Errors
/// Fails when the text is not JSON or has no video stream with a size.
pub fn parse_ffprobe(json: &str) -> Result<VideoInfo, VideoError> {
    let root: Value = serde_json::from_str(json).map_err(|e| VideoError::Probe(e.to_string()))?;
    let streams = root
        .get("streams")
        .and_then(Value::as_array)
        .map_or(&[][..], Vec::as_slice);
    let stream = streams
        .iter()
        .find(|s| {
            s.get("codec_type").and_then(Value::as_str) == Some("video")
                && s.pointer("/disposition/attached_pic")
                    .and_then(Value::as_i64)
                    != Some(1)
        })
        .ok_or_else(|| VideoError::Probe("no video stream".into()))?;
    let dim = |key: &str| {
        stream
            .get(key)
            .and_then(Value::as_u64)
            .and_then(|v| u32::try_from(v).ok())
            .filter(|v| *v > 0)
    };
    let (Some(width), Some(height)) = (dim("width"), dim("height")) else {
        return Err(VideoError::Probe("the video stream has no size".into()));
    };
    let format = root.get("format").unwrap_or(&Value::Null);
    let duration_ms = format
        .get("duration")
        .and_then(seconds_to_ms)
        .or_else(|| stream.get("duration").and_then(seconds_to_ms))
        .unwrap_or(0);
    let creation = tag(format, |k| k == "creation_time")
        .or_else(|| tag(stream, |k| k == "creation_time"))
        .and_then(parse_creation_time);
    let location = tag(format, is_location_key)
        .or_else(|| tag(stream, is_location_key))
        .and_then(parse_iso6709);
    Ok(VideoInfo {
        width,
        height,
        rotation: rotation_of(stream),
        duration_ms,
        creation_time_utc: creation,
        location,
    })
}

fn is_location_key(key: &str) -> bool {
    key.starts_with("location") || key.contains("iso6709")
}

fn parse_creation_time(text: &str) -> Option<NaiveDateTime> {
    let text = text.trim();
    DateTime::parse_from_rfc3339(text)
        .map(|d| d.naive_utc())
        .ok()
        .or_else(|| NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S").ok())
        .or_else(|| NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f").ok())
}

/// Splits `+37.77-122.41+012.0/` into its signed numbers.
fn signed_numbers(text: &str) -> Vec<&str> {
    let end = text
        .find(|c: char| c.is_ascii_alphabetic() || c == '/')
        .unwrap_or(text.len());
    let text = &text[..end];
    let mut starts: Vec<usize> = text
        .char_indices()
        .filter(|(_, c)| *c == '+' || *c == '-')
        .map(|(i, _)| i)
        .collect();
    starts.push(text.len());
    starts.windows(2).map(|w| &text[w[0]..w[1]]).collect()
}

/// One coordinate: decimal degrees (`+37.7749`), `DDMM.MMM` or `DDMMSS.SSS`; `deg_digits` is the
/// width of the degrees field (2 for latitude, 3 for longitude).
fn coordinate(text: &str, deg_digits: usize) -> Option<f64> {
    let sign = if text.starts_with('-') { -1.0 } else { 1.0 };
    let body = &text[1..];
    let int_len = body.find('.').unwrap_or(body.len());
    let value = match int_len.checked_sub(deg_digits)? {
        0 => body.parse::<f64>().ok()?,
        2 => {
            let (d, m) = body.split_at(deg_digits);
            d.parse::<f64>().ok()? + m.parse::<f64>().ok()? / 60.0
        }
        4 => {
            let (d, rest) = body.split_at(deg_digits);
            let (m, s) = rest.split_at(2);
            d.parse::<f64>().ok()? + m.parse::<f64>().ok()? / 60.0 + s.parse::<f64>().ok()? / 3600.0
        }
        _ => return None,
    };
    Some(sign * value)
}

/// Parses an ISO 6709 string as found in the `location` tag.
pub fn parse_iso6709(text: &str) -> Option<GeoPoint> {
    let parts = signed_numbers(text.trim());
    let (lat, lon) = (parts.first()?, parts.get(1)?);
    let (lat, lon) = (coordinate(lat, 2)?, coordinate(lon, 3)?);
    ((-90.0..=90.0).contains(&lat) && (-180.0..=180.0).contains(&lon))
        .then_some(GeoPoint { lat, lon })
}
