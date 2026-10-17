//! Splits a query string into terms (SPEC §6.8.1).

use std::ops::Range;

/// One whitespace-separated term of a query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Term {
    /// Byte range in the source string, for diagnostics.
    pub span: Range<usize>,
    /// Leading `-`.
    pub negated: bool,
    /// Lower-cased key of a `key:value` term.
    pub key: Option<String>,
    /// The word, phrase or filter value (without surrounding quotes).
    pub text: String,
    /// Whether `text` was written in quotes.
    pub quoted: bool,
    /// A quoted phrase was never closed.
    pub unterminated: bool,
}

struct Cursor<'a> {
    src: &'a str,
    pos: usize,
}

impl Cursor<'_> {
    fn peek(&self) -> Option<char> {
        self.src[self.pos..].chars().next()
    }

    fn bump(&mut self) -> Option<char> {
        let c = self.peek()?;
        self.pos += c.len_utf8();
        Some(c)
    }

    fn take_while(&mut self, f: impl Fn(char) -> bool) -> &str {
        let start = self.pos;
        while self.peek().is_some_and(&f) {
            self.bump();
        }
        &self.src[start..self.pos]
    }

    /// Reads a `"`-quoted string; the opening quote is the next char.
    fn quoted(&mut self) -> (String, bool) {
        self.bump();
        let start = self.pos;
        while let Some(c) = self.peek() {
            if c == '"' {
                let text = self.src[start..self.pos].to_owned();
                self.bump();
                return (text, false);
            }
            self.bump();
        }
        (self.src[start..].to_owned(), true)
    }
}

fn is_key_char(c: char) -> bool {
    c.is_ascii_alphabetic()
}

/// Tokenizes `src`. Never fails: anything unrecognised becomes a plain word.
pub(crate) fn lex(src: &str) -> Vec<Term> {
    let mut cur = Cursor { src, pos: 0 };
    let mut out = Vec::new();
    loop {
        cur.take_while(char::is_whitespace);
        let start = cur.pos;
        if cur.peek().is_none() {
            break;
        }
        let mut negated = false;
        if cur.peek() == Some('-') {
            let after = src[cur.pos + 1..].chars().next();
            if after.is_some_and(|c| !c.is_whitespace()) {
                negated = true;
                cur.bump();
            }
        }
        let mut term = Term {
            span: start..start,
            negated,
            key: None,
            text: String::new(),
            quoted: false,
            unterminated: false,
        };
        if cur.peek() == Some('"') {
            let (text, unterminated) = cur.quoted();
            term.text = text;
            term.quoted = true;
            term.unterminated = unterminated;
        } else {
            let word_start = cur.pos;
            let key = cur.take_while(is_key_char).to_owned();
            if !key.is_empty() && cur.peek() == Some(':') {
                cur.bump();
                term.key = Some(key.to_ascii_lowercase());
                if cur.peek() == Some('"') {
                    let (text, unterminated) = cur.quoted();
                    term.text = text;
                    term.quoted = true;
                    term.unterminated = unterminated;
                } else {
                    term.text = cur.take_while(|c| !c.is_whitespace()).to_owned();
                }
            } else {
                cur.take_while(|c| !c.is_whitespace());
                term.text = src[word_start..cur.pos].to_owned();
            }
        }
        term.span = start..cur.pos;
        out.push(term);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn simple(src: &str) -> Vec<(bool, Option<&'static str>, String)> {
        lex(src)
            .into_iter()
            .map(|t| {
                let key = t.key.as_deref().map(|k| match k {
                    "tag" => "tag",
                    "rating" => "rating",
                    _ => "other",
                });
                (t.negated, key, t.text)
            })
            .collect()
    }

    #[test]
    fn splits_words_filters_and_phrases() {
        assert_eq!(
            simple("beach  tag:sun -tag:work \"red car\" rating:>=3"),
            vec![
                (false, None, "beach".into()),
                (false, Some("tag"), "sun".into()),
                (true, Some("tag"), "work".into()),
                (false, None, "red car".into()),
                (false, Some("rating"), ">=3".into()),
            ]
        );
    }

    #[test]
    fn quoted_filter_values_keep_spaces() {
        let t = lex("camera:\"Canon EOS\" x");
        assert_eq!(t[0].text, "Canon EOS");
        assert!(t[0].quoted && t[0].key.as_deref() == Some("camera"));
        assert_eq!(t[1].text, "x");
        assert_eq!(
            &"camera:\"Canon EOS\" x"[t[0].span.clone()],
            "camera:\"Canon EOS\""
        );
    }

    #[test]
    fn lone_dash_and_unterminated_quotes() {
        assert_eq!(lex("-")[0].text, "-");
        assert!(!lex("-")[0].negated);
        let t = lex("\"open ended");
        assert!(t[0].unterminated && t[0].text == "open ended");
        assert!(lex("   ").is_empty());
        assert_eq!(lex("é-ü ✓")[1].text, "✓");
    }

    #[test]
    fn colon_without_key_is_a_word() {
        assert_eq!(lex(":x")[0].key, None);
        assert_eq!(lex("12:30")[0].key, None);
        assert_eq!(lex("a:")[0].text, "");
    }
}
