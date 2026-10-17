//! M11.5: the README stays in step with the command line.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use assert_cmd::Command;
use std::path::Path;

#[test]
fn readme_documents_every_cli_subcommand() {
    let help = Command::cargo_bin("lightbox-cli")
        .unwrap()
        .arg("--help")
        .assert()
        .success();
    let help = String::from_utf8(help.get_output().stdout.clone()).unwrap();
    let readme =
        std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("../../README.md"))
            .unwrap();
    let commands: Vec<&str> = help
        .lines()
        .skip_while(|l| !l.starts_with("Commands:"))
        .skip(1)
        .take_while(|l| l.starts_with("  "))
        .filter_map(|l| l.split_whitespace().next())
        .filter(|c| *c != "help")
        .collect();
    assert!(commands.len() >= 14, "{commands:?}");
    for command in commands {
        assert!(
            readme.contains(&format!("lightbox-cli {command}")),
            "README.md does not show `lightbox-cli {command}`"
        );
    }
}
