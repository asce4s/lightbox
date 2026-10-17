//! `taken_at` resolution with the SPEC §6.2.2 precedence.

use chrono::{DateTime, Local, NaiveDate, NaiveDateTime};
use lb_core::TakenAtSource;

/// A resolved capture time and where it came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TakenAt {
    /// Naive local capture time.
    pub at: NaiveDateTime,
    /// The rule that produced `at`.
    pub source: TakenAtSource,
}

fn digits(b: &[u8], start: usize, n: usize) -> Option<u32> {
    let slice = b.get(start..start + n)?;
    slice.iter().all(u8::is_ascii_digit).then(|| {
        slice
            .iter()
            .fold(0_u32, |acc, d| acc * 10 + u32::from(d - b'0'))
    })
}

fn build(y: u32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> Option<NaiveDateTime> {
    let year = i32::try_from(y)
        .ok()
        .filter(|y| (1970..=2100).contains(y))?;
    NaiveDate::from_ymd_opt(year, mo, d)?.and_hms_opt(h, mi, s)
}

/// `YYYYMMDD` + `_`/`-` + `HHMMSS` (IMG_, PXL_, Screenshot_, VID_ ...).
fn compact_at(b: &[u8], i: usize) -> Option<NaiveDateTime> {
    if !matches!(b.get(i + 8), Some(b'_' | b'-')) {
        return None;
    }
    build(
        digits(b, i, 4)?,
        digits(b, i + 4, 2)?,
        digits(b, i + 6, 2)?,
        digits(b, i + 9, 2)?,
        digits(b, i + 11, 2)?,
        digits(b, i + 13, 2)?,
    )
}

/// `YYYY-MM-DD` + space/`_`/`T` + `HH.MM.SS` (also `:` or `-` between the time parts).
fn dashed_at(b: &[u8], i: usize) -> Option<NaiveDateTime> {
    let sep = |at: usize, set: &[u8]| b.get(at).filter(|c| set.contains(c)).map(|_| ());
    sep(i + 4, b"-")?;
    sep(i + 7, b"-")?;
    sep(i + 10, b" _T")?;
    sep(i + 13, b".:-")?;
    sep(i + 16, b".:-")?;
    build(
        digits(b, i, 4)?,
        digits(b, i + 5, 2)?,
        digits(b, i + 8, 2)?,
        digits(b, i + 11, 2)?,
        digits(b, i + 14, 2)?,
        digits(b, i + 17, 2)?,
    )
}

/// Extracts a timestamp from a file name such as `IMG_20240131_235959.jpg`,
/// `2024-01-31 23.59.59.jpg`, `PXL_20240131_235959123.jpg` or `Screenshot_20240131-235959.png`.
pub fn filename_date(file_name: &str) -> Option<NaiveDateTime> {
    let b = file_name.as_bytes();
    (0..b.len()).find_map(|i| {
        let boundary = i == 0 || !b[i - 1].is_ascii_digit();
        if !boundary || !b[i].is_ascii_digit() {
            return None;
        }
        compact_at(b, i).or_else(|| dashed_at(b, i))
    })
}

/// Converts file mtime (unix seconds) to naive local time.
pub fn mtime_to_naive_local(unix_seconds: i64) -> Option<NaiveDateTime> {
    DateTime::from_timestamp(unix_seconds, 0).map(|utc| utc.with_timezone(&Local).naive_local())
}

/// Candidate capture times from file contents.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct DateSources {
    /// EXIF `DateTimeOriginal`.
    pub exif_original: Option<NaiveDateTime>,
    /// EXIF `DateTimeDigitized`.
    pub exif_digitized: Option<NaiveDateTime>,
    /// Container creation time of a video.
    pub video_creation: Option<NaiveDateTime>,
}

