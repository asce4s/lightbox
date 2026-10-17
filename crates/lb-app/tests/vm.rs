//! M6.7: view-models without any UI toolkit.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use lb_app::vm::{
    EditorTab, EditorVm, ExportDialogVm, ExportField, ExportPhase, LibraryVm, Modifiers, NavKey,
    SizeChoice, TuningSlider, ViewerVm, ZoomMode, columns_for,
};
use lb_app::{AppEvent, FilterChips, LightboxOptions, ViewSource};
use lb_core::{EditOp, GroupBy, PhotoId, SortOrder};
use lb_testkit::FakeClock;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

/// Folders `a` (5 photos), `b` (4), `c` (3), grouped by folder, three columns.
fn grid() -> (common::TestLib, LibraryVm) {
    let t = common::build(&[("a", 5), ("b", 4), ("c", 3)], LightboxOptions::default());
    let mut vm = LibraryVm::new(&t.lb);
    vm.set_zoom(96);
    vm.set_viewport_width(320.0);
    vm.set_group_by(GroupBy::Folder);
    vm.set_sort(SortOrder::FileNameAsc);
    (t, vm)
}

fn group(vm: &LibraryVm, g: usize) -> Vec<PhotoId> {
    vm.result().groups[g].ids.clone()
}

#[test]
fn vm_selection_click_shift_ctrl() {
    let (_t, mut vm) = grid();
    assert_eq!(vm.columns(), 3);
    assert_eq!(vm.result().groups.len(), 3);
    let (a, b) = (group(&vm, 0), group(&vm, 1));
    let sel = |vm: &LibraryVm| vm.selected_ids();

    vm.click(a[1], Modifiers::NONE);
    assert_eq!(sel(&vm), [a[1]]);
    assert_eq!(vm.focused(), Some(a[1]));

    vm.click(a[3], Modifiers::SHIFT);
    assert_eq!(sel(&vm), [a[1], a[2], a[3]], "range from the anchor");

    vm.click(a[0], Modifiers::SHIFT);
    assert_eq!(
        sel(&vm),
        [a[0], a[1]],
        "a new shift click re-ranges from the same anchor"
    );

    vm.click(a[3], Modifiers::CTRL);
    assert_eq!(sel(&vm), [a[0], a[1], a[3]], "ctrl adds");
    vm.click(a[1], Modifiers::CTRL);
    assert_eq!(sel(&vm), [a[0], a[3]], "ctrl toggles off");

    vm.click(
        b[1],
        Modifiers {
            shift: true,
            ctrl: true,
        },
    );
    let mut expected = a.clone();
    expected.extend(&b[..=1]);
    assert_eq!(
        sel(&vm),
        expected,
        "ctrl+shift adds the range from the last ctrl-clicked anchor (a1) across groups"
    );

    vm.click(a[3], Modifiers::NONE);
    vm.click(b[2], Modifiers::SHIFT);
    assert_eq!(
        sel(&vm),
        [a[3], a[4], b[0], b[1], b[2]],
        "range crosses a group header"
    );

    vm.select_all();
    assert_eq!(sel(&vm).len(), 12);
    vm.clear_selection();
    assert!(sel(&vm).is_empty());
    assert_eq!(vm.selection().anchor, None);

    vm.click(PhotoId::new(99_999), Modifiers::NONE);
    assert!(sel(&vm).is_empty(), "unknown ids are ignored");
}

