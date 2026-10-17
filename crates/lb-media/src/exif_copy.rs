//! Copying and patching EXIF blocks for exported files (SPEC §6.10).
//!
//! The TIFF structure is patched in place: values are overwritten, and removed data (GPS,
//! thumbnail) is zeroed rather than left behind, so privacy-relevant bytes do not survive.

use std::fs::File;
use std::io::BufReader;
use std::ops::Range;
use std::path::Path;

const TAG_ORIENTATION: u16 = 0x0112;
const TAG_EXIF_IFD: u16 = 0x8769;
const TAG_GPS_IFD: u16 = 0x8825;
const TAG_PIXEL_X: u16 = 0xA002;
const TAG_PIXEL_Y: u16 = 0xA003;
const TAG_THUMB_OFFSET: u16 = 0x0201;
const TAG_THUMB_LENGTH: u16 = 0x0202;

const SHORT: u16 = 3;
const LONG: u16 = 4;
const ENTRY: usize = 12;

/// What to change when copying an EXIF block onto re-encoded pixels.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct ExifRewrite {
    /// Remove the GPS IFD and its data.
    pub strip_gps: bool,
    /// New `Orientation` value, if the tag exists.
    pub orientation: Option<u16>,
    /// New `PixelXDimension` and `PixelYDimension`, if the tags exist.
    pub dimensions: Option<(u32, u32)>,
    /// Remove the embedded thumbnail (it shows the unedited picture).
    pub drop_thumbnail: bool,
}

/// The raw TIFF-structured EXIF block of `path` (any container the EXIF reader understands).
pub fn raw_exif(path: &Path) -> Option<Vec<u8>> {
    let file = File::open(path).ok()?;
    let exif = exif::Reader::new()
        .read_from_container(&mut BufReader::new(file))
        .ok()?;
    Some(exif.buf().to_vec())
}

struct Tiff {
    buf: Vec<u8>,
    little: bool,
}

fn type_size(typ: u16) -> usize {
    match typ {
        3 | 8 => 2,
        4 | 9 | 11 => 4,
        5 | 10 | 12 => 8,
        _ => 1,
    }
}

impl Tiff {
    fn new(buf: Vec<u8>) -> Option<Self> {
        let little = match buf.get(..4)? {
            [b'I', b'I', 42, 0] => true,
            [b'M', b'M', 0, 42] => false,
            _ => return None,
        };
        Some(Self { buf, little })
    }

    fn u16(&self, at: usize) -> Option<u16> {
        let b: [u8; 2] = self.buf.get(at..at.checked_add(2)?)?.try_into().ok()?;
        Some(if self.little {
            u16::from_le_bytes(b)
        } else {
            u16::from_be_bytes(b)
        })
    }

    fn u32(&self, at: usize) -> Option<u32> {
        let b: [u8; 4] = self.buf.get(at..at.checked_add(4)?)?.try_into().ok()?;
        Some(if self.little {
            u32::from_le_bytes(b)
        } else {
            u32::from_be_bytes(b)
        })
    }

