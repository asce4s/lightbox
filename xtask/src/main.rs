//! Developer automation: `cargo xtask <ci|fixtures|goldens|perf|coverage|package|models>`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod ci;
mod coverage;
mod deps;
mod icon;
mod models;
mod package;

use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <ci|fixtures|goldens|perf|coverage|package|models>";

/// Result of dispatching a subcommand.
#[derive(Debug, PartialEq, Eq)]
enum Dispatch {
    Ci,
    Fixtures,
    Goldens { update: Option<String> },
    Models { dir: Option<String> },
    Perf { benches: bool },
    Coverage { from_json: Option<String> },
    Package,
    Usage(String),
}

fn dispatch(args: &[String]) -> Dispatch {
    match args.first().map(String::as_str) {
        Some("ci") => Dispatch::Ci,
        Some("fixtures") => Dispatch::Fixtures,
        Some("goldens") => match args.get(1).map(String::as_str) {
            None => Dispatch::Goldens { update: None },
            Some("--update") => match args.get(2) {
                Some(target) if !target.starts_with('-') => Dispatch::Goldens {
                    update: Some(target.clone()),
                },
                _ => Dispatch::Usage(
                    "usage: cargo xtask goldens [--update <suite|name|all>]".to_owned(),
                ),
            },
            Some(_) => {
                Dispatch::Usage("usage: cargo xtask goldens [--update <suite|name|all>]".to_owned())
            }
        },
        Some("perf") => match args.get(1).map(String::as_str) {
            None => Dispatch::Perf { benches: false },
            Some("--bench") => Dispatch::Perf { benches: true },
            Some(_) => Dispatch::Usage("usage: cargo xtask perf [--bench]".to_owned()),
        },
        Some("coverage") => match args.get(1).map(String::as_str) {
            None => Dispatch::Coverage { from_json: None },
            Some("--from-json") => match args.get(2) {
                Some(path) => Dispatch::Coverage {
                    from_json: Some(path.clone()),
                },
                None => {
                    Dispatch::Usage("usage: cargo xtask coverage [--from-json <path>]".to_owned())
                }
            },
            Some(_) => {
                Dispatch::Usage("usage: cargo xtask coverage [--from-json <path>]".to_owned())
            }
        },
        Some("package") => Dispatch::Package,
        Some("models") => match args.get(1).map(String::as_str) {
            None => Dispatch::Models { dir: None },
            Some("--dir") => match args.get(2) {
                Some(dir) => Dispatch::Models {
                    dir: Some(dir.clone()),
                },
                None => Dispatch::Usage("usage: cargo xtask models [--dir <path>]".to_owned()),
            },
            Some(_) => Dispatch::Usage("usage: cargo xtask models [--dir <path>]".to_owned()),
        },
        Some(other) => Dispatch::Usage(format!("unknown subcommand `{other}`\n{USAGE}")),
        None => Dispatch::Usage(USAGE.to_owned()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match dispatch(&args) {
        Dispatch::Ci => {
            if ci::run() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            }
        }
        Dispatch::Fixtures => match lb_testkit::fixtures::generate_default() {
            Ok(dir) => {
                println!("fixtures generated in {}", dir.display());
                ExitCode::SUCCESS
            }
            Err(e) => {
                eprintln!("fixture generation failed: {e:#}");
                ExitCode::FAILURE
            }
        },
        Dispatch::Goldens { update } => goldens(update.as_deref()),
        Dispatch::Models { dir } => match models::run(dir.as_deref()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("model download failed: {e:#}");
                ExitCode::FAILURE
            }
        },
        Dispatch::Perf { benches } => perf(benches),
        Dispatch::Package => match package::run() {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("packaging failed: {e:#}");
                ExitCode::FAILURE
            }
        },
        Dispatch::Coverage { from_json } => match coverage::run(from_json.as_deref()) {
            Ok(()) => ExitCode::SUCCESS,
            Err(e) => {
                eprintln!("{e:#}");
                ExitCode::FAILURE
            }
        },
        Dispatch::Usage(msg) => {
            eprintln!("{msg}");
            ExitCode::from(2)
        }
    }
}

/// Packages that own a `tests/perf.rs` budget check (SPEC §11).
const PERF_PACKAGES: [&str; 3] = ["lb-app", "lb-thumbs", "lb-imaging"];

/// Packages with criterion benches.
const BENCH_PACKAGES: [&str; 4] = ["lb-search", "lb-imaging", "lb-thumbs", "lb-catalog"];

/// Runs the performance budget tests in release mode, enforcing SPEC §11 (budget + 50%), and
/// optionally the criterion benches.
fn perf(benches: bool) -> ExitCode {
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let mut test = std::process::Command::new(&cargo);
    test.args(["test", "--release", "--locked"]);
    for package in PERF_PACKAGES {
        test.args(["-p", package]);
    }
    test.args(["--test", "perf", "--", "--nocapture", "--test-threads=1"]);
    test.env(lb_testkit::perf::PERF_ENV, "1");
    let mut ok = run_status(&mut test);
    ok &= viewer_process_perf();
    if benches {
        let mut bench = std::process::Command::new(&cargo);
        bench.args(["bench", "--locked"]);
        for package in BENCH_PACKAGES {
            bench.args(["-p", package]);
        }
        ok &= run_status(&mut bench);
    }
    if ok {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    }
}