#[test]
fn vm_keyboard_navigation_grid() {
    let (_t, mut vm) = grid();
    let (a, b, c) = (group(&vm, 0), group(&vm, 1), group(&vm, 2));

    vm.move_focus(NavKey::Right, false);
    assert_eq!(
        vm.focused(),
        Some(a[0]),
        "first key press focuses the first photo"
    );
    vm.move_focus(NavKey::Right, false);
    assert_eq!(vm.focused(), Some(a[1]));
    assert_eq!(
        vm.selected_ids(),
        [a[1]],
        "plain moves select the focused photo"
    );

    vm.move_focus(NavKey::Down, false);
    assert_eq!(vm.focused(), Some(a[4]), "same column in the next row");
    vm.move_focus(NavKey::Down, false);
    assert_eq!(vm.focused(), Some(b[1]), "next group, same column");
    vm.move_focus(NavKey::Down, false);
    assert_eq!(
        vm.focused(),
        Some(b[3]),
        "short last row clamps to its last cell"
    );
    vm.move_focus(NavKey::Down, false);
    assert_eq!(vm.focused(), Some(c[0]));
    vm.move_focus(NavKey::Down, false);
    assert_eq!(vm.focused(), Some(c[0]), "bottom edge stays");

    vm.move_focus(NavKey::Up, false);
    assert_eq!(vm.focused(), Some(b[3]));
    vm.move_focus(NavKey::Up, false);
    assert_eq!(vm.focused(), Some(b[0]));
    vm.move_focus(NavKey::Up, false);
    assert_eq!(vm.focused(), Some(a[3]), "previous group's last row");

    vm.move_focus(NavKey::Left, false);
    assert_eq!(vm.focused(), Some(a[2]));
    vm.move_focus(NavKey::Home, false);
    assert_eq!(vm.focused(), Some(a[0]));
    vm.move_focus(NavKey::Left, false);
    assert_eq!(vm.focused(), Some(a[0]), "start stays");
    vm.move_focus(NavKey::End, false);
    assert_eq!(vm.focused(), Some(c[2]));
    vm.move_focus(NavKey::Home, false);
    for _ in 0..4 {
        vm.move_focus(NavKey::Right, false);
    }
    assert_eq!(vm.focused(), Some(a[4]));
    vm.move_focus(NavKey::Right, false);
    assert_eq!(vm.focused(), Some(b[0]), "right wraps into the next group");
    vm.move_focus(NavKey::Left, false);
    assert_eq!(vm.focused(), Some(a[4]), "left wraps back");

    vm.click(a[0], Modifiers::NONE);
    vm.move_focus(NavKey::Down, true);
    assert_eq!(
        vm.selected_ids(),
        [a[0], a[1], a[2], a[3]],
        "shift extends from the anchor"
    );
    assert_eq!(vm.activate(), Some(a[3]));

    vm.toggle_hold_focused();
    assert_eq!(vm.tray(), [a[3]]);
    vm.toggle_hold_focused();
    assert!(vm.tray().is_empty());
}

#[test]
fn vm_scroll_anchor_preserved_on_resort() {
    let (_t, mut vm) = grid();
    let anchor = group(&vm, 1)[3];
    vm.set_scroll_anchor(Some(anchor));
    let row_top = |vm: &LibraryVm, id: PhotoId| {
        let (g, i) = vm.position_of(id).unwrap();
        vm.layout().row_top(vm.layout().row_of(g, i).unwrap())
    };
    assert_eq!(vm.anchor_scroll_top(), row_top(&vm, anchor));
    let before = vm.anchor_scroll_top();
    assert!(before > 0.0);

    vm.set_group_by(GroupBy::Month);
    assert_eq!(
        vm.scroll_anchor(),
        Some(anchor),
        "regrouping keeps the anchor photo"
    );
    assert_eq!(vm.anchor_scroll_top(), row_top(&vm, anchor));

    vm.set_sort(SortOrder::FileNameAsc);
    assert_eq!(vm.scroll_anchor(), Some(anchor));
    assert_eq!(vm.anchor_scroll_top(), row_top(&vm, anchor));

    vm.set_viewport_width(1000.0);
    assert!(vm.columns() > 3);
    assert_eq!(vm.scroll_anchor(), Some(anchor), "resizing keeps it too");
    assert_eq!(vm.anchor_scroll_top(), row_top(&vm, anchor));

    let at = vm.anchor_scroll_top();
    assert_eq!(
        vm.photo_at_scroll(at)
            .map(|id| vm.position_of(id).unwrap().0),
        Some(vm.position_of(anchor).unwrap().0),
        "the first photo at the anchor offset is in the anchor's group"
    );

    let old_order = vm.order().to_vec();
    let idx = old_order.iter().position(|i| *i == anchor).unwrap();
    let anchor_name = vm.photos(&[anchor]).unwrap()[0].path.clone();
    let other: String = if anchor_name.to_string_lossy().ends_with("p03.jpg") {
        "p00".into()
    } else {
        "p03".into()
    };
    vm.set_search(&other);
    assert!(!vm.order().contains(&anchor));
    let expected = old_order[idx..]
        .iter()
        .chain(old_order[..idx].iter().rev())
        .find(|id| vm.order().contains(id))
        .copied();
    assert_eq!(
        vm.scroll_anchor(),
        expected,
        "a vanished anchor moves to its nearest survivor"
    );
    assert!(vm.scroll_anchor().is_some());
}

