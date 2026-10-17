//! M9.2: timeline scrubber view-model.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use chrono::NaiveDate;
use lb_app::vm::LibraryVm;

const NAMES: [&str; 6] = [
    "IMG_20210615_120000.jpg",
    "IMG_20210620_120000.jpg",
    "IMG_20210701_120000.jpg",
    "IMG_20211231_120000.jpg",
    "IMG_20220105_120000.jpg",
    "IMG_20220106_120000.jpg",
];

fn vm() -> (common::TestLib, LibraryVm) {
    let t = common::named(&NAMES);
    let mut vm = LibraryVm::new(&t.lb);
    vm.set_zoom(96);
    vm.set_viewport_width(320.0);
    vm.refresh().unwrap();
    (t, vm)
}

#[test]
fn vm_timeline_buckets_by_month() {
    let (t, vm) = vm();
    let months: Vec<_> = vm
        .timeline()
        .iter()
        .map(|b| (b.year, b.month, b.count))
        .collect();
    assert_eq!(
        months,
        vec![(2022, 1, 2), (2021, 12, 1), (2021, 7, 1), (2021, 6, 2)],
        "newest first, like the grid"
    );
    assert_eq!(vm.timeline()[0].first, t.ids[5]);
    assert_eq!(vm.timeline()[3].first, t.ids[1]);
}

#[test]
fn vm_timeline_jump_anchors_first_photo_of_month() {
    let (t, mut vm) = vm();
    let row = vm.timeline_jump(2).unwrap();
    assert_eq!(vm.scroll_anchor(), Some(t.ids[2]));
    assert_eq!(vm.layout().row_top(row), vm.anchor_scroll_top());
    assert!(vm.timeline_jump(9).is_none());
}

#[test]
fn vm_timeline_filter_sets_date_chip() {
    let (t, mut vm) = vm();
    vm.timeline_filter(3, 2);
    assert_eq!(
        vm.view().chips.date,
        Some((
            NaiveDate::from_ymd_opt(2021, 6, 1),
            NaiveDate::from_ymd_opt(2021, 7, 31)
        ))
    );
    assert_eq!(vm.order().len(), 3);
    assert!(vm.order().contains(&t.ids[2]));
    assert_eq!(vm.timeline().len(), 2);
}
