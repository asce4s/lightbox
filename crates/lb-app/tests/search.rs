//! M6.5: search text, filter chips and grouping through `SearchService`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use chrono::NaiveDate;
use lb_app::{FilterChips, GroupKey, Lightbox, ViewQuery, ViewSource};
use lb_core::{GroupBy, PhotoId, RootMode};
use lb_jobs::JobQueue;

fn name_of(p: &lb_core::Photo) -> String {
    p.path.file_name().unwrap().to_string_lossy().into_owned()
}

struct Lib {
    _invariants: lb_testkit::Invariants,
    _home: tempfile::TempDir,
    _photos: tempfile::TempDir,
    lb: Lightbox,
}

impl Lib {
    /// Photos at `dir/name` (dates come from the names), scanned.
    fn new(files: &[&str]) -> Self {
        let home = tempfile::tempdir().unwrap();
        let photos = tempfile::tempdir().unwrap();
        for (i, f) in files.iter().enumerate() {
            let path = photos.path().join(f);
            std::fs::create_dir_all(path.parent().unwrap()).unwrap();
            common::write_jpeg(&path, u8::try_from(i + 1).unwrap());
        }
        let lb = Lightbox::open_in(home.path(), JobQueue::inline()).unwrap();
        lb.library()
            .add_root(photos.path(), RootMode::Watch)
            .unwrap();
        Self {
            _invariants: common::invariants(home.path(), &[photos.path()]),
            _home: home,
            _photos: photos,
            lb,
        }
    }

    fn id(&self, name: &str) -> PhotoId {
        self.lb
            .photos()
            .all()
            .unwrap()
            .into_iter()
            .find(|p| name_of(p) == name)
            .unwrap_or_else(|| panic!("no photo {name}"))
            .id
    }

    fn names(&self, search: &str) -> Vec<String> {
        let mut view = ViewQuery::new(ViewSource::AllPhotos);
        view.search = search.to_owned();
        self.run(&view)
    }

    fn run(&self, view: &ViewQuery) -> Vec<String> {
        let result = self.lb.search().query(view).unwrap();
        assert_eq!(result.total, result.ids().len());
        self.lb
            .photos()
            .get_many(&result.ids())
            .unwrap()
            .into_iter()
            .map(|p| name_of(&p))
            .collect()
    }
}