#[test]
fn vm_editor_slider_preview_then_commit() {
    let t = common::library(1);
    let id = t.ids[0];
    let mut vm = EditorVm::new(&t.lb, id);
    assert_eq!(vm.tab(), EditorTab::Basic);
    vm.set_tab(EditorTab::Tuning);

    let version = || t.lb.photos().get(id).unwrap().unwrap().edit_version;
    let v0 = version();
    for v in [0.1, 0.3, 0.5] {
        vm.set_tuning(TuningSlider::FillLight, v).unwrap();
    }
    assert!(
        vm.committed().unwrap().ops.is_empty(),
        "dragging stores nothing"
    );
    assert_eq!(version(), v0);
    assert_eq!(vm.preview_stack().unwrap().ops.len(), 1);
    assert_eq!(vm.tuning_value(TuningSlider::FillLight).unwrap(), 0.5);
    assert!(!vm.can_undo().unwrap());

    let cancel = AtomicBool::new(false);
    let with = vm.render(48, &cancel).unwrap();
    vm.set_show_before(true);
    assert!(vm.preview_stack().unwrap().ops.is_empty());
    let before = vm.render(48, &cancel).unwrap();
    assert_ne!(
        with.as_raw(),
        before.as_raw(),
        "the preview shows the draft"
    );
    vm.set_show_before(false);

    vm.set_tuning(TuningSlider::Temperature, 0.25).unwrap();
    assert!(vm.commit().unwrap());
    let stack = vm.committed().unwrap();
    assert_eq!(stack.ops.len(), 1, "one op per drag");
    assert!(matches!(
        stack.ops[0],
        EditOp::Tuning { fill_light, temperature, .. } if fill_light == 0.5 && temperature == 0.25
    ));
    assert_eq!(version(), v0 + 1);
    assert!(vm.draft().is_none());
    assert!(!vm.commit().unwrap(), "nothing left to commit");

    vm.set_tuning(TuningSlider::Shadows, 0.4).unwrap();
    assert_eq!(
        vm.tuning_value(TuningSlider::FillLight).unwrap(),
        0.5,
        "sliders keep each other's values"
    );
    vm.commit().unwrap();
    assert_eq!(
        vm.committed().unwrap().ops.len(),
        1,
        "same-kind drag replaces the op"
    );

    vm.set_tuning(TuningSlider::Shadows, 9.0).unwrap();
    assert_eq!(
        vm.tuning_value(TuningSlider::Shadows).unwrap(),
        1.0,
        "values clamp"
    );
    vm.cancel();
    assert_eq!(vm.tuning_value(TuningSlider::Shadows).unwrap(), 0.4);

    assert!(vm.undo().unwrap());
    assert!(vm.committed().unwrap().ops.is_empty());
    assert!(vm.can_redo().unwrap());
    assert!(vm.redo().unwrap());
    vm.lucky().unwrap();
    assert_eq!(vm.committed().unwrap().ops.len(), 2);
    vm.revert().unwrap();
    assert!(vm.committed().unwrap().ops.is_empty());
    assert!(
        vm.redo().unwrap(),
        "undoing a revert restores everything in one step"
    );
    assert_eq!(vm.committed().unwrap().ops.len(), 2);
}

