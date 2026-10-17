//! A forgiving INI reader that keeps section and key order.

/// One `[section]` with its `key=value` lines in file order.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub(crate) struct Section {
    pub name: String,
    pub entries: Vec<(String, String)>,
}

impl Section {
    /// The last value of `key` (case-insensitive).
    pub fn get(&self, key: &str) -> Option<&str> {
        self.entries
            .iter()
            .rev()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }
}

/// Splits `text` into sections. Lines before the first section, blank lines and comments are
/// ignored; values keep everything after the first `=`.
pub(crate) fn read_sections(text: &str) -> Vec<Section> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut sections: Vec<Section> = Vec::new();
    for raw in text.lines() {
        let line = raw.trim_end_matches('\r').trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            sections.push(Section {
                name: name.trim().to_owned(),
                entries: Vec::new(),
            });
        } else if let (Some(section), Some((key, value))) =
            (sections.last_mut(), line.split_once('='))
        {
            section
                .entries
                .push((key.trim().to_owned(), value.trim().to_owned()));
        }
    }
    sections
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sections_keep_order_and_equals_in_values() {
        let s = read_sections("\u{feff}junk=1\r\n[A]\r\nx=1\r\n; note\r\ny = a=b \r\n\r\n[B]\r\n");
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].name, "A");
        assert_eq!(
            s[0].entries,
            [("x".into(), "1".into()), ("y".into(), "a=b".into())]
        );
        assert_eq!(s[0].get("Y"), Some("a=b"));
        assert!(s[1].entries.is_empty());
    }
}
