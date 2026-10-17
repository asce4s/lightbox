//! Developer automation: `cargo xtask <ci|fixtures|goldens|perf|coverage|models>`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod ci;
mod coverage;
mod deps;
mod models;

use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <ci|fixtures|goldens|perf|coverage|models>";

/// Result of dispatching a subcommand.
#[derive(Debug, PartialEq, Eq)]
enum Dispatch {
    Ci,
    Fixtures,
    Goldens { update: Option<String> },
    Models { dir: Option<String> },
    Perf { benches: bool },
    Coverage { from_json: Option<String> },
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
    cmd.args(["--test", "goldens", "--test", "collage"]);
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