#[test]
fn vm_library_intents_and_events() {
    let (t, mut vm) = grid();
    let a = group(&vm, 0);
    vm.click(a[0], Modifiers::NONE);
    vm.click(a[1], Modifiers::SHIFT);

    vm.toggle_star_selected();
    let photos = t.lb.photos().get_many(&a[..3]).unwrap();
    assert_eq!(
        photos.iter().map(|p| p.starred).collect::<Vec<_>>(),
        [true, true, false]
    );
    vm.rate_selected(4);
    assert!(
        t.lb.photos()
            .get_many(&a[..2])
            .unwrap()
            .iter()
            .all(|p| p.rating == 4)
    );

    let chips = FilterChips {
        starred: true,
        ..FilterChips::default()
    };
    vm.set_chips(chips);
    assert_eq!(vm.order().len(), 2, "chips filter the grid");
    assert_eq!(vm.selected_ids().len(), 2);
    vm.set_chips(FilterChips::default());
    vm.set_source(ViewSource::Starred);
    assert_eq!(vm.order().len(), 2);
    assert!(
        vm.selected_ids().is_empty(),
        "changing source clears the selection"
    );

    vm.set_source(ViewSource::AllPhotos);
    vm.click(a[0], Modifiers::NONE);
    vm.rotate_selected(1);
    assert_eq!(t.lb.edits().stack(a[0]).unwrap().ops.len(), 1);

    vm.hold_selected();
    vm.click(a[3], Modifiers::NONE);
    assert_eq!(vm.target_ids(), [a[0]], "the tray wins over the selection");
    vm.clear_tray();
    assert_eq!(vm.target_ids(), [a[3]]);

    t.lb.photos().set_hidden(&[a[4]], true).unwrap();
    assert!(vm.handle_event(&AppEvent::PhotosChanged { ids: vec![a[4]] }));
    assert!(!vm.order().contains(&a[4]));
    assert!(!vm.handle_event(&AppEvent::SettingsChanged { key: "x".into() }));
    vm.set_show_hidden(true);
    assert!(vm.order().contains(&a[4]));

    vm.set_zoom(5);
    assert_eq!(vm.zoom(), 96);
    vm.set_zoom(9999);
    assert_eq!(vm.zoom(), 320);
    assert_eq!(columns_for(320.0, 96), 3);
    assert_eq!(columns_for(10.0, 320), 1);
}

#[test]
fn vm_grid_layout_virtualizes_rows() {
    let (_t, vm) = grid();
    let layout = vm.layout();
    assert_eq!(
        layout.rows.len(),
        3 + 2 + 2 + 1,
        "3 headers and the cell rows"
    );
    assert_eq!(layout.row_top(0), 0.0);
    let small = layout.visible_rows(0.0, 100.0, 0.0);
    assert!(small.len() < layout.rows.len());
    assert_eq!(small.start, 0);
    let all = layout.visible_rows(0.0, layout.total_height(), 0.0);
    assert_eq!(all, 0..layout.rows.len());
    let mid = layout.visible_rows(layout.row_top(4) + 1.0, 10.0, 0.0);
    assert_eq!(mid.start, 4, "the row containing the top edge is included");
}

#[test]
fn vm_search_debounce_uses_the_clock() {
    let clock = Arc::new(FakeClock::new());
    let options = LightboxOptions {
        clock: clock.clone(),
        ..LightboxOptions::default()
    };
    let t = common::build(&[("", 6)], options);
    let mut vm = LibraryVm::new(&t.lb);
    vm.refresh().unwrap();
    assert_eq!(vm.order().len(), 6);

    vm.type_search("p0");
    assert!(!vm.tick());
    clock.advance(Duration::from_millis(100));
    vm.type_search("p03");
    clock.advance(Duration::from_millis(100));
    assert!(!vm.tick(), "each keystroke restarts the wait");
    assert_eq!(vm.order().len(), 6);
    clock.advance(Duration::from_millis(60));
    assert!(vm.tick());
    assert_eq!(vm.order().len(), 1);
    assert!(!vm.tick(), "applied once");
}

