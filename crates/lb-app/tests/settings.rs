//! M9.1: typed settings.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod common;

use lb_app::{AppEvent, JobQueue, Lightbox, Settings, SlideshowSettings, ThemeChoice, Transition};
use lb_core::GroupBy;

#[test]
fn settings_defaults() {
    let t = common::library(0);
    let s = t.lb.settings().load().unwrap();
    assert_eq!(s, Settings::default());
    assert_eq!(s.theme, ThemeChoice::System);
    assert_eq!(s.thumbs.cache_limit_mb, 2048);
    assert_eq!(s.thumbs.memory_limit_mb, 400);
    assert_eq!(s.export.quality, 85);
    assert!(s.export.apply_edits);
    assert_eq!(s.slideshow.duration_secs, 3);
    assert_eq!(s.slideshow.transition, Transition::Crossfade);
    assert!(!s.faces.enabled);
    assert!(!s.library.show_hidden && !s.library.show_missing);
    assert_eq!(
        s.map.tile_url,
        "https://tile.openstreetmap.org/{z}/{x}/{y}.png"
    );
    assert_eq!(s.video.ffmpeg_path, None);
    assert!(
        t.lb.catalog_handle().list_settings().unwrap().is_empty(),
        "defaults are not stored"
    );
}

#[test]
fn settings_roundtrip() {
    let t = common::library(0);
    let saved =
        t.lb.settings()
            .update(|s| {
                s.theme = ThemeChoice::Dark;
                s.grid.group_by = GroupBy::Folder;
                s.thumbs.cache_limit_mb = 512;
                s.slideshow = SlideshowSettings {
                    duration_secs: 9,
                    shuffle: true,
                    transition: Transition::KenBurns,
                    ..SlideshowSettings::default()
                };
                s.video.ffmpeg_path = Some("/opt/ffmpeg".into());
            })
            .unwrap();
    drop(t.lb);
    let reopened = Lightbox::open(t.home.path(), JobQueue::inline()).unwrap();
    assert_eq!(reopened.settings().load().unwrap(), saved);
    assert_eq!(saved.theme, ThemeChoice::Dark);
}

#[test]
fn settings_unknown_keys_ignored() {
    let t = common::library(0);
    let catalog = t.lb.catalog_handle();
    catalog.set_setting("from_the_future", "{\"x\":1}").unwrap();
    catalog.set_setting("theme", "\"dark\"").unwrap();
    catalog
        .set_setting("slideshow", "{\"duration_secs\":7,\"new_option\":true}")
        .unwrap();
    catalog.set_setting("thumbs", "not json at all").unwrap();
    catalog
        .set_setting("grid", "{\"group_by\":\"nonsense\"}")
        .unwrap();
    let s = t.lb.settings().load().unwrap();
    assert_eq!(s.theme, ThemeChoice::Dark);
    assert_eq!(s.slideshow.duration_secs, 7);
    assert!(
        s.slideshow.loop_forever,
        "missing fields keep their default"
    );
    assert_eq!(
        s.thumbs,
        Settings::default().thumbs,
        "garbage falls back to defaults"
    );
    assert_eq!(s.grid, Settings::default().grid);
    t.lb.settings().set_theme(ThemeChoice::Light).unwrap();
    assert!(
        catalog.get_setting("from_the_future").unwrap().is_some(),
        "unknown keys are kept"
    );
}

#[test]
fn theme_change_emits_event() {
    let t = common::library(0);
    let events = t.lb.events();
    t.lb.settings().set_theme(ThemeChoice::Dark).unwrap();
    t.lb.bus().tick();
    let got: Vec<_> = events.try_iter().collect();
    assert!(
        got.iter()
            .any(|e| matches!(e, AppEvent::SettingsChanged { key } if key == "theme")),
        "{got:?}"
    );
    t.lb.settings().set_theme(ThemeChoice::Dark).unwrap();
    t.lb.bus().tick();
    assert!(
        events.try_iter().next().is_none(),
        "no event when nothing changed"
    );
}
