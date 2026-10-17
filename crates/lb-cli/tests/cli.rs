//! M6.8: end-to-end tests of every `lightbox-cli` subcommand against a throwaway library.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use assert_cmd::Command;
use serde_json::Value;
use std::path::{Path, PathBuf};

struct Env {
    home: tempfile::TempDir,
    photos: tempfile::TempDir,
}

fn write_jpeg(path: &Path, seed: u8) {
    let img = image::RgbImage::from_fn(80, 60, |x, y| {
        image::Rgb([(x * 3) as u8, (y * 4) as u8, seed.wrapping_mul(53)])
    });
    img.save(path).unwrap();
}

impl Env {
    /// A library with three photos `IMG_2021061{1,2,3}_120000.jpg`, already added as a root.
    fn new() -> Self {
        let env = Self::empty();
        for (i, day) in [11u8, 12, 13].iter().enumerate() {
            write_jpeg(
                &env.photos
                    .path()
                    .join(format!("IMG_202106{day}_120000.jpg")),
                u8::try_from(i).unwrap() + 1,
            );
        }
        env.json(&["roots", "add", env.photos.path().to_str().unwrap()]);
        env
    }

    fn empty() -> Self {
        Self {
            home: tempfile::tempdir().unwrap(),
            photos: tempfile::tempdir().unwrap(),
        }
    }

    fn cmd(&self, args: &[&str]) -> Command {
        let mut cmd = Command::cargo_bin("lightbox-cli").unwrap();
        cmd.env("LIGHTBOX_HOME", self.home.path()).args(args);
        cmd
    }

    fn json(&self, args: &[&str]) -> Value {
        let out = self.cmd(args).arg("--json").assert().success();
        serde_json::from_slice(&out.get_output().stdout).unwrap()
    }

    fn ids(&self, query: &str) -> Vec<i64> {
        let v = self.json(&["query", query]);
        let mut ids: Vec<i64> = v["photos"]
            .as_array()
            .unwrap()
            .iter()
            .map(|p| p["id"].as_i64().unwrap())
            .collect();
        ids.sort_unstable();
        ids
    }

    fn all(&self) -> Vec<i64> {
        self.ids("")
    }
}

fn s(i: i64) -> String {
    i.to_string()
}

#[test]
fn cli_roots_e2e() {
    let env = Env::empty();
    write_jpeg(&env.photos.path().join("a.jpg"), 1);
    let path = env.photos.path().to_str().unwrap();

    let added = env.json(&["roots", "add", path, "--mode", "once"]);
    assert_eq!(added["mode"], "once");
    assert_eq!(added["scan"]["added"], 1);
    let list = env.json(&["roots", "list"]);
    let roots = list.as_array().unwrap();
    assert_eq!(roots.len(), 1);
    assert_eq!(roots[0]["id"], added["id"]);
    assert!(
        roots[0]["path"]
            .as_str()
            .unwrap()
            .ends_with(env.photos.path().file_name().unwrap().to_str().unwrap())
    );

    env.cmd(&["roots", "list"])
        .assert()
        .success()
        .stdout(predicates::str::contains("once"));
    env.cmd(&["roots", "add", path, "--mode", "bogus"])
        .assert()
        .code(1)
        .stderr(predicates::str::contains("bogus"));
}

#[test]
fn cli_scan_e2e() {
    let env = Env::new();
    let rescan = env.json(&["scan", "--wait"]);
    assert_eq!(rescan[0]["summary"]["added"], 0);
    assert_eq!(rescan[0]["summary"]["unchanged"], 3);

    write_jpeg(&env.photos.path().join("IMG_20210614_120000.jpg"), 9);
    std::fs::remove_file(env.photos.path().join("IMG_20210611_120000.jpg")).unwrap();
    let root = env.json(&["roots", "list"])[0]["id"].as_i64().unwrap();
    let again = env.json(&["scan", "--root", &s(root)]);
    assert_eq!(again[0]["root"], root);
    assert_eq!(again[0]["summary"]["added"], 1);
    assert_eq!(again[0]["summary"]["missing"], 1);
    assert_eq!(
        env.all().len(),
        3,
        "missing files drop out of the default view"
    );
    env.cmd(&["scan"]).assert().success();
}