#[test]
fn vm_viewer_navigation_and_zoom() {
    let t = common::library(4);
    let mut v = ViewerVm::open(&t.lb, t.ids.clone(), t.ids[1]).unwrap();
    assert_eq!(v.current(), t.ids[1]);
    assert!(v.go_prev() && v.current() == t.ids[0]);
    assert!(!v.go_prev(), "start stays");
    assert!(v.end() && v.current() == t.ids[3]);
    assert!(!v.go_next() && !v.has_next());
    assert!(v.home() && v.has_next());
    assert!(v.goto(t.ids[2]));
    assert!(!v.goto(PhotoId::new(777)));

    assert_eq!(v.zoom(), ZoomMode::Fit);
    v.toggle_fit_actual();
    assert_eq!(v.zoom(), ZoomMode::Actual);
    v.pan_by(5.0, -3.0);
    assert_eq!(v.pan(), (5.0, -3.0));
    v.toggle_fit_actual();
    assert_eq!((v.zoom(), v.pan()), (ZoomMode::Fit, (0.0, 0.0)));
    v.zoom_in(0.5);
    assert!(matches!(v.zoom(), ZoomMode::Factor(f) if (f - 0.625).abs() < 1e-6));
    v.pan_by(1.0, 1.0);
    v.go_next();
    assert_eq!(
        (v.zoom(), v.pan()),
        (ZoomMode::Fit, (0.0, 0.0)),
        "navigating resets the view"
    );

    assert!(!v.info_visible());
    v.toggle_info();
    assert!(v.info_visible());
    assert!(v.photo().unwrap().is_some());

    assert!(v.handle_event(&AppEvent::PhotosRemoved {
        ids: vec![t.ids[3]]
    }));
    assert_eq!(v.filmstrip().len(), 3);
    assert!(!v.handle_event(&AppEvent::PhotosRemoved { ids: t.ids.clone() }));
    assert!(ViewerVm::open(&t.lb, t.ids.clone(), PhotoId::new(777)).is_none());
}

#[test]
fn vm_export_dialog_validation_and_run() {
    let t = common::library(2);
    let out = tempfile::tempdir().unwrap();
    let mut d = ExportDialogVm::new(&t.lb, t.ids.clone());
    let fields: Vec<_> = d.errors().iter().map(|e| e.field).collect();
    assert_eq!(fields, [ExportField::Destination]);
    assert!(!d.start().unwrap(), "invalid settings do not start");

    d.dest = out.path().to_string_lossy().into_owned();
    d.size = SizeChoice::Custom;
    d.custom_size = "abc".into();
    assert_eq!(d.errors()[0].field, ExportField::CustomSize);
    d.custom_size = "32".into();
    d.quality = 0;
    assert_eq!(d.errors()[0].field, ExportField::Quality);
    d.quality = 90;
    d.watermark = "  ".into();
    let o = d.options().unwrap();
    assert_eq!(o.size, lb_app::ExportSize::LongEdge(32));
    assert_eq!(o.quality, 90);
    assert_eq!(o.watermark, None, "blank watermark means none");

    assert!(d.start().unwrap());
    d.poll();
    let ExportPhase::Done(report) = d.phase() else {
        panic!("inline jobs finish immediately: {:?}", d.phase());
    };
    assert_eq!(report.written.len(), 2);
    for f in &report.written {
        assert_eq!(
            image::image_dimensions(&f.path)
                .unwrap()
                .0
                .max(image::image_dimensions(&f.path).unwrap().1),
            32
        );
    }

    let empty = ExportDialogVm::new(&t.lb, vec![]);
    assert!(
        empty
            .errors()
            .iter()
            .any(|e| e.field == ExportField::Selection)
    );
}
