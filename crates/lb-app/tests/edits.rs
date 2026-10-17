//! M6.2: edit service and edit-aware thumbnails.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use lb_app::{AppError, AppEvent, Lightbox};
use lb_core::{EditOp, NormRect, PhotoId};
use lb_jobs::{JobQueue, Priority};
use lb_testkit::wait_until;
use lb_thumbs::ThumbSize;
use std::sync::atomic::AtomicBool;
use std::time::Duration;

fn tuning(fill: f32) -> EditOp {
    EditOp::Tuning {
        fill_light: fill,
        highlights: 0.0,
        shadows: 0.0,
        temperature: 0.0,
        neutral: None,
    }
}

fn names(lb: &Lightbox, id: PhotoId) -> Vec<&'static str> {
    lb.edits()
        .stack(id)
        .unwrap()
        .ops
        .iter()
        .map(EditOp::name)
        .collect()
}

#[test]
fn slider_drag_coalesces_into_one_op() {
    let t = common::library(1);
    let (lb, id) = (&t.lb, t.ids[0]);
    for step in 1..=30 {
        lb.edits().adjust(id, tuning(step as f32 / 30.0)).unwrap();
    }
    let stack = lb.edits().stack(id).unwrap();
    assert_eq!(
        stack.ops,
        vec![tuning(1.0)],
        "one op holding the last value"
    );
    assert_eq!(stack.version, 30, "every commit bumps the version");

    let stack = lb.edits().undo(id).unwrap();
    assert!(stack.is_empty(), "a whole drag is a single undo step");

    lb.edits()
        .push(id, EditOp::Sharpen { amount: 1.0 })
        .unwrap();
    lb.edits().adjust(id, tuning(0.2)).unwrap();
    lb.edits().adjust(id, tuning(0.4)).unwrap();
    assert_eq!(names(lb, id), ["sharpen", "tuning"]);
    lb.edits()
        .adjust(id, EditOp::Sharpen { amount: 0.5 })
        .unwrap();
    assert_eq!(
        names(lb, id),
        ["sharpen", "tuning", "sharpen"],
        "only the top op is replaced"
    );
    lb.edits().adjust(id, EditOp::Sepia).unwrap();
    lb.edits().adjust(id, EditOp::Sepia).unwrap();
    assert_eq!(
        names(lb, id).iter().filter(|n| **n == "sepia").count(),
        2,
        "non-slider ops stack"
    );
}

#[test]
fn undo_redo_semantics() {
    let t = common::library(2);
    let (lb, id, other) = (&t.lb, t.ids[0], t.ids[1]);
    let edits = lb.edits();
    let rx = lb.events();

    assert!(!edits.can_undo(id).unwrap() && !edits.can_redo(id).unwrap());
    assert_eq!(
        edits.undo(id).unwrap().version,
        0,
        "undo on an empty stack is a no-op"
    );
    assert_eq!(edits.redo(id).unwrap().version, 0);

    edits.push(id, EditOp::Sepia).unwrap();
    edits.push(id, EditOp::Sharpen { amount: 1.0 }).unwrap();
    edits
        .push(
            id,
            EditOp::Vignette {
                amount: 0.5,
                radius: 0.5,
            },
        )
        .unwrap();
    assert_eq!(names(lb, id), ["sepia", "sharpen", "vignette"]);
    assert_eq!(lb.photos().get(id).unwrap().unwrap().edit_version, 3);

    edits.undo(id).unwrap();
    assert_eq!(names(lb, id), ["sepia", "sharpen"]);
    assert!(edits.can_redo(id).unwrap());
    let v = edits.redo(id).unwrap();
    assert_eq!(v.ops.len(), 3);
    assert_eq!(v.version, 5);

    edits.undo(id).unwrap();
    edits.undo(id).unwrap();
    assert_eq!(names(lb, id), ["sepia"]);
    edits.push(id, EditOp::BlackAndWhite).unwrap();
    assert!(
        !edits.can_redo(id).unwrap(),
        "a new op clears the redo history"
    );
    assert_eq!(names(lb, id), ["sepia", "black_and_white"]);

    assert!(
        edits.stack(other).unwrap().is_empty(),
        "other photos are untouched"
    );
    assert_eq!(lb.photos().get(other).unwrap().unwrap().edit_version, 0);

    lb.bus().flush();
    let versions: Vec<_> = rx
        .try_iter()
        .filter_map(|e| match e {
            AppEvent::PhotoEdited { id: i, version } if i == id => Some(version),
            _ => None,
        })
        .collect();
    assert_eq!(
        versions,
        (1..=8).collect::<Vec<_>>(),
        "one PhotoEdited per change, in order"
    );
}