#[test]
fn cli_query_e2e() {
    let env = Env::new();
    let all = env.json(&["query", ""]);
    assert_eq!(all["total"], 3);
    let first = &all["photos"][0];
    assert!(
        first["path"]
            .as_str()
            .unwrap()
            .ends_with("IMG_20210613_120000.jpg"),
        "newest first"
    );
    assert_eq!(first["starred"], false);
    assert_eq!(first["taken_at"], "2021-06-13 12:00:00");

    assert_eq!(env.ids("date:2021-06-12").len(), 1);
    assert_eq!(env.ids("year:2020").len(), 0);
    let bad = env.json(&["query", "rating:abc"]);
    assert_eq!(bad["total"], 3);
    assert_eq!(bad["diagnostics"][0]["start"], 0);

    env.cmd(&["query", "date:2021-06-11"])
        .assert()
        .success()
        .stdout(predicates::str::contains("IMG_20210611_120000.jpg"));
    env.cmd(&["query", "x", "--source", "nowhere"])
        .assert()
        .code(1)
        .stderr(predicates::str::contains("unknown source"));
    let first_id = env.all()[0];
    env.json(&["star", &s(first_id)]);
    let starred = env.json(&["query", "", "--source", "starred"]);
    assert_eq!(starred["total"], 1);
}

#[test]
fn cli_star_e2e() {
    let env = Env::new();
    let ids = env.all();
    let r = env.json(&["star", &s(ids[0]), &s(ids[1])]);
    assert_eq!(r["changed"], 2);
    assert_eq!(env.ids("is:starred"), ids[..2]);
    let r = env.json(&["star", &s(ids[0]), "--off"]);
    assert_eq!(r["changed"], 1);
    assert_eq!(env.ids("is:starred"), ids[1..2]);
    env.cmd(&["star", "99999"]).assert().code(1);
    env.cmd(&["star"]).assert().code(1);
}

#[test]
fn cli_rate_e2e() {
    let env = Env::new();
    let ids = env.all();
    let r = env.json(&["rate", &s(ids[2]), "4"]);
    assert_eq!(r["rating"], 4);
    assert_eq!(env.ids("rating:>=4"), ids[2..]);
    env.json(&["rate", &s(ids[2]), "0"]);
    assert!(env.ids("rating:>=1").is_empty());
    env.cmd(&["rate", &s(ids[2]), "9"]).assert().code(1);
    env.cmd(&["rate", "99999", "3"]).assert().code(1);
}

#[test]
fn cli_tag_e2e() {
    let env = Env::new();
    let ids = env.all();
    let r = env.json(&["tag", "add", "Holiday", &s(ids[0]), &s(ids[1])]);
    assert_eq!(r["changed"], 2);
    assert_eq!(env.ids("tag:holiday"), ids[..2]);
    assert_eq!(env.ids("-tag:holiday"), ids[2..]);
    let r = env.json(&["tag", "add", "holiday", &s(ids[0])]);
    assert_eq!(r["changed"], 0, "already tagged");
    let r = env.json(&["tag", "remove", "holiday", &s(ids[0])]);
    assert_eq!(r["changed"], 1);
    assert_eq!(env.ids("tag:holiday"), ids[1..2]);
    env.cmd(&["tag", "add", "   ", &s(ids[0])])
        .assert()
        .failure();
}

#[test]
fn cli_album_e2e() {
    let env = Env::new();
    let ids = env.all();
    let album = env.json(&["album", "create", "Best of"]);
    let aid = album["id"].as_i64().unwrap();
    assert_eq!(album["name"], "Best of");
    let added = env.json(&["album", "add", &s(aid), &s(ids[0]), &s(ids[2])]);
    assert_eq!(added["added"], 2);
    let list = env.json(&["album", "list"]);
    assert_eq!(list[0]["name"], "Best of");
    assert_eq!(list[0]["photos"], 2);
    let in_album = env.json(&["query", "", "--source", &format!("album:{aid}")]);
    assert_eq!(in_album["total"], 2);
    assert_eq!(env.ids("album:\"best of\""), [ids[0], ids[2]]);
    env.cmd(&["album", "add", "999", &s(ids[0])])
        .assert()
        .code(1);
    env.cmd(&["album", "list"])
        .assert()
        .success()
        .stdout(predicates::str::contains("Best of"));
}

