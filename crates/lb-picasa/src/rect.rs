//! The 64-bit rectangle encoding used by `rect64(...)` and `crop64=1,...`.

use lb_core::NormRect;

/// Parses up to 16 hex digits; Picasa drops leading zeros.
pub fn decode_hex_u64(hex: &str) -> Option<u64> {
    let hex = hex.trim();
    if hex.is_empty() || hex.len() > 16 || !hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        return None;
    }
    u64::from_str_radix(hex, 16).ok()
}

/// Decodes four 16-bit values (left, top, right, bottom) as fractions of 65535 into a
/// normalized rectangle. Returns `None` for malformed or empty rectangles.
pub fn decode_rect64(hex: &str) -> Option<NormRect> {
    let v = decode_hex_u64(hex)?;
    let part = |shift: u32| f32::from(((v >> shift) & 0xffff) as u16) / 65535.0;
    let (left, top, right, bottom) = (part(48), part(32), part(16), part(0));
    if right <= left || bottom <= top {
        return None;
    }
    Some(NormRect {
        x: left,
        y: top,
        w: right - left,
        h: bottom - top,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    type Row = (&'static str, Option<(f32, f32, f32, f32)>);

    #[test]
    fn rect64_decode() {
        let table: &[Row] = &[
            ("0000000000000000", None),
            (
                "0000000000010001",
                Some((0.0, 0.0, 0.000_015_259, 0.000_015_259)),
            ),
            ("ffffffffffffffff", None),
            ("00000000ffffffff", Some((0.0, 0.0, 1.0, 1.0))),
            (
                "0000000080008000",
                Some((0.0, 0.0, 0.500_007_6, 0.500_007_6)),
            ),
            ("8000800080008000", None),
            (
                "40004000c000c000",
                Some((0.250_003_8, 0.250_003_8, 0.500_007_6, 0.500_007_6)),
            ),
            ("zzzz", None),
            ("", None),
            ("00000000000000000", None),
        ];
        for (hex, want) in table {
            let got = decode_rect64(hex).map(|r| (r.x, r.y, r.w, r.h));
            match (got, want) {
                (None, None) => {}
                (Some(g), Some(w)) => {
                    for (a, b) in [(g.0, w.0), (g.1, w.1), (g.2, w.2), (g.3, w.3)] {
                        assert!((a - b).abs() < 1e-5, "{hex}: {g:?} vs {w:?}");
                    }
                }
                other => panic!("{hex}: {other:?}"),
            }
        }
    }

    #[test]
    fn leading_zeros_may_be_dropped() {
        assert_eq!(decode_hex_u64("ff"), Some(255));
        assert_eq!(
            decode_rect64("ffff0000ffff"),
            decode_rect64("0000ffff0000ffff")
        );
    }

    #[test]
    fn fixture_rect_is_inside_the_image() {
        let r = decode_rect64("3f845bcb59418507").unwrap();
        assert!(r.x >= 0.0 && r.y >= 0.0 && r.x + r.w <= 1.0 && r.y + r.h <= 1.0);
        assert!((r.x - 0.2480).abs() < 1e-3);
        assert!((r.y - 0.3578).abs() < 1e-3);
    }
}
