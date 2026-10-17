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
    Stub(&'static str, &'static str),
    Usage(String),
}

fn dispatch(args: &[String]) -> Dispatch {
    match args.first().map(String::as_str) {
        Some("ci") => Dispatch::Ci,
        Some("fixtures") => Dispatch::Fixtures,
        Some("goldens") => Dispatch::Stub("goldens", "M5.3"),
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
        for name in ["goldens", "perf", "models"] {
            assert!(matches!(d(&[name]), Dispatch::Stub(n, _) if n == name));
        }
        assert!(matches!(d(&[]), Dispatch::Usage(_)));
        assert!(matches!(d(&["nope"]), Dispatch::Usage(_)));
    }
}
