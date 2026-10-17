//! Minimal EXIF (TIFF-in-APP1) writer: IFD0 + ExifIFD + GPS IFD, little endian.
//!
//! Hand-built so fixtures are bit-for-bit reproducible and fully under our control (SPEC §12.3).

use super::manifest::ExifSpec;
use anyhow::{Context, Result, bail};

const ASCII: u16 = 2;
const BYTE: u16 = 1;
const SHORT: u16 = 3;
const LONG: u16 = 4;
const RATIONAL: u16 = 5;

struct Entry {
    tag: u16,
    typ: u16,
    count: u32,
    data: Vec<u8>,
}

impl Entry {
    fn ascii(tag: u16, s: &str) -> Self {
        let mut data = s.as_bytes().to_vec();
        data.push(0);
        Self {
            tag,
            typ: ASCII,
            count: data.len() as u32,
            data,
        }
    }

    fn short(tag: u16, v: u16) -> Self {
        Self {
            tag,
            typ: SHORT,
            count: 1,
            data: v.to_le_bytes().to_vec(),
        }
    }

    fn long(tag: u16, v: u32) -> Self {
        Self {
            tag,
            typ: LONG,
            count: 1,
            data: v.to_le_bytes().to_vec(),
        }
    }

    fn byte(tag: u16, v: &[u8]) -> Self {
        Self {
            tag,
            typ: BYTE,
            count: v.len() as u32,
            data: v.to_vec(),
        }
    }

    fn rationals(tag: u16, v: &[(u32, u32)]) -> Self {
        let data = v
            .iter()
            .flat_map(|(n, d)| n.to_le_bytes().into_iter().chain(d.to_le_bytes()))
            .collect();
        Self {
            tag,
            typ: RATIONAL,
            count: v.len() as u32,
            data,
        }
    }
}

struct Ifd(Vec<Entry>);

impl Ifd {
    fn size(&self) -> u32 {
        let table = 2 + 12 * self.0.len() as u32 + 4;
        let extra: u32 = self
            .0
            .iter()
            .filter(|e| e.data.len() > 4)
            .map(|e| (e.data.len() as u32 + 1) & !1)
            .sum();
        table + extra
    }

    fn set_long(&mut self, tag: u16, v: u32) {
        if let Some(e) = self.0.iter_mut().find(|e| e.tag == tag) {
            e.data = v.to_le_bytes().to_vec();
        }
    }

    fn serialize(&mut self, offset: u32) -> Vec<u8> {
        self.0.sort_by_key(|e| e.tag);
        let table_len = 2 + 12 * self.0.len() as u32 + 4;
        let mut out = Vec::new();
        out.extend((self.0.len() as u16).to_le_bytes());
        let mut extra = Vec::new();
        for e in &self.0 {
            out.extend(e.tag.to_le_bytes());
            out.extend(e.typ.to_le_bytes());
            out.extend(e.count.to_le_bytes());
            if e.data.len() <= 4 {
                let mut inline = e.data.clone();
                inline.resize(4, 0);
                out.extend(inline);
            } else {
                out.extend((offset + table_len + extra.len() as u32).to_le_bytes());
                extra.extend(&e.data);
                if extra.len() % 2 == 1 {
                    extra.push(0);
                }
            }
        }
        out.extend(0u32.to_le_bytes());
        out.extend(extra);
        out
    }
}

fn parse_exposure(s: &str) -> Result<(u32, u32)> {
    if let Some((n, d)) = s.split_once('/') {
        return Ok((n.trim().parse()?, d.trim().parse()?));
    }
    let secs: f64 = s.trim().parse()?;
    Ok(((secs * 1_000_000.0).round() as u32, 1_000_000))
}

fn exif_datetime(iso: &str) -> Result<String> {
    let (d, t) = iso
        .split_once('T')
        .with_context(|| format!("bad datetime {iso:?}"))?;
    Ok(format!("{} {t}", d.replace('-', ":")))
}

fn dms(deg: f64) -> [(u32, u32); 3] {
    let a = deg.abs();
    let d = a.floor();
    let m = ((a - d) * 60.0).floor();
    let s = ((a - d) * 60.0 - m) * 60.0;
    [
        (d as u32, 1),
        (m as u32, 1),
        ((s * 10_000.0).round() as u32, 10_000),
    ]
}

fn rational(v: f64) -> (u32, u32) {
    ((v * 1000.0).round() as u32, 1000)
}