/// Picks the capture time: EXIF original, EXIF digitized, video creation time, file-name
/// pattern, then `mtime` (SPEC §6.2.2). `mtime` is the file's modification time as naive local.
pub fn resolve_taken_at(sources: &DateSources, file_name: &str, mtime: NaiveDateTime) -> TakenAt {
    let from = |at, source| TakenAt { at, source };
    if let Some(at) = sources.exif_original.or(sources.exif_digitized) {
        return from(at, TakenAtSource::Exif);
    }
    if let Some(at) = sources.video_creation {
        return from(at, TakenAtSource::Video);
    }
    if let Some(at) = filename_date(file_name) {
        return from(at, TakenAtSource::Filename);
    }
    from(mtime, TakenAtSource::Mtime)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(y: i32, mo: u32, d: u32, h: u32, mi: u32, s: u32) -> NaiveDateTime {
        NaiveDate::from_ymd_opt(y, mo, d)
            .and_then(|d| d.and_hms_opt(h, mi, s))
            .unwrap()
    }

    #[test]
    fn filename_date_patterns() {
        let cases = [
            ("IMG_20240131_235959.jpg", Some(dt(2024, 1, 31, 23, 59, 59))),
            ("2024-01-31 23.59.59.jpg", Some(dt(2024, 1, 31, 23, 59, 59))),
            (
                "PXL_20240131_235959123.jpg",
                Some(dt(2024, 1, 31, 23, 59, 59)),
            ),
            (
                "Screenshot_20240131-235959.png",
                Some(dt(2024, 1, 31, 23, 59, 59)),
            ),
            ("IMG_20220315_101500.jpg", Some(dt(2022, 3, 15, 10, 15, 0))),
            ("VID_20200229_000000.mp4", Some(dt(2020, 2, 29, 0, 0, 0))),
            ("2024-01-31_23-59-59.jpg", Some(dt(2024, 1, 31, 23, 59, 59))),
            (
                "trip 2024-01-31T23:59:59.jpg",
                Some(dt(2024, 1, 31, 23, 59, 59)),
            ),
            ("IMG_20230231_101500.jpg", None),
            ("IMG_20240131_256000.jpg", None),
            ("IMG_12345678_123456.jpg", None),
            ("IMG_0001.JPG", None),
            ("beach_01.jpg", None),
            ("120240131_235959.jpg", None),
            ("café ☕ 写真.jpg", None),
            ("", None),
        ];
        for (name, expected) in cases {
            assert_eq!(filename_date(name), expected, "{name:?}");
        }
    }

    #[test]
    fn precedence_table() {
        let mtime = dt(2020, 1, 1, 0, 0, 0);
        let exif_o = dt(2021, 7, 1, 12, 0, 0);
        let exif_d = dt(2021, 7, 2, 12, 0, 0);
        let video = dt(2019, 5, 5, 5, 5, 5);
        let name = "IMG_20220315_101500.jpg";
        let file = dt(2022, 3, 15, 10, 15, 0);
        let all = DateSources {
            exif_original: Some(exif_o),
            exif_digitized: Some(exif_d),
            video_creation: Some(video),
        };
        let table = [
            (all, name, exif_o, TakenAtSource::Exif),
            (
                DateSources {
                    exif_original: None,
                    ..all
                },
                name,
                exif_d,
                TakenAtSource::Exif,
            ),
            (
                DateSources {
                    exif_original: None,
                    exif_digitized: None,
                    ..all
                },
                name,
                video,
                TakenAtSource::Video,
            ),
            (DateSources::default(), name, file, TakenAtSource::Filename),
            (
                DateSources::default(),
                "plain.jpg",
                mtime,
                TakenAtSource::Mtime,
            ),
        ];
        for (sources, file_name, at, source) in table {
            assert_eq!(
                resolve_taken_at(&sources, file_name, mtime),
                TakenAt { at, source },
                "{file_name} {sources:?}"
            );
        }
    }

    #[test]
    fn mtime_conversion_is_local_time() {
        let converted = mtime_to_naive_local(1_577_836_800).unwrap();
        let expected = DateTime::from_timestamp(1_577_836_800, 0)
            .unwrap()
            .with_timezone(&Local)
            .naive_local();
        assert_eq!(converted, expected);
    }
}
