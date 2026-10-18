//! Natural (human) filename order (SPEC-VIEWER §V1, §V14).
//!
//! Digit runs compare as integers. When two runs have the same value, the
//! shorter one (fewer leading zeros) comes first, so `img2` < `img02` < `img10`.
//! Letters compare by Unicode simple case folding. The extension is part of the
//! string, not a separate key.

use std::cmp::Ordering;
use std::ffi::OsStr;

/// Natural order of two file names.
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a, b);
    loop {
        match (next_chunk(a), next_chunk(b)) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some((ca, rest_a)), Some((cb, rest_b))) => {
                let ord = if is_digits(ca) && is_digits(cb) {
                    cmp_num(ca, cb)
                } else {
                    cmp_text(ca, cb)
                };
                if ord != Ordering::Equal {
                    return ord;
                }
                a = rest_a;
                b = rest_b;
            }
        }
    }
}

/// [`natural_cmp`] for paths. Non-UTF-8 names fall back to the platform order.
pub fn natural_cmp_os(a: &OsStr, b: &OsStr) -> Ordering {
    match (a.to_str(), b.to_str()) {
        (Some(a), Some(b)) => natural_cmp(a, b),
        _ => a.cmp(b),
    }
}

fn next_chunk(s: &str) -> Option<(&str, &str)> {
    let mut chars = s.char_indices();
    let (_, first) = chars.next()?;
    let digit = first.is_ascii_digit();
    let mut end = s.len();
    for (i, c) in chars {
        if c.is_ascii_digit() != digit {
            end = i;
            break;
        }
    }
    Some((&s[..end], &s[end..]))
}

fn is_digits(s: &str) -> bool {
    s.starts_with(|c: char| c.is_ascii_digit())
}

fn cmp_text(a: &str, b: &str) -> Ordering {
    a.to_lowercase().cmp(&b.to_lowercase())
}

/// Significant digits first, then fewer leading zeros.
fn cmp_num(a: &str, b: &str) -> Ordering {
    let a_sig = a.trim_start_matches('0');
    let b_sig = b.trim_start_matches('0');
    a_sig
        .len()
        .cmp(&b_sig.len())
        .then_with(|| a_sig.cmp(b_sig))
        .then_with(|| a.len().cmp(&b.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn natural_sort_table() {
        let less = [
            ("img2", "IMG10"),
            ("IMG2.jpg", "img10.jpg"),
            ("img2.png", "IMG10.jpg"),
            ("file1", "file01"),
            ("file01", "file001"),
            ("file2", "file10"),
            ("a.2", "a.10"),
            ("readme", "readme.txt"),
            ("a.jpg", "a.png"),
            ("cafe", "café"),
            ("photo", "фото"),
            ("b", "é"),
            ("Äpfel", "äpfel2"),
            ("010", "0010"),
            ("naïve", "naïve2"),
        ];
        for (a, b) in less {
            assert_eq!(natural_cmp(a, b), Ordering::Less, "{a:?} < {b:?}");
            assert_eq!(natural_cmp(b, a), Ordering::Greater, "{b:?} > {a:?}");
        }

        let equal = [
            ("IMG2", "img2"),
            ("Ä", "ä"),
            ("photo.JPG", "PHOTO.jpg"),
            ("010", "010"),
            ("café", "CAFÉ"),
        ];
        for (a, b) in equal {
            assert_eq!(natural_cmp(a, b), Ordering::Equal, "{a:?} == {b:?}");
        }

        let mut names = ["IMG10.jpg", "img2.jpg", "IMG02.jpg", "img1.jpg"];
        names.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(names, ["img1.jpg", "img2.jpg", "IMG02.jpg", "IMG10.jpg"]);

        assert_eq!(
            natural_cmp_os(OsStr::new("img2.jpg"), OsStr::new("IMG10.jpg")),
            Ordering::Less
        );
    }
}
