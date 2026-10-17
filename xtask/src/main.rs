//! Developer automation: `cargo xtask <ci|fixtures|goldens|perf|models>`.
#![allow(clippy::unwrap_used, clippy::expect_used)]

mod ci;
mod deps;

use std::process::ExitCode;

const USAGE: &str = "usage: cargo xtask <ci|fixtures|goldens|perf|models>";

/// Result of dispatching a subcommand.
#[derive(Debug, PartialEq, Eq)]
enum Dispatch {
    Ci,
    Fixtures,
    Goldens { update: Option<String> },
    Stub(&'static str, &'static str),
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
        Some("perf") => Dispatch::Stub("perf", "M11.1"),
        Some("models") => Dispatch::Stub("models", "M10.3"),
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
        Dispatch::Stub(name, task) => {
            println!("`xtask {name}` is not implemented yet (planned in {task})");
            ExitCode::SUCCESS
        }
        Dispatch::Usage(msg) => {
            eprintln!("{msg}");
            ExitCode::from(2)
        }
    }
}

/// Runs the golden-image tests, optionally rewriting the selected goldens (SPEC §12.4).
fn goldens(update: Option<&str>) -> ExitCode {
    let mut cmd =
        std::process::Command::new(std::env::var("CARGO").unwrap_or_else(|_| "cargo".into()));
    cmd.args(["test", "-p", "lb-imaging", "--locked", "--test", "goldens"]);
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
        for name in ["perf", "models"] {
            assert!(matches!(d(&[name]), Dispatch::Stub(n, _) if n == name));
        }
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