#[test]
fn revert_is_undoable() {
    let t = common::library(1);
    let (lb, id) = (&t.lb, t.ids[0]);
    let edits = lb.edits();
    assert_eq!(
        edits.revert(id).unwrap().version,
        0,
        "reverting nothing changes nothing"
    );

    edits.push(id, EditOp::Sepia).unwrap();
    edits.push(id, EditOp::Sharpen { amount: 1.0 }).unwrap();
    edits.push(id, tuning(0.5)).unwrap();
    let before = edits.stack(id).unwrap();

    let reverted = edits.revert(id).unwrap();
    assert!(reverted.is_empty());
    assert_eq!(reverted.version, before.version + 1);
    assert!(!edits.can_undo(id).unwrap());
    assert!(edits.can_redo(id).unwrap());

    let restored = edits.redo(id).unwrap();
    assert_eq!(
        restored.ops, before.ops,
        "one redo restores every op in order"
    );
    assert_eq!(restored.version, before.version + 2);
    assert!(!edits.can_redo(id).unwrap());
}

#[test]
fn edits_persist_across_reopen() {
    let t = common::library(1);
    let id = t.ids[0];
    t.lb.edits().push(id, EditOp::Sepia).unwrap();
    t.lb.edits()
        .push(id, EditOp::Sharpen { amount: 1.0 })
        .unwrap();
    t.lb.edits().undo(id).unwrap();
    let home = t.home.path().to_path_buf();
    drop(t.lb);
    let lb = Lightbox::open(&home, JobQueue::inline()).unwrap();
    assert_eq!(names(&lb, id), ["sepia"]);
    assert!(lb.edits().can_redo(id).unwrap());
}

#[test]
fn invalid_ops_are_rejected_and_ranges_clamped() {
    let t = common::library(1);
    let (lb, id) = (&t.lb, t.ids[0]);
    let err = lb
        .edits()
        .push(id, EditOp::Sharpen { amount: f32::NAN })
        .unwrap_err();
    assert!(matches!(err, AppError::Edit(_)), "{err}");
    let err = lb
        .edits()
        .push(
            id,
            EditOp::Crop {
                x: 0.5,
                y: 0.5,
                w: 0.0,
                h: 0.2,
            },
        )
        .unwrap_err();
    assert!(matches!(err, AppError::Edit(_)), "{err}");
    let err = lb
        .edits()
        .push(
            id,
            EditOp::RedEye {
                regions: Vec::<NormRect>::new(),
            },
        )
        .unwrap_err();
    assert!(matches!(err, AppError::Edit(_)), "{err}");
    assert_eq!(
        lb.edits().stack(id).unwrap().version,
        0,
        "nothing was stored"
    );

    lb.edits()
        .push(id, EditOp::Sharpen { amount: 99.0 })
        .unwrap();
    assert_eq!(
        lb.edits().stack(id).unwrap().ops,
        vec![EditOp::Sharpen { amount: 2.0 }]
    );

    let missing = PhotoId::new(9999);
    assert!(matches!(
        lb.edits().push(missing, EditOp::Sepia),
        Err(AppError::NotFound("photo"))
    ));
}

#[test]
fn thumbnail_reflects_edits() {
    let t = common::library(1);
    let (lb, id) = (&t.lb, t.ids[0]);
    let thumbs = lb.thumbs();
    assert_eq!(thumbs.get_cached(id, ThumbSize::S).unwrap(), None);

    let plain_key = thumbs.cache_key(id, ThumbSize::S).unwrap();
    let plain_path = thumbs.generate(id, ThumbSize::S).unwrap();
    assert_eq!(
        thumbs.get_cached(id, ThumbSize::S).unwrap(),
        Some(plain_path.clone())
    );
    let plain = image::open(&plain_path).unwrap().to_rgb8();

    lb.edits().push(id, EditOp::BlackAndWhite).unwrap();
    let edited_key = thumbs.cache_key(id, ThumbSize::S).unwrap();
    assert_ne!(plain_key, edited_key, "cache key changes after an edit");
    assert_eq!(
        thumbs.get_cached(id, ThumbSize::S).unwrap(),
        None,
        "the old thumbnail is not served for the edited photo"
    );
    let edited_path = thumbs.generate(id, ThumbSize::S).unwrap();
    assert_ne!(plain_path, edited_path);
    let edited = image::open(&edited_path).unwrap().to_rgb8();
    assert_eq!(plain.dimensions(), edited.dimensions());
    assert_ne!(plain.as_raw(), edited.as_raw(), "the image itself differs");
    let grey = edited
        .pixels()
        .all(|p| p.0[0].abs_diff(p.0[1]) <= 3 && p.0[1].abs_diff(p.0[2]) <= 3);
    assert!(grey, "black and white edit is visible in the thumbnail");

    lb.edits().revert(id).unwrap();
    assert_eq!(thumbs.cache_key(id, ThumbSize::S).unwrap(), plain_key);
    assert_eq!(
        thumbs.get_cached(id, ThumbSize::S).unwrap(),
        Some(plain_path),
        "reverting finds the original thumbnail again"
    );
}

