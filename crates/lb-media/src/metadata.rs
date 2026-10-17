//! EXIF extraction (SPEC §6.2.2).

use chrono::{NaiveDate, NaiveDateTime};
use exif::{Exif, In, Rational, Tag, Value};
use lb_core::GeoPoint;
use std::fs::File;
use std::io::BufReader;
use std::path::Path;

/// The EXIF fields Lightbox indexes. Every field is optional; a file without EXIF yields the
/// default value.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct ExifData {
    /// `Orientation` 1..=8 (1 when absent or invalid).
    pub orientation: u8,
    /// `DateTimeOriginal` as naive local time.
    pub date_time_original: Option<NaiveDateTime>,
    /// `DateTimeDigitized` as naive local time.
    pub date_time_digitized: Option<NaiveDateTime>,
    /// `OffsetTimeOriginal`, e.g. `+01:00`.
    pub offset: Option<String>,
    /// `Make`.
    pub make: Option<String>,
    /// `Model`.
    pub model: Option<String>,
    /// `LensModel`.
    pub lens: Option<String>,
    /// ISO speed.
    pub iso: Option<u32>,
    /// Aperture f-number.
    pub f_number: Option<f64>,
    /// Exposure time in seconds.
    pub exposure_s: Option<f64>,
    /// Focal length in millimetres.
    pub focal_mm: Option<f64>,
    /// GPS position in signed decimal degrees.
    pub gps: Option<GeoPoint>,
    /// GPS altitude in metres, negative below sea level.
    pub gps_alt: Option<f64>,
    /// `ImageDescription` or `UserComment`.
    pub description: Option<String>,
}

fn ascii(exif: &Exif, tag: Tag) -> Option<String> {
    match &exif.get_field(tag, In::PRIMARY)?.value {
        Value::Ascii(parts) => {
            let text = parts
                .iter()
                .map(|p| {
                    String::from_utf8_lossy(p)
                        .trim_matches(['\0', ' '])
                        .to_owned()
                })
                .collect::<Vec<_>>()
                .join(" ");
            let text = text.trim().to_owned();
            (!text.is_empty()).then_some(text)
        }
        _ => None,
    }
}

fn rational(exif: &Exif, tag: Tag) -> Option<f64> {
    match &exif.get_field(tag, In::PRIMARY)?.value {
        Value::Rational(v) => v.first().and_then(ratio),
        _ => None,
    }
}

fn ratio(r: &Rational) -> Option<f64> {
    (r.denom != 0).then(|| f64::from(r.num) / f64::from(r.denom))
}

fn uint(exif: &Exif, tag: Tag) -> Option<u32> {
    exif.get_field(tag, In::PRIMARY)?.value.get_uint(0)
}

/// Parses an EXIF `YYYY:MM:DD HH:MM:SS` timestamp; zero dates and garbage give `None`.
fn exif_datetime(text: &str) -> Option<NaiveDateTime> {
    let t = text.trim();
    let (date, time) = t.split_once(' ')?;
    let mut d = date.split(':').map(|p| p.parse::<u32>().ok());
    let mut tm = time
        .split(':')
        .map(|p| p.get(..2).and_then(|p| p.parse::<u32>().ok()));
    let (y, m, day) = (d.next()??, d.next()??, d.next()??);
    let (hh, mm, ss) = (tm.next()??, tm.next()??, tm.next().flatten().unwrap_or(0));
    NaiveDate::from_ymd_opt(i32::try_from(y).ok()?, m, day)?.and_hms_opt(hh, mm, ss)
}

fn datetime(exif: &Exif, tag: Tag) -> Option<NaiveDateTime> {
    ascii(exif, tag).and_then(|s| exif_datetime(&s))
}

fn dms(exif: &Exif, tag: Tag, ref_tag: Tag, negative: char) -> Option<f64> {
    let Value::Rational(parts) = &exif.get_field(tag, In::PRIMARY)?.value else {
        return None;
    };
    let [d, m, s] = parts.as_slice() else {
        return None;
    };
    let value = ratio(d)? + ratio(m)? / 60.0 + ratio(s)? / 3600.0;
    let sign = match ascii(exif, ref_tag)?.chars().next()?.to_ascii_uppercase() {
        c if c == negative => -1.0,
        _ => 1.0,
    };
    Some(sign * value)
}