    fn put_u16(&mut self, at: usize, v: u16) -> Option<()> {
        let b = if self.little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        };
        self.buf
            .get_mut(at..at.checked_add(2)?)?
            .copy_from_slice(&b);
        Some(())
    }

    fn put_u32(&mut self, at: usize, v: u32) -> Option<()> {
        let b = if self.little {
            v.to_le_bytes()
        } else {
            v.to_be_bytes()
        };
        self.buf
            .get_mut(at..at.checked_add(4)?)?
            .copy_from_slice(&b);
        Some(())
    }

    fn zero(&mut self, at: usize, len: usize) {
        if let Some(range) = at
            .checked_add(len)
            .and_then(|end| self.buf.get_mut(at..end))
        {
            range.fill(0);
        }
    }

    fn entry_count(&self, ifd: usize) -> Option<usize> {
        let n = usize::from(self.u16(ifd)?);
        let end = ifd.checked_add(2 + n * ENTRY + 4)?;
        (end <= self.buf.len()).then_some(n)
    }

    /// Offset of the entry for `tag` in the IFD at `ifd`.
    fn find(&self, ifd: usize, tag: u16) -> Option<usize> {
        let n = self.entry_count(ifd)?;
        (0..n)
            .map(|i| ifd + 2 + i * ENTRY)
            .find(|&e| self.u16(e) == Some(tag))
    }

    fn value_u32(&self, entry: usize) -> Option<usize> {
        let typ = self.u16(entry + 2)?;
        let v = if typ == SHORT {
            u32::from(self.u16(entry + 8)?)
        } else {
            self.u32(entry + 8)?
        };
        usize::try_from(v).ok()
    }

    /// Zeroes an IFD and the out-of-line values of its entries.
    fn wipe_ifd(&mut self, ifd: usize) {
        let Some(n) = self.entry_count(ifd) else {
            return;
        };
        for i in 0..n {
            let e = ifd + 2 + i * ENTRY;
            let (Some(typ), Some(count)) = (self.u16(e + 2), self.u32(e + 4)) else {
                continue;
            };
            let size = usize::try_from(count)
                .ok()
                .and_then(|c| c.checked_mul(type_size(typ)));
            if let (Some(size), Some(off)) = (size, self.u32(e + 8))
                && size > 4
                && let Ok(off) = usize::try_from(off)
            {
                self.zero(off, size);
            }
        }
        self.zero(ifd, 2 + n * ENTRY + 4);
    }

    fn remove_entry(&mut self, ifd: usize, entry: usize) -> Option<()> {
        let n = self.entry_count(ifd)?;
        let tail_end = ifd + 2 + n * ENTRY + 4;
        self.buf.copy_within(entry + ENTRY..tail_end, entry);
        self.zero(tail_end - ENTRY, ENTRY);
        self.put_u16(ifd, u16::try_from(n - 1).ok()?)
    }

    fn strip_gps(&mut self, ifd0: usize) -> Option<()> {
        let entry = self.find(ifd0, TAG_GPS_IFD)?;
        let gps = self.value_u32(entry)?;
        self.wipe_ifd(gps);
        self.remove_entry(ifd0, entry)
    }

    fn drop_thumbnail(&mut self, ifd0: usize) -> Option<()> {
        let n = self.entry_count(ifd0)?;
        let next_at = ifd0 + 2 + n * ENTRY;
        let ifd1 = usize::try_from(self.u32(next_at)?).ok()?;
        if ifd1 == 0 {
            return Some(());
        }
        self.put_u32(next_at, 0)?;
        if let (Some(off), Some(len)) = (
            self.find(ifd1, TAG_THUMB_OFFSET),
            self.find(ifd1, TAG_THUMB_LENGTH),
        ) && let (Some(off), Some(len)) = (self.value_u32(off), self.value_u32(len))
        {
            self.zero(off, len);
        }
        self.wipe_ifd(ifd1);
        Some(())
    }
}

/// Applies `rewrite` to a TIFF-structured EXIF block. `None` when the block is malformed.
pub fn rewrite_exif(tiff: &[u8], rewrite: &ExifRewrite) -> Option<Vec<u8>> {
    let mut t = Tiff::new(tiff.to_vec())?;
    let ifd0 = usize::try_from(t.u32(4)?).ok()?;
    t.entry_count(ifd0)?;
    if let Some(o) = rewrite.orientation
        && let Some(e) = t.find(ifd0, TAG_ORIENTATION)
    {
        t.put_u16(e + 2, SHORT)?;
        t.put_u32(e + 4, 1)?;
        t.put_u16(e + 8, o)?;
        t.put_u16(e + 10, 0)?;
    }
    if let Some((w, h)) = rewrite.dimensions
        && let Some(ptr) = t.find(ifd0, TAG_EXIF_IFD)
    {
        let exif_ifd = t.value_u32(ptr)?;
        for (tag, v) in [(TAG_PIXEL_X, w), (TAG_PIXEL_Y, h)] {
            if let Some(e) = t.find(exif_ifd, tag) {
                t.put_u16(e + 2, LONG)?;
                t.put_u32(e + 4, 1)?;
                t.put_u32(e + 8, v)?;
            }
        }
    }
    if rewrite.drop_thumbnail {
        t.drop_thumbnail(ifd0);
    }
    if rewrite.strip_gps {
        t.strip_gps(ifd0);
    }
    Some(t.buf)
}

const EXIF_HEADER: &[u8] = b"Exif\0\0";

