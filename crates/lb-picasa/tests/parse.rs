//! Parsing the generated `basic/picasa` fixture (SPEC §6.17).
#![allow(clippy::unwrap_used, clippy::expect_used)]

use lb_picasa::parse_picasa_ini;
use lb_testkit::fixtures;

fn fixture_text() -> String {
    let dir = fixtures::default_generated_dir();
    assert!(
        dir.join(fixtures::MARKER).is_file(),
        "run `cargo xtask fixtures` first"
    );
    std::fs::read_to_string(dir.join("basic/picasa/.picasa.ini")).unwrap()
}

#[test]
fn picasa_ini_parse_fixture() {
    let ini = parse_picasa_ini(&fixture_text());
    insta::assert_debug_snapshot!("picasa_ini_parse_fixture", ini);
}

#[test]
fn fixture_covers_every_mapping() {
    let ini = parse_picasa_ini(&fixture_text());
    assert_eq!(
        ini.folder.description.as_deref(),
        Some("Folder caption from Picasa")
    );
    assert_eq!(ini.albums.len(), 2);
    assert_eq!(
        ini.contacts.get("4f3e2d1c").map(String::as_str),
        Some("Ana")
    );
    assert_eq!(ini.files.len(), 4);
    assert_eq!(ini.files[1].skipped_filters, ["madeupfilter"]);
    assert_eq!(ini.files[2].caption.as_deref(), Some("Line one\nLine two"));
}