/// Validates a GPS fix: in range and not exactly (0, 0) (SPEC §6.2.2).
pub(crate) fn valid_gps(lat: f64, lon: f64) -> Option<GeoPoint> {
    let ok = lat.is_finite()
        && lon.is_finite()
        && (-90.0..=90.0).contains(&lat)
        && (-180.0..=180.0).contains(&lon)
        && !(lat == 0.0 && lon == 0.0);
    ok.then_some(GeoPoint { lat, lon })
}

fn user_comment(exif: &Exif) -> Option<String> {
    let Value::Undefined(bytes, _) = &exif.get_field(Tag::UserComment, In::PRIMARY)?.value else {
        return None;
    };
    let body = bytes.strip_prefix(b"ASCII\0\0\0")?;
    let text = String::from_utf8_lossy(body)
        .trim_matches(['\0', ' '])
        .to_owned();
    (!text.is_empty()).then_some(text)
}

fn from_exif(exif: &Exif) -> ExifData {
    let orientation = uint(exif, Tag::Orientation)
        .and_then(|v| u8::try_from(v).ok())
        .filter(|v| (1..=8).contains(v))
        .unwrap_or(1);
    let gps = dms(exif, Tag::GPSLatitude, Tag::GPSLatitudeRef, 'S')
        .zip(dms(exif, Tag::GPSLongitude, Tag::GPSLongitudeRef, 'W'))
        .and_then(|(lat, lon)| valid_gps(lat, lon));
    let gps_alt = gps.and_then(|_| {
        let alt = rational(exif, Tag::GPSAltitude)?;
        let below = uint(exif, Tag::GPSAltitudeRef) == Some(1);
        Some(if below { -alt } else { alt })
    });
    ExifData {
        orientation,
        date_time_original: datetime(exif, Tag::DateTimeOriginal),
        date_time_digitized: datetime(exif, Tag::DateTimeDigitized),
        offset: ascii(exif, Tag::OffsetTimeOriginal),
        make: ascii(exif, Tag::Make),
        model: ascii(exif, Tag::Model),
        lens: ascii(exif, Tag::LensModel),
        iso: uint(exif, Tag::PhotographicSensitivity),
        f_number: rational(exif, Tag::FNumber),
        exposure_s: rational(exif, Tag::ExposureTime),
        focal_mm: rational(exif, Tag::FocalLength),
        gps,
        gps_alt,
        description: ascii(exif, Tag::ImageDescription).or_else(|| user_comment(exif)),
    }
}

/// Reads the EXIF block of `path`. Missing or unparsable EXIF is not an error: the result is
/// then [`ExifData::default`] with orientation 1.
pub fn read_exif(path: &Path) -> ExifData {
    let parsed = File::open(path).ok().and_then(|f| {
        exif::Reader::new()
            .read_from_container(&mut BufReader::new(f))
            .ok()
    });
    match parsed {
        Some(exif) => from_exif(&exif),
        None => ExifData {
            orientation: 1,
            ..ExifData::default()
        },
    }
}

/// EXIF orientation of the file (1..=8); 1 when absent or unreadable.
pub(crate) fn read_orientation(path: &Path) -> u8 {
    read_exif(path).orientation
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exif_datetime_parsing() {
        let dt = |y, m, d, h, mi, s| {
            NaiveDate::from_ymd_opt(y, m, d).and_then(|d| d.and_hms_opt(h, mi, s))
        };
        assert_eq!(
            exif_datetime("2021:07:01 12:00:00"),
            dt(2021, 7, 1, 12, 0, 0)
        );
        assert_eq!(
            exif_datetime(" 2024:01:31 23:59:59 "),
            dt(2024, 1, 31, 23, 59, 59)
        );
        assert_eq!(exif_datetime("0000:00:00 00:00:00"), None);
        assert_eq!(exif_datetime("2021:13:01 12:00:00"), None);
        assert_eq!(exif_datetime("garbage"), None);
        assert_eq!(exif_datetime(""), None);
    }

    #[test]
    fn gps_validation() {
        assert!(valid_gps(38.7223, -9.1393).is_some());
        assert!(valid_gps(0.0, 0.0).is_none(), "null island is rejected");
        assert!(valid_gps(0.0, 10.0).is_some());
        assert!(valid_gps(91.0, 0.0).is_none());
        assert!(valid_gps(0.0, -181.0).is_none());
        assert!(valid_gps(f64::NAN, 1.0).is_none());
    }
}