/// Marker segments before the entropy-coded data: `(range of the whole segment, marker)`.
fn segments(jpeg: &[u8]) -> Vec<(Range<usize>, u8)> {
    let mut out = Vec::new();
    if jpeg.get(..2) != Some(&[0xFF, 0xD8]) {
        return out;
    }
    let mut at = 2;
    while let (Some(&0xFF), Some(&marker)) = (jpeg.get(at), jpeg.get(at + 1)) {
        if marker == 0xDA || marker == 0xD9 {
            break;
        }
        let Some(len) = jpeg
            .get(at + 2..at + 4)
            .map(|b| usize::from(u16::from_be_bytes([b[0], b[1]])))
        else {
            break;
        };
        let end = at + 2 + len;
        if len < 2 || end > jpeg.len() {
            break;
        }
        out.push((at..end, marker));
        at = end;
    }
    out
}

fn exif_segment(jpeg: &[u8]) -> Option<Range<usize>> {
    segments(jpeg)
        .into_iter()
        .find(|(range, marker)| {
            *marker == 0xE1 && jpeg.get(range.start + 4..range.start + 10) == Some(EXIF_HEADER)
        })
        .map(|(range, _)| range)
}

fn build_segment(tiff: &[u8]) -> Option<Vec<u8>> {
    let len = u16::try_from(tiff.len() + EXIF_HEADER.len() + 2).ok()?;
    let mut seg = vec![0xFF, 0xE1];
    seg.extend(len.to_be_bytes());
    seg.extend(EXIF_HEADER);
    seg.extend(tiff);
    Some(seg)
}

/// The EXIF block (TIFF structure) inside a JPEG file's bytes.
pub fn jpeg_exif(jpeg: &[u8]) -> Option<&[u8]> {
    let r = exif_segment(jpeg)?;
    jpeg.get(r.start + 10..r.end)
}

/// Inserts `tiff` as the EXIF segment of `jpeg`, replacing any existing one. `None` when the
/// block does not fit in one segment or `jpeg` is not a JPEG.
pub fn embed_exif(jpeg: &[u8], tiff: &[u8]) -> Option<Vec<u8>> {
    let seg = build_segment(tiff)?;
    let (mut out, rest_from) = match exif_segment(jpeg) {
        Some(r) => {
            let mut out = jpeg[..r.start].to_vec();
            out.extend(&seg);
            (out, r.end)
        }
        None => {
            let after = segments(jpeg)
                .iter()
                .find(|(_, m)| *m != 0xE0)
                .map_or(2, |(r, _)| r.start);
            if jpeg.get(..2) != Some(&[0xFF, 0xD8]) {
                return None;
            }
            let mut out = jpeg[..after].to_vec();
            out.extend(&seg);
            (out, after)
        }
    };
    out.extend(&jpeg[rest_from..]);
    Some(out)
}