#[test]
fn thumb_request_emits_thumb_ready() {
    let t = common::library(1);
    let (lb, id) = (&t.lb, t.ids[0]);
    let rx = lb.events();
    let ticket = lb
        .thumbs()
        .request_thumb(id, ThumbSize::M, Priority::Interactive);
    assert_eq!(ticket.wait(Duration::from_secs(5)), Some(Ok(())));
    let mut got = None;
    assert!(wait_until(Duration::from_secs(2), || {
        got = rx
            .try_iter()
            .find(|e| matches!(e, AppEvent::ThumbReady { .. }));
        got.is_some()
    }));
    assert_eq!(
        got,
        Some(AppEvent::ThumbReady {
            id,
            size: ThumbSize::M,
            version: 0
        })
    );
    assert!(lb.thumbs().get_cached(id, ThumbSize::M).unwrap().is_some());

    let missing = lb
        .thumbs()
        .request_thumb(PhotoId::new(777), ThumbSize::M, Priority::Foreground);
    assert!(matches!(missing.wait(Duration::from_secs(5)), Some(Err(_))));
}

#[test]
fn warm_generates_every_missing_thumbnail_once() {
    let t = common::library(3);
    let tickets = t.lb.thumbs().warm(ThumbSize::S).unwrap();
    assert_eq!(tickets.len(), 3);
    assert!(
        tickets
            .iter()
            .all(|k| k.wait(Duration::from_secs(5)) == Some(Ok(())))
    );
    assert!(t.lb.thumbs().warm(ThumbSize::S).unwrap().is_empty());
}

#[test]
fn preview_reflects_stack_and_can_be_cancelled() {
    let t = common::library(1);
    let (lb, id) = (&t.lb, t.ids[0]);
    let plain = lb.edits().render_preview(id, 32).unwrap();
    assert_eq!(plain.width().max(plain.height()), 32);
    lb.edits().push(id, EditOp::BlackAndWhite).unwrap();
    let bw = lb.edits().render_preview(id, 32).unwrap();
    assert_ne!(plain.as_raw(), bw.as_raw());

    let draft = lb.edits().stack(id).unwrap();
    let cancel = AtomicBool::new(true);
    assert!(matches!(
        lb.edits().render_stack_preview(id, &draft, 32, &cancel),
        Err(AppError::Cancelled)
    ));
}

#[test]
fn original_file_unchanged_after_edits() {
    let t = common::library(1);
    let (lb, id) = (&t.lb, t.ids[0]);
    let path = lb.photos().get(id).unwrap().unwrap().path;
    let snapshot = || {
        let meta = std::fs::metadata(&path).unwrap();
        (
            std::fs::read(&path).unwrap(),
            meta.modified().unwrap(),
            meta.len(),
        )
    };
    let before = snapshot();
    let dir_listing = |p: &std::path::Path| {
        let mut v: Vec<_> = std::fs::read_dir(p)
            .unwrap()
            .map(|e| e.unwrap().file_name())
            .collect();
        v.sort();
        v
    };
    let listing = dir_listing(path.parent().unwrap());

    let edits = lb.edits();
    edits.push(id, EditOp::Sepia).unwrap();
    edits.adjust(id, tuning(0.5)).unwrap();
    edits.push(id, EditOp::RotateQuarter { turns: 1 }).unwrap();
    edits.undo(id).unwrap();
    edits.redo(id).unwrap();
    edits.revert(id).unwrap();
    edits.redo(id).unwrap();
    for size in ThumbSize::ALL {
        lb.thumbs().generate(id, size).unwrap();
    }
    edits.render_preview(id, 64).unwrap();

    assert!(
        snapshot() == before,
        "original bytes, mtime and size are unchanged"
    );
    assert_eq!(
        dir_listing(path.parent().unwrap()),
        listing,
        "no sidecar or temp files next to originals"
    );
}
