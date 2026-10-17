//! M9.7: batch rename patterns (SPEC §6.7.4).
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use chrono::NaiveDate;
use lb_app::expand_rename_pattern;

#[test]
fn batch_rename_pattern_table() {
    let date = NaiveDate::from_ymd_opt(2021, 6, 5)
        .unwrap()
        .and_hms_opt(14, 3, 9)
        .unwrap();
    let ok: &[(&str, &str, u64, &str)] = &[
        ("{name}", "IMG_1", 1, "IMG_1"),
        ("{n}", "a", 7, "7"),
        ("{n:03}", "a", 7, "007"),
        ("{n:03}", "a", 1234, "1234"),
        ("{n:1}", "a", 5, "5"),
        ("{date}", "a", 1, "2021-06-05"),
        ("{date:%Y-%m-%d}_{n:03}", "a", 2, "2021-06-05_002"),
        ("{date:%H.%M.%S}", "a", 1, "14.03.09"),
        ("trip_{name}_{n:02}", "beach", 3, "trip_beach_03"),
        ("{{{name}}}", "x", 1, "{x}"),
        ("plain", "x", 1, "plain"),
        ("", "x", 1, ""),
    ];
    for (pattern, stem, n, want) in ok {
        assert_eq!(
            expand_rename_pattern(pattern, stem, *n, date).unwrap(),
            *want,
            "pattern {pattern:?}"
        );
    }
    for bad in [
        "{nope}",
        "{name",
        "name}",
        "{n:x}",
        "{n:99}",
        "{date:}",
        "{name:3}",
        "{date:%Q}",
    ] {
        assert!(
            expand_rename_pattern(bad, "a", 1, date).is_err(),
            "pattern {bad:?} must be rejected"
        );
    }
}

#[test]
fn batch_rename_renames_files_and_keeps_user_data() {
    let t = common::library(3);
    let (lb, ids) = (&t.lb, t.ids.clone());
    lb.photos().set_starred(&ids[..1], true).unwrap();
    let preview = lb
        .photos()
        .preview_batch_rename(&ids, "trip_{n:03}", 1)
        .unwrap();
    let names: Vec<_> = preview.iter().map(|p| p.to.clone().unwrap()).collect();
    assert_eq!(names, ["trip_001.jpg", "trip_002.jpg", "trip_003.jpg"]);

    let report = lb.photos().batch_rename(&ids, "trip_{n:03}", 1).unwrap();
    assert_eq!(report.renamed, ids);
    for (id, name) in ids.iter().zip(&names) {
        let p = lb.photos().get(*id).unwrap().unwrap();
        assert_eq!(p.path.file_name().unwrap().to_str().unwrap(), name);
        assert!(p.path.exists());
    }
    assert!(lb.photos().get(ids[0]).unwrap().unwrap().starred);
}

#[test]
fn batch_rename_chains_within_batch_and_reports_clashes() {
    let t = common::library(3);
    let (lb, ids) = (&t.lb, t.ids.clone());
    let before: Vec<_> = ids
        .iter()
        .map(|i| std::fs::read(lb.photos().get(*i).unwrap().unwrap().path).unwrap())
        .collect();
    // p00 -> p01 -> p02 -> p03: each target is the previous name of the next photo.
    let report = lb.photos().batch_rename(&ids, "p{n:02}", 1).unwrap();
    assert_eq!(report.renamed.len(), 3, "{report:?}");
    for (i, id) in ids.iter().enumerate() {
        let p = lb.photos().get(*id).unwrap().unwrap();
        assert_eq!(
            p.path.file_name().unwrap().to_str().unwrap(),
            format!("p{:02}.jpg", i + 1)
        );
        assert_eq!(
            std::fs::read(&p.path).unwrap(),
            before[i],
            "content follows the photo"
        );
    }

    let same = lb.photos().batch_rename(&ids, "same", 1).unwrap();
    assert_eq!(same.renamed.len(), 1);
    assert_eq!(same.failed.len(), 2, "{same:?}");
}

#[test]
fn batch_rename_rejects_empty_and_bad_patterns() {
    let t = common::library(2);
    assert!(t.lb.photos().batch_rename(&[], "{n}", 1).is_err());
    assert!(t.lb.photos().batch_rename(&t.ids, "{bogus}", 1).is_err());
    assert!(
        t.lb.photos()
            .batch_rename(&t.ids, "a/{n}", 1)
            .unwrap()
            .renamed
            .is_empty()
    );
}