/// Removes the EXIF segment from a JPEG, or rewrites it with `f` (which gets the TIFF block and
/// may return `None` to drop it). Pixel data is untouched.
pub fn map_jpeg_exif(jpeg: &[u8], f: impl FnOnce(&[u8]) -> Option<Vec<u8>>) -> Option<Vec<u8>> {
    let Some(r) = exif_segment(jpeg) else {
        return Some(jpeg.to_vec());
    };
    let mut out = jpeg[..r.start].to_vec();
    if let Some(seg) = f(&jpeg[r.start + 10..r.end]).and_then(|t| build_segment(&t)) {
        out.extend(seg);
    }
    out.extend(&jpeg[r.end..]);
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::read_exif;
    use lb_testkit::fixtures::exif_writer::insert_exif;
    use lb_testkit::fixtures::manifest::ExifSpec;

    fn jpeg_with(spec: &ExifSpec) -> Vec<u8> {
        let img = image::RgbImage::from_pixel(8, 6, image::Rgb([10, 200, 30]));
        let mut plain = std::io::Cursor::new(Vec::new());
        img.write_to(&mut plain, image::ImageFormat::Jpeg).unwrap();
        insert_exif(&plain.into_inner(), spec).unwrap()
    }

    fn spec() -> ExifSpec {
        ExifSpec {
            make: Some("Canon".into()),
            model: Some("EOS Test".into()),
            orientation: Some(6),
            gps: Some([48.8584, 2.2945]),
            altitude: Some(35.0),
            taken: Some("2021-06-15T12:00:00".into()),
            description: Some("a caption".into()),
            ..ExifSpec::default()
        }
    }

    fn read(jpeg: &[u8]) -> crate::ExifData {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.jpg");
        std::fs::write(&p, jpeg).unwrap();
        read_exif(&p)
    }

    #[test]
    fn rewrite_resets_orientation_and_strips_gps() {
        let jpeg = jpeg_with(&spec());
        let tiff = jpeg_exif(&jpeg).unwrap();
        let out = rewrite_exif(
            tiff,
            &ExifRewrite {
                strip_gps: true,
                orientation: Some(1),
                dimensions: Some((640, 480)),
                drop_thumbnail: true,
            },
        )
        .unwrap();
        let new = embed_exif(&jpeg, &out).unwrap();
        let data = read(&new);
        assert_eq!(data.orientation, 1);
        assert_eq!(data.gps, None);
        assert_eq!(data.make.as_deref(), Some("Canon"));
        assert_eq!(data.model.as_deref(), Some("EOS Test"));
        assert!(data.date_time_original.is_some());
        assert_eq!(data.description.as_deref(), Some("a caption"));
        let before = read(&jpeg);
        assert_eq!(before.orientation, 6);
        assert!(before.gps.is_some());
    }

    #[test]
    fn stripped_block_contains_no_gps_bytes() {
        let jpeg = jpeg_with(&spec());
        let tiff = jpeg_exif(&jpeg).unwrap();
        let out = rewrite_exif(
            tiff,
            &ExifRewrite {
                strip_gps: true,
                ..ExifRewrite::default()
            },
        )
        .unwrap();
        assert_eq!(out.len(), tiff.len(), "patched in place");
        assert_ne!(out, tiff);
        let exif = exif::Reader::new().read_raw(out).unwrap();
        assert!(
            exif.fields().all(|f| f.tag.context() != exif::Context::Gps),
            "no GPS field survives"
        );
    }

    #[test]
    fn keep_all_only_resets_orientation() {
        let jpeg = jpeg_with(&spec());
        let tiff = jpeg_exif(&jpeg).unwrap();
        let out = rewrite_exif(
            tiff,
            &ExifRewrite {
                orientation: Some(1),
                ..ExifRewrite::default()
            },
        )
        .unwrap();
        let data = read(&embed_exif(&jpeg, &out).unwrap());
        assert_eq!(data.orientation, 1);
        assert!(data.gps.is_some());
    }

    #[test]
    fn jpeg_segment_helpers() {
        let jpeg = jpeg_with(&spec());
        let stripped = map_jpeg_exif(&jpeg, |_| None).unwrap();
        assert!(jpeg_exif(&stripped).is_none());
        assert_eq!(read(&stripped).make, None);
        let kept = map_jpeg_exif(&jpeg, |t| Some(t.to_vec())).unwrap();
        assert_eq!(kept, jpeg);
        let plain = map_jpeg_exif(&stripped, |_| None).unwrap();
        assert_eq!(plain, stripped, "no EXIF to remove");
        let embedded = embed_exif(&stripped, jpeg_exif(&jpeg).unwrap()).unwrap();
        assert_eq!(read(&embedded).make.as_deref(), Some("Canon"));
        assert!(
            image::load_from_memory(&embedded).is_ok(),
            "still decodes after embedding"
        );
    }

    #[test]
    fn malformed_input_is_rejected_not_panicking() {
        assert!(rewrite_exif(b"", &ExifRewrite::default()).is_none());
        assert!(rewrite_exif(b"II*\0\xff\xff\xff\xff", &ExifRewrite::default()).is_none());
        assert!(rewrite_exif(b"XX*\0\0\0\0\0", &ExifRewrite::default()).is_none());
        assert!(embed_exif(b"not a jpeg", b"II*\0").is_none());
        assert!(jpeg_exif(b"\xff\xd8\xff\xe1\x00").is_none());
        let huge = vec![0u8; 70_000];
        assert!(embed_exif(&[0xFF, 0xD8, 0xFF, 0xD9], &huge).is_none());
    }
}