fn sorted(mut v: Vec<String>) -> Vec<String> {
    v.sort();
    v
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn search_fts_prefix() {
    let lib = Lib::new(&[
        "beach_IMG_20210615_120000.jpg",
        "sunset_IMG_20210616_120000.jpg",
        "Holidays/IMG_20210617_120000.jpg",
    ]);
    let sunset = lib.id("sunset_IMG_20210616_120000.jpg");
    lib.lb
        .photos()
        .set_caption(&[sunset], Some("A walk on the Beachfront"))
        .unwrap();

    assert_eq!(
        sorted(lib.names("beac")),
        strs(&[
            "beach_IMG_20210615_120000.jpg",
            "sunset_IMG_20210616_120000.jpg"
        ]),
        "prefix matches file names and captions"
    );
    assert_eq!(lib.names("beach").len(), 2);
    assert!(
        lib.names("\"beac\"").is_empty(),
        "a quoted phrase is not a prefix"
    );
    assert_eq!(
        lib.names("holid"),
        strs(&["IMG_20210617_120000.jpg"]),
        "folder names are searchable"
    );
    assert_eq!(
        lib.names("beach sunset"),
        vec!["sunset_IMG_20210616_120000.jpg"]
    );
    assert_eq!(
        lib.names("-beach"),
        strs(&["IMG_20210617_120000.jpg"]),
        "negated words exclude matches"
    );
    assert!(lib.names("nothingmatches").is_empty());
    assert_eq!(lib.names("").len(), 3, "empty text shows everything");
}

#[test]
fn search_tag_negation() {
    let lib = Lib::new(&[
        "a_IMG_20210101_100000.jpg",
        "b_IMG_20210102_100000.jpg",
        "c_IMG_20210103_100000.jpg",
        "d_IMG_20210104_100000.jpg",
    ]);
    let a = lib.id("a_IMG_20210101_100000.jpg");
    let b = lib.id("b_IMG_20210102_100000.jpg");
    lib.lb.tags().add(&[a, b], &["Work".to_owned()]).unwrap();
    lib.lb.tags().add(&[b], &["trip".to_owned()]).unwrap();

    assert_eq!(
        sorted(lib.names("tag:work")),
        strs(&["a_IMG_20210101_100000.jpg", "b_IMG_20210102_100000.jpg"]),
        "tag names are case-insensitive"
    );
    assert_eq!(
        sorted(lib.names("-tag:work")),
        strs(&["c_IMG_20210103_100000.jpg", "d_IMG_20210104_100000.jpg"])
    );
    assert_eq!(
        lib.names("tag:work -tag:trip"),
        vec!["a_IMG_20210101_100000.jpg"]
    );
    assert_eq!(
        lib.names("tag:wor").len(),
        0,
        "tag filters match whole names"
    );
    assert_eq!(
        lib.names("wor").len(),
        2,
        "bare words reach tags through the index"
    );
}

#[test]
fn search_date_range_inclusive() {
    let lib = Lib::new(&[
        "IMG_20230531_235959.jpg",
        "IMG_20230601_000000.jpg",
        "IMG_20230831_235959.jpg",
        "IMG_20230901_000000.jpg",
    ]);
    assert_eq!(
        sorted(lib.names("date:2023-06..2023-08")),
        strs(&["IMG_20230601_000000.jpg", "IMG_20230831_235959.jpg"]),
        "both ends are inclusive, including the whole last month"
    );
    assert_eq!(
        lib.names("date:2023-06-01..2023-06-01"),
        vec!["IMG_20230601_000000.jpg"]
    );
    assert_eq!(
        lib.names("date:2023-08-31"),
        vec!["IMG_20230831_235959.jpg"]
    );
    assert_eq!(lib.names("date:2023-09..").len(), 1);
    assert_eq!(lib.names("date:..2023-05").len(), 1);
    assert_eq!(lib.names("year:2023").len(), 4);
    assert_eq!(lib.names("year:2022").len(), 0);
    assert_eq!(lib.names("month:6").len(), 1);
    assert_eq!(
        lib.names("-date:2023-06..2023-08").len(),
        2,
        "negated ranges are the complement"
    );
}

#[test]
fn search_rating_comparison() {
    let names: Vec<String> = (0..6)
        .map(|r| format!("r{r}_IMG_2021010{}_100000.jpg", r + 1))
        .collect();
    let refs: Vec<&str> = names.iter().map(String::as_str).collect();
    let lib = Lib::new(&refs);
    for (r, name) in names.iter().enumerate() {
        lib.lb
            .photos()
            .set_rating(&[lib.id(name)], u8::try_from(r).unwrap())
            .unwrap();
    }
    let count = |q: &str| lib.names(q).len();
    assert_eq!(count("rating:>=3"), 3);
    assert_eq!(count("rating:>3"), 2);
    assert_eq!(count("rating:<=1"), 2);
    assert_eq!(count("rating:<1"), 1);
    assert_eq!(count("rating:=5"), 1);
    assert_eq!(count("rating:4"), 1);
    assert_eq!(count("rating:2..3"), 2);
    assert_eq!(count("-rating:>=3"), 3);
    assert_eq!(count("rating:>=3 rating:<=4"), 2, "terms combine with AND");
}

#[test]
fn search_combines_with_chips() {
    let lib = Lib::new(&[
        "beach_a_IMG_20210101_100000.jpg",
        "beach_b_IMG_20210102_100000.jpg",
        "beach_c_IMG_20210103_100000.jpg",
        "city_IMG_20210104_100000.jpg",
    ]);
    let a = lib.id("beach_a_IMG_20210101_100000.jpg");
    let b = lib.id("beach_b_IMG_20210102_100000.jpg");
    let city = lib.id("city_IMG_20210104_100000.jpg");
    let photos = lib.lb.photos();
    photos.set_starred(&[a, b, city], true).unwrap();
    photos.set_rating(&[b, city], 4).unwrap();

    let mut view = ViewQuery::new(ViewSource::AllPhotos);
    view.chips.starred = true;
    assert_eq!(lib.run(&view).len(), 3);

    view.search = "beach".into();
    assert_eq!(sorted(lib.run(&view)).len(), 2);

    view.chips.min_rating = Some(4);
    assert_eq!(lib.run(&view), vec!["beach_b_IMG_20210102_100000.jpg"]);

    view.chips.date = Some((NaiveDate::from_ymd_opt(2021, 1, 3), None));
    assert!(lib.run(&view).is_empty(), "date chip narrows further");

    assert_eq!(
        view.chips.to_query_text(),
        "is:starred rating:>=4 date:2021-01-03.."
    );
    assert_eq!(
        view.effective_text(),
        "beach is:starred rating:>=4 date:2021-01-03.."
    );
    assert!(FilterChips::default().is_empty());
    assert!(!view.chips.is_empty());

    let mut over_source = ViewQuery::new(ViewSource::Starred);
    over_source.search = "city".into();
    assert_eq!(lib.run(&over_source), vec!["city_IMG_20210104_100000.jpg"]);

    let mut hidden = ViewQuery::new(ViewSource::AllPhotos);
    photos.set_hidden(&[a], true).unwrap();
    assert_eq!(lib.run(&hidden).len(), 3);
    hidden.search = "is:hidden".into();
    assert_eq!(
        lib.run(&hidden),
        vec!["beach_a_IMG_20210101_100000.jpg"],
        "is:hidden reveals hidden photos"
    );
}

#[test]
fn view_query_groups_by_folder_and_month() {
    let lib = Lib::new(&[
        "Trips/IMG_20210115_100000.jpg",
        "Trips/IMG_20210120_100000.jpg",
        "Trips/IMG_20210305_100000.jpg",
        "Home/IMG_20210118_100000.jpg",
        "Home/IMG_20220101_100000.jpg",
    ]);
    let search = lib.lb.search();

    let mut view = ViewQuery::new(ViewSource::AllPhotos);
    view.group_by = GroupBy::Month;
    let by_month = search.query(&view).unwrap();
    assert_eq!(by_month.total, 5);
    let titles: Vec<_> = by_month.groups.iter().map(|g| g.title.as_str()).collect();
    assert_eq!(
        titles,
        ["January 2022", "March 2021", "January 2021"],
        "newest month first"
    );
    assert_eq!(by_month.groups[2].ids.len(), 3);
    assert_eq!(by_month.groups[2].subtitle, "3 photos");
    assert_eq!(by_month.groups[0].subtitle, "1 photo");
    assert_eq!(
        by_month.groups[1].key,
        GroupKey::Month {
            year: 2021,
            month: 3
        }
    );

    view.group_by = GroupBy::Folder;
    let by_folder = search.query(&view).unwrap();
    let mut folders: Vec<_> = by_folder
        .groups
        .iter()
        .map(|g| (g.title.clone(), g.ids.len()))
        .collect();
    folders.sort();
    assert_eq!(
        folders,
        [("Home".to_owned(), 2), ("Trips".to_owned(), 3)],
        "one group per folder"
    );
    assert!(
        by_folder
            .groups
            .iter()
            .all(|g| g.subtitle.contains(g.title.as_str()))
    );

    view.group_by = GroupBy::Year;
    let by_year = search.query(&view).unwrap();
    assert_eq!(
        by_year
            .groups
            .iter()
            .map(|g| g.title.as_str())
            .collect::<Vec<_>>(),
        ["2022", "2021"]
    );

    view.group_by = GroupBy::None;
    let flat = search.query(&view).unwrap();
    assert_eq!(flat.groups.len(), 1);
    assert_eq!(flat.groups[0].ids.len(), 5);

    view.group_by = GroupBy::Month;
    view.sort = lb_core::SortOrder::DateTakenAsc;
    let asc = search.query(&view).unwrap();
    assert_eq!(asc.groups[0].title, "January 2021");
    assert_eq!(asc.ids().len(), 5);

    view.search = "nomatch".into();
    let none = search.query(&view).unwrap();
    assert_eq!((none.total, none.groups.len()), (0, 0));
}

#[test]
fn search_service_reports_diagnostics() {
    let lib = Lib::new(&["IMG_20210101_100000.jpg"]);
    let plan = lib.lb.search().parse("rating:abc beach");
    assert_eq!(plan.diagnostics.len(), 1);
    assert_eq!(plan.diagnostics[0].span, 0..10);
    assert!(
        lib.names("rating:abc").len() == 1,
        "invalid filters are ignored, not fatal"
    );
}