/// Cold `lightbox view --perf-json` on a generated 24 MP JPEG (SPEC-VIEWER §V12).
///
/// Timing is informational when `CI` is set. A local run enforces the cold-start budgets.
fn viewer_process_perf() -> bool {
    let dir = std::env::temp_dir().join(format!("lightbox-perf-{}", std::process::id()));
    if let Err(error) = std::fs::create_dir_all(&dir) {
        eprintln!("could not create {dir:?}: {error}");
        return false;
    }
    let path = dir.join("frame.jpg");
    let image = image::RgbImage::from_fn(6000, 4000, |x, y| {
        image::Rgb([(x % 251) as u8, (y % 251) as u8, 32])
    });
    if let Err(error) = image.save(&path) {
        eprintln!("could not write the perf jpeg: {error}");
        return false;
    }
    let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
    let output = std::process::Command::new(cargo)
        .args(["run", "-p", "lightbox", "--release", "--locked", "--"])
        .arg("view")
        .arg("--perf-json")
        .arg(&path)
        .env("LIGHTBOX_HOME", dir.join("home"))
        .output();
    let _ = std::fs::remove_dir_all(&dir);
    let output = match output {
        Ok(output) => output,
        Err(error) => {
            eprintln!("could not run lightbox view --perf-json: {error}");
            return false;
        }
    };
    let stdout = String::from_utf8_lossy(&output.stdout);
    if !output.status.success() {
        eprintln!(
            "lightbox view --perf-json failed\n{stdout}\n{}",
            String::from_utf8_lossy(&output.stderr)
        );
        return false;
    }
    let Some(line) = stdout
        .lines()
        .rev()
        .find(|line| line.trim_start().starts_with('{'))
    else {
        eprintln!("viewer perf JSON missing from stdout:\n{stdout}");
        return false;
    };
    let json: serde_json::Value = match serde_json::from_str(line) {
        Ok(json) => json,
        Err(error) => {
            eprintln!("viewer perf JSON is not an object: {error}: {line}");
            return false;
        }
    };
    let first_frame = json
        .get("first_frame_ms")
        .and_then(serde_json::Value::as_u64);
    let first_image = json
        .get("first_image_ms")
        .and_then(serde_json::Value::as_u64);
    let l2 = json.get("l2_ms").and_then(serde_json::Value::as_u64);
    println!(
        "perf view --perf-json: first_frame {first_frame:?} ms, first_image {first_image:?} ms, l2 {l2:?} ms"
    );
    if std::env::var_os("CI").is_some() {
        println!("perf view --perf-json: informational on CI");
        return first_frame.is_some() && first_image.is_some() && l2.is_some();
    }
    let within = first_frame.is_some_and(|ms| ms <= 300)
        && first_image.is_some_and(|ms| ms <= 500)
        && l2.is_some_and(|ms| ms <= 500);
    if !within {
        eprintln!("cold viewer budget exceeded (frame ≤ 300 ms, image and l2 ≤ 500 ms): {line}");
    }
    within
}

fn run_status(cmd: &mut std::process::Command) -> bool {
    match cmd.status() {
        Ok(status) => status.success(),
        Err(e) => {
            eprintln!("could not run cargo: {e}");
            false
        }
    }
}

/// Runs the golden-image tests, optionally rewriting the selected goldens (SPEC §12.4).
fn goldens(update: Option<&str>) -> ExitCode {
    let mut cmd =
        std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.args(["test", "-p", "lb-imaging", "--locked"]);
    cmd.args([
        "--test", "goldens", "--test", "collage", "--test", "ambient",
    ]);
    if let Some(target) = update {
        cmd.env(lb_testkit::UPDATE_ENV, target);
        cmd.args(["--", "--nocapture"]);
        println!(
            "updating goldens matching `{target}` - inspect target/test-artifacts and log the reason in TASKS.md Decisions"
        );
    }
    match cmd.status() {
        Ok(status) if status.success() => ExitCode::SUCCESS,
        Ok(_) => ExitCode::FAILURE,
        Err(e) => {
            eprintln!("could not run cargo: {e}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &[&str]) -> Dispatch {
        dispatch(&s.iter().map(|x| (*x).to_owned()).collect::<Vec<_>>())
    }

    #[test]
    fn xtask_dispatches_all_subcommands() {
        assert_eq!(d(&["ci"]), Dispatch::Ci);
        assert_eq!(d(&["fixtures"]), Dispatch::Fixtures);
        assert_eq!(d(&["perf"]), Dispatch::Perf { benches: false });
        assert_eq!(d(&["perf", "--bench"]), Dispatch::Perf { benches: true });
        assert!(matches!(d(&["perf", "x"]), Dispatch::Usage(_)));
        assert_eq!(d(&["coverage"]), Dispatch::Coverage { from_json: None });
        assert_eq!(
            d(&["coverage", "--from-json", "c.json"]),
            Dispatch::Coverage {
                from_json: Some("c.json".into())
            }
        );
        assert!(matches!(
            d(&["coverage", "--from-json"]),
            Dispatch::Usage(_)
        ));
        assert_eq!(d(&["package"]), Dispatch::Package);
        assert_eq!(d(&["models"]), Dispatch::Models { dir: None });
        assert_eq!(
            d(&["models", "--dir", "/m"]),
            Dispatch::Models {
                dir: Some("/m".into())
            }
        );
        assert!(matches!(d(&["models", "--dir"]), Dispatch::Usage(_)));
        assert_eq!(d(&["goldens"]), Dispatch::Goldens { update: None });
        assert_eq!(
            d(&["goldens", "--update", "golden_x"]),
            Dispatch::Goldens {
                update: Some("golden_x".into())
            }
        );
        assert!(matches!(d(&["goldens", "--update"]), Dispatch::Usage(_)));
        assert!(matches!(d(&["goldens", "bogus"]), Dispatch::Usage(_)));
        assert!(matches!(d(&[]), Dispatch::Usage(_)));
        assert!(matches!(d(&["nope"]), Dispatch::Usage(_)));
    }
}