/// Builds the payload of an APP1 segment (`Exif\0\0` + TIFF) for `spec`.
pub fn build_exif_payload(spec: &ExifSpec) -> Result<Vec<u8>> {
    let mut ifd0 = Vec::new();
    if let Some(v) = &spec.description {
        ifd0.push(Entry::ascii(0x010E, v));
    }
    if let Some(v) = &spec.make {
        ifd0.push(Entry::ascii(0x010F, v));
    }
    if let Some(v) = &spec.model {
        ifd0.push(Entry::ascii(0x0110, v));
    }
    if let Some(v) = spec.orientation {
        if !(1..=8).contains(&v) {
            bail!("orientation {v} out of range");
        }
        ifd0.push(Entry::short(0x0112, v));
    }
    let taken = spec.taken.as_deref().map(exif_datetime).transpose()?;
    if let Some(t) = &taken {
        ifd0.push(Entry::ascii(0x0132, t));
    }

    let mut exif = Vec::new();
    if let Some(v) = &spec.exposure {
        exif.push(Entry::rationals(0x829A, &[parse_exposure(v)?]));
    }
    if let Some(v) = spec.f_number {
        exif.push(Entry::rationals(0x829D, &[rational(v)]));
    }
    if let Some(v) = spec.iso {
        exif.push(Entry::short(0x8827, v));
    }
    if let Some(t) = &taken {
        exif.push(Entry::ascii(0x9003, t));
        exif.push(Entry::ascii(0x9004, t));
    }
    if let Some(v) = &spec.offset {
        exif.push(Entry::ascii(0x9011, v));
    }
    if let Some(v) = spec.focal_mm {
        exif.push(Entry::rationals(0x920A, &[rational(v)]));
    }
    if let Some(v) = &spec.lens {
        exif.push(Entry::ascii(0xA434, v));
    }

    let gps = spec.gps.map(|[lat, lon]| {
        let mut g = vec![
            Entry::byte(0x0000, &[2, 3, 0, 0]),
            Entry::ascii(0x0001, if lat < 0.0 { "S" } else { "N" }),
            Entry::rationals(0x0002, &dms(lat)),
            Entry::ascii(0x0003, if lon < 0.0 { "W" } else { "E" }),
            Entry::rationals(0x0004, &dms(lon)),
        ];
        if let Some(alt) = spec.altitude {
            g.push(Entry::byte(0x0005, &[u8::from(alt < 0.0)]));
            g.push(Entry::rationals(0x0006, &[rational(alt.abs())]));
        }
        g
    });

    let has_exif = !exif.is_empty();
    if has_exif {
        ifd0.push(Entry::long(0x8769, 0));
    }
    if gps.is_some() {
        ifd0.push(Entry::long(0x8825, 0));
    }
    let mut ifd0 = Ifd(ifd0);
    let mut exif_ifd = Ifd(exif);
    let mut gps_ifd = Ifd(gps.unwrap_or_default());

    let exif_off = 8 + ifd0.size();
    let gps_off = exif_off + if has_exif { exif_ifd.size() } else { 0 };
    ifd0.set_long(0x8769, exif_off);
    ifd0.set_long(0x8825, gps_off);

    let mut payload = b"Exif\0\0II*\0".to_vec();
    payload.extend(8u32.to_le_bytes());
    payload.extend(ifd0.serialize(8));
    if has_exif {
        payload.extend(exif_ifd.serialize(exif_off));
    }
    if !gps_ifd.0.is_empty() {
        payload.extend(gps_ifd.serialize(gps_off));
    }
    Ok(payload)
}

/// Inserts an EXIF APP1 segment into a JPEG (after SOI and any JFIF APP0).
pub fn insert_exif(jpeg: &[u8], spec: &ExifSpec) -> Result<Vec<u8>> {
    if jpeg.len() < 4 || jpeg[..2] != [0xFF, 0xD8] {
        bail!("not a JPEG (missing SOI)");
    }
    let payload = build_exif_payload(spec)?;
    let seg_len = u16::try_from(payload.len() + 2).context("EXIF payload too large for APP1")?;
    let mut at = 2;
    if jpeg[2] == 0xFF && jpeg[3] == 0xE0 {
        at = 4 + usize::from(u16::from_be_bytes([jpeg[4], jpeg[5]]));
    }
    let mut out = Vec::with_capacity(jpeg.len() + payload.len() + 4);
    out.extend(&jpeg[..at]);
    out.extend([0xFF, 0xE1]);
    out.extend(seg_len.to_be_bytes());
    out.extend(&payload);
    out.extend(&jpeg[at..]);
    Ok(out)
}
