#![allow(clippy::unwrap_used, clippy::expect_used)]

use lb_search::{QueryPlan, parse};
use proptest::prelude::*;
use std::fmt::Write;

const CASES: &[&str] = &[
    "",
    "   ",
    "beach",
    "bea",
    "b",
    "beach sunset",
    "\"red car\"",
    "\"unterminated",
    "-beach",
    "-\"red car\"",
    "-",
    "tag:work",
    "-tag:work",
    "tag:\"summer trip\"",
    "tag:",
    "TAG:Work",
    "album:Family",
    "-album:Family",
    "folder:2021",
    "folder:\"my photos\"",
    "folder:100%_off",
    "camera:\"Canon EOS\"",
    "lens:50mm",
    "person:\"Ana\"",
    "type:photo",
    "type:video",
    "type:audio",
    "ext:jpg",
    "ext:.PNG",
    "caption:holiday",
    "name:IMG_",
    "rating:3",
    "rating:>=3",
    "rating:>3",
    "rating:<=2",
    "rating:<1",
    "rating:=5",
    "rating:2..4",
    "rating:..2",
    "rating:abc",
    "date:2023-06..2023-08",
    "date:2023-06-15",
    "date:2023-06",
    "date:2023",
    "date:2023-12..",
    "date:..2020-02-29",
    "date:2023-02-30",
    "date:..",
    "date:9999-12-31",
    "year:2019",
    "year:19",
    "month:6",
    "month:June",
    "month:13",
    "starred:yes",
    "starred:no",
    "starred:maybe",
    "hidden:true",
    "-hidden:true",
    "has:gps",
    "has:caption",
    "has:faces",
    "has:edits",
    "has:tags",
    "-has:gps",
    "has:wings",
    "is:starred",
    "is:hidden",
    "is:edited",
    "is:video",
    "is:photo",
    "is:missing",
    "-is:missing",
    "is:unreadable",
    "colour:red",
    "beach rating:>=3 year:2021",
    "beach -tag:work \"red car\" has:gps",
    "http://example.com",
    "12:30",
    "a\"b",
    "'; DROP TABLE photos; --",
    "tag:x' OR 1=1 --",
    "名前 écran",
    "...",
];

fn render(plan: &QueryPlan) -> String {
    let mut out = String::new();
    writeln!(out, "  sql:  {}", plan.sql_where).unwrap();
    writeln!(out, "  args: {:?}", plan.params).unwrap();
    writeln!(out, "  fts:  {:?}", plan.fts).unwrap();
    if plan.include_hidden || plan.include_missing {
        writeln!(
            out,
            "  include: hidden={} missing={}",
            plan.include_hidden, plan.include_missing
        )
        .unwrap();
    }
    for d in &plan.diagnostics {
        writeln!(out, "  diag: {:?} {}", d.span, d.message).unwrap();
    }
    out
}

#[test]
fn parser_table_tests() {
    assert!(CASES.len() >= 40);
    let mut out = String::new();
    for case in CASES {
        writeln!(out, "{case:?}").unwrap();
        out.push_str(&render(&parse(case)));
    }
    insta::assert_snapshot!("parser_table", out);
}

#[test]
fn parser_is_deterministic_and_trim_insensitive() {
    assert_eq!(parse("  beach   rating:3 "), parse("beach rating:3"));
    assert!(parse("").is_empty());
    assert!(!parse("beach").is_empty());
}

proptest! {
    #![proptest_config(ProptestConfig { cases: 512, ..ProptestConfig::default() })]

    #[test]
    fn parser_never_panics(src in "\\PC{0,60}") {
        let plan = parse(&src);
        prop_assert_eq!(plan.sql_where.matches('?').count(), plan.params.len());
    }

    #[test]
    fn parser_never_panics_on_query_like_text(src in "[-a-z:\"><=.0-9' %_*(); ]{0,60}") {
        let plan = parse(&src);
        prop_assert_eq!(plan.sql_where.matches('?').count(), plan.params.len());
    }

    #[test]
    fn parser_params_bound(
        marker in "zzq[0-9]{3}",
        key in prop::sample::select(vec![
            "tag", "album", "folder", "camera", "lens", "person", "caption", "name", "ext",
            "type", "rating", "date", "year", "month", "starred", "has", "is", "bogus", "",
        ]),
        negate in any::<bool>(),
        quote in any::<bool>(),
    ) {
        let value = if quote { format!("\"{marker}'; --\"") } else { marker.clone() };
        let prefix = if key.is_empty() { String::new() } else { format!("{key}:") };
        let src = format!("{}{prefix}{value}", if negate { "-" } else { "" });
        let plan = parse(&src);
        prop_assert!(!plan.sql_where.contains("zzq"), "user text leaked into SQL: {}", plan.sql_where);
        prop_assert!(!plan.sql_where.contains("DROP"));
        prop_assert_eq!(plan.sql_where.matches('?').count(), plan.params.len());
    }
}