#[test]
fn cli_edit_e2e() {
    let env = Env::new();
    let id = s(env.all()[0]);
    let shown = env.json(&["edit", "show", &id]);
    assert_eq!(shown["ops"].as_array().unwrap().len(), 0);

    let pushed = env.json(&["edit", "push", &id, r#"{"op":"rotate_quarter","turns":1}"#]);
    assert_eq!(pushed["ops"][0]["op"], "rotate_quarter");
    env.json(&["edit", "push", &id, r#"{"op":"sepia"}"#]);
    assert_eq!(
        env.json(&["edit", "show", &id])["ops"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    assert_eq!(env.ids("is:edited").len(), 1);

    let undone = env.json(&["edit", "undo", &id]);
    assert_eq!(undone["ops"].as_array().unwrap().len(), 1);
    let reverted = env.json(&["edit", "revert", &id]);
    assert_eq!(reverted["ops"].as_array().unwrap().len(), 0);
    assert!(env.ids("is:edited").is_empty());

    env.cmd(&["edit", "push", &id, "{not json"])
        .assert()
        .code(1);
    env.cmd(&["edit", "push", &id, r#"{"op":"rotate_quarter","turns":3}"#])
        .assert()
        .code(1);
    env.cmd(&["edit", "show", "99999"]).assert().code(1);
}

#[test]
fn cli_render_e2e() {
    let env = Env::new();
    let id = s(env.all()[0]);
    env.json(&["edit", "push", &id, r#"{"op":"rotate_quarter","turns":1}"#]);
    let out = env.photos.path().join("../render-out.png");
    let out: PathBuf = out.parent().unwrap().join(format!("render-{id}.png"));
    let r = env.json(&["render", &id, out.to_str().unwrap(), "--max-edge", "40"]);
    assert_eq!(
        (r["width"].as_u64(), r["height"].as_u64()),
        (Some(30), Some(40))
    );
    assert_eq!(image::image_dimensions(&out).unwrap(), (30, 40));
    std::fs::remove_file(&out).unwrap();
    env.cmd(&["render", &id, out.to_str().unwrap(), "--max-edge", "0"])
        .assert()
        .code(1);
}

#[test]
fn cli_export_e2e() {
    let env = Env::new();
    let ids = env.all();
    let dest = tempfile::tempdir().unwrap();
    let to = dest.path().join("out");
    let r = env.json(&[
        "export",
        &s(ids[0]),
        &s(ids[1]),
        "--to",
        to.to_str().unwrap(),
        "--long-edge",
        "40",
        "--quality",
        "70",
        "--strip-gps",
    ]);
    assert_eq!(r["written"].as_array().unwrap().len(), 2);
    for f in r["written"].as_array().unwrap() {
        let (w, h) = image::image_dimensions(f["path"].as_str().unwrap()).unwrap();
        assert_eq!((w, h), (40, 30));
    }
    let again = env.json(&["export", &s(ids[0]), "--to", to.to_str().unwrap()]);
    assert!(
        again["written"][0]["path"]
            .as_str()
            .unwrap()
            .contains("(1)")
    );
    env.cmd(&[
        "export",
        &s(ids[0]),
        "--to",
        to.to_str().unwrap(),
        "--quality",
        "0",
    ])
    .assert()
    .code(1);
    env.cmd(&["export", "99999", "--to", to.to_str().unwrap()])
        .assert()
        .code(1);
}

#[test]
fn cli_thumbs_e2e() {
    let env = Env::new();
    let r = env.json(&["thumbs", "warm", "--size", "S"]);
    assert_eq!(r["requested"], 3);
    assert_eq!(r["failed"], 0);
    let again = env.json(&["thumbs", "warm", "--size", "S"]);
    assert_eq!(
        again["requested"], 0,
        "cached thumbnails are not regenerated"
    );
    let other = env.json(&["thumbs", "warm", "--size", "l"]);
    assert_eq!(other["requested"], 3);
    env.cmd(&["thumbs", "warm", "--size", "XL"])
        .assert()
        .code(1);
}

#[test]
fn cli_doctor_e2e() {
    let env = Env::new();
    let r = env.json(&["doctor"]);
    assert_eq!(r["photos"], 3);
    assert_eq!(r["roots"], 1);
    assert_eq!(r["integrity_problems"].as_array().unwrap().len(), 0);
    assert!(r["schema_version"].as_u64().unwrap() >= 2);
    assert!(
        r["data_dir"]
            .as_str()
            .unwrap()
            .starts_with(env.home.path().to_str().unwrap())
    );
    env.cmd(&["doctor"])
        .assert()
        .success()
        .stdout(predicates::str::contains("integrity ok"));
}

#[test]
fn cli_exit_codes() {
    let env = Env::new();
    env.cmd(&["--help"]).assert().code(0);
    env.cmd(&["--version"]).assert().code(0);
    env.cmd(&["no-such-command"]).assert().code(1);
    env.cmd(&["rate", "x", "y"]).assert().code(1);
    env.cmd(&["star", "99999"])
        .assert()
        .code(1)
        .stderr(predicates::str::contains("not found"));
    let json_err = env.cmd(&["star", "99999", "--json"]).assert().code(1);
    let v: Value = serde_json::from_slice(&json_err.get_output().stderr).unwrap();
    assert!(v["error"].as_str().unwrap().contains("not found"));

    let blocker = env.home.path().join("blocker");
    std::fs::write(&blocker, b"x").unwrap();
    let id = s(env.all()[0]);
    env.cmd(&["export", &id, "--to", blocker.to_str().unwrap()])
        .assert()
        .code(2);
    env.cmd(&["roots", "add", "/definitely/not/a/folder"])
        .assert()
        .failure();
    env.cmd(&["roots", "list"]).assert().code(0);
}

#[test]
fn cli_dupes_e2e() {
    let env = Env::empty();
    let blocks = |seed: u32| {
        image::RgbImage::from_fn(160, 120, |x, y| {
            let cell = (y * 6 / 120) * 8 + x * 8 / 160;
            let v = (cell.wrapping_add(seed).wrapping_mul(2_654_435_761) >> 13) as u8;
            image::Rgb([v, v, v])
        })
    };
    blocks(1).save(env.photos.path().join("a.jpg")).unwrap();
    blocks(99).save(env.photos.path().join("b.jpg")).unwrap();
    std::fs::copy(
        env.photos.path().join("a.jpg"),
        env.photos.path().join("a_copy.jpg"),
    )
    .unwrap();
    env.json(&["roots", "add", env.photos.path().to_str().unwrap()]);

    let r = env.json(&["dupes"]);
    let groups = r["groups"].as_array().unwrap();
    assert_eq!(groups.len(), 1, "{r}");
    assert_eq!(groups[0]["exact"], true);
    assert_eq!(groups[0]["ids"].as_array().unwrap().len(), 2);
    let all = env.all();
    assert_eq!(all.len(), 3);
    assert!(
        !groups[0]["ids"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| { i.as_i64() == Some(all[2]) }),
        "b.jpg is not a duplicate"
    );

    let text = env.cmd(&["dupes", "--threshold", "0"]).assert().success();
    assert!(String::from_utf8_lossy(&text.get_output().stdout).starts_with("exact"));
    env.cmd(&["dupes", "--threshold", "99"]).assert().code(1);
}

#[test]
fn cli_collage_e2e() {
    let env = Env::empty();
    for (i, n) in ["a", "b", "c"].iter().enumerate() {
        write_jpeg(&env.photos.path().join(format!("{n}.jpg")), i as u8);
    }
    env.json(&["roots", "add", env.photos.path().to_str().unwrap()]);
    let ids = env.all();
    let out = env.photos.path().join("out").join("c.jpg");
    let (a, b, c) = (s(ids[0]), s(ids[1]), s(ids[2]));
    let r = env.json(&[
        "collage",
        "pile",
        &a,
        &b,
        &c,
        "--out",
        out.to_str().unwrap(),
        "--size",
        "400x300",
        "--seed",
        "42",
    ]);
    assert_eq!(r["width"], 400);
    assert_eq!(image::image_dimensions(&out).unwrap(), (400, 300));

    env.cmd(&["collage", "spiral", &a, &b, "--out", out.to_str().unwrap()])
        .assert()
        .code(1);
    env.cmd(&["collage", "grid", &a, "--out", out.to_str().unwrap()])
        .assert()
        .code(1);
    env.cmd(&[
        "collage",
        "grid",
        &a,
        &b,
        "--out",
        out.to_str().unwrap(),
        "--size",
        "big",
    ])
    .assert()
    .code(1);
}

#[test]
fn cli_picasa_import_e2e() {
    let env = Env::empty();
    let dir = env.photos.path();
    write_jpeg(&dir.join("IMG_0001.JPG"), 1);
    write_jpeg(&dir.join("IMG_0002.JPG"), 2);
    std::fs::write(
        dir.join(".picasa.ini"),
        "[Picasa]\ndescription=Trip\n\
         [IMG_0001.JPG]\nstar=yes\ncaption=Hello\nkeywords=sea\nrotate=rotate(1)\n\
         filters=bw=1;nonsense=1;\n\
         [IMG_0003.JPG]\nstar=yes\n",
    )
    .unwrap();
    env.json(&["roots", "add", dir.to_str().unwrap()]);
    let folder = dir.to_str().unwrap();

    let dry = env.json(&["picasa-import", folder, "--dry-run"]);
    assert_eq!(dry["dry_run"], true);
    assert_eq!(dry["starred"], 1);
    let starred = |env: &Env| {
        env.json(&["query", "", "--source", "starred"])["photos"]
            .as_array()
            .unwrap()
            .len()
    };
    assert_eq!(starred(&env), 0);

    let done = env.json(&["picasa-import", folder]);
    assert_eq!(done["photos_matched"], 1);
    assert_eq!(done["photos_missing"], 1);
    assert_eq!(done["starred"], 1);
    assert_eq!(done["captions"], 1);
    assert_eq!(done["tags_added"], 1);
    assert_eq!(done["edits"], 1);
    assert_eq!(done["folder_captions"], 1);
    assert_eq!(done["skipped_filters"][0], "nonsense");
    assert_eq!(env.ids("sea").len(), 1);
    assert_eq!(starred(&env), 1);

    let again = env.json(&["picasa-import", folder, "--overwrite"]);
    assert_eq!(again["starred"], 0);
    assert_eq!(again["captions"], 0);
    assert_eq!(again["edits"], 0);

    env.cmd(&["picasa-import", "/definitely/not/a/folder"])
        .assert()
        .code(1);
}
