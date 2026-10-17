//! `cargo xtask ci`: the single gate run by the agent and by CI (SPEC §13).

use crate::deps;
use anyhow::{Context, Result};
use std::process::Command;

/// What a CI step does.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Run `cargo <args>` with extra environment variables.
    Cargo {
        args: Vec<String>,
        env: Vec<(String, String)>,
    },
    /// Check the dependency rules from SPEC §4.2.
    DepRules,
    /// Generate test fixtures (`cargo xtask fixtures`).
    Fixtures,
    /// Run `cargo deny check`.
    Deny,
}

/// One numbered step of the CI pipeline.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    /// 1-based step number from SPEC §13.
    pub number: usize,
    /// Short description.
    pub name: &'static str,
    /// What to run.
    pub action: Action,
}

/// Result of executing one step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Outcome {
    /// Step succeeded.
    Passed,
    /// Step failed; the pipeline stops.
    Failed(String),
    /// Step could not run because tooling is not ready yet; reported loudly.
    Skipped(String),
}

fn cargo(args: &[&str], env: &[(&str, &str)]) -> Action {
    Action::Cargo {
        args: args.iter().map(|s| (*s).to_owned()).collect(),
        env: env
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect(),
    }
}

/// The ordered CI plan, steps 1–9 of SPEC §13.
pub fn plan() -> Vec<Step> {
    let steps = [
        (
            "cargo fmt --check",
            cargo(&["fmt", "--all", "--", "--check"], &[]),
        ),
        (
            "cargo clippy -D warnings",
            cargo(
                &[
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--all-features",
                    "--locked",
                    "--",
                    "-D",
                    "warnings",
                ],
                &[],
            ),
        ),
        ("dependency rules (SPEC 4.2)", Action::DepRules),
        ("fixtures", Action::Fixtures),
        (
            "tests (default features)",
            cargo(&["test", "--workspace", "--locked"], &[]),
        ),
        (
            "tests (all features)",
            cargo(&["test", "--workspace", "--all-features", "--locked"], &[]),
        ),
        (
            "tests (lb-app, no default features)",
            cargo(
                &["test", "-p", "lb-app", "--no-default-features", "--locked"],
                &[],
            ),
        ),
        (
            "cargo doc -D warnings",
            cargo(
                &["doc", "--workspace", "--no-deps", "--locked"],
                &[("RUSTDOCFLAGS", "-D warnings")],
            ),
        ),
        ("cargo deny check", Action::Deny),
    ];
    steps
        .into_iter()
        .enumerate()
        .map(|(i, (name, action))| Step {
            number: i + 1,
            name,
            action,
        })
        .collect()
}

/// Executes a single step. Abstracted so the runner can be unit tested.
pub trait Executor {
    /// Runs `step` and reports the outcome.
    fn execute(&mut self, step: &Step) -> Outcome;
}

/// Runs `steps` in order, stopping at the first failure.
pub fn run_plan(steps: &[Step], exec: &mut dyn Executor) -> Vec<(Step, Outcome)> {
    let mut results = Vec::new();
    for step in steps {
        println!("\n=== [{}/{}] {} ===", step.number, steps.len(), step.name);
        let outcome = exec.execute(step);
        match &outcome {
            Outcome::Skipped(why) => println!("!!! SKIPPED: {} ({why})", step.name),
            Outcome::Failed(why) => println!("FAILED: {} ({why})", step.name),
            Outcome::Passed => println!("ok: {}", step.name),
        }
        let failed = matches!(outcome, Outcome::Failed(_));
        results.push((step.clone(), outcome));
        if failed {
            break;
        }
    }
    results
}

/// Renders the final report and returns `true` when no step failed.
pub fn summarize(results: &[(Step, Outcome)]) -> (String, bool) {
    let mut text = String::from("\n=== CI summary ===\n");
    let mut ok = true;
    for (step, outcome) in results {
        let line = match outcome {
            Outcome::Passed => format!("  PASSED   {}", step.name),
            Outcome::Skipped(why) => format!("  SKIPPED  {} -- {why}", step.name),
            Outcome::Failed(why) => {
                ok = false;
                format!("  FAILED   {} -- {why}", step.name)
            }
        };
        text.push_str(&line);
        text.push('\n');
    }
    let skipped = results
        .iter()
        .filter(|(_, o)| matches!(o, Outcome::Skipped(_)))
        .count();
    if skipped > 0 {
        text.push_str(&format!(
            "WARNING: {skipped} step(s) SKIPPED. Remove skips before the end of M1.\n"
        ));
    }
    text.push_str(if ok { "CI green\n" } else { "CI RED\n" });
    (text, ok)
}

/// Executor that runs the real tools.
pub struct RealExecutor;

impl RealExecutor {
    fn cargo_cmd() -> Command {
        Command::new(std::env::var_os("CARGO").unwrap_or_else(|| "cargo".into()))
    }

    fn dep_rules() -> Result<Outcome> {
        let out = Self::cargo_cmd()
            .args(["metadata", "--no-deps", "--format-version", "1", "--locked"])
            .output()
            .context("running cargo metadata")?;
        if !out.status.success() {
            return Ok(Outcome::Failed(format!(
                "cargo metadata failed: {}",
                String::from_utf8_lossy(&out.stderr)
            )));
        }
        let violations = deps::check(&String::from_utf8_lossy(&out.stdout))?;
        if violations.is_empty() {
            return Ok(Outcome::Passed);
        }
        for v in &violations {
            println!("violation: {v}");
        }
        Ok(Outcome::Failed(format!(
            "{} dependency rule violation(s)",
            violations.len()
        )))
    }

    fn deny() -> Outcome {
        let available = Self::cargo_cmd()
            .args(["deny", "--version"])
            .output()
            .is_ok_and(|o| o.status.success());
        if !available {
            let why = "cargo-deny is not installed (cargo install cargo-deny --locked)";
            return if std::env::var_os("CI").is_some() {
                Outcome::Failed(why.into())
            } else {
                Outcome::Skipped(why.into())
            };
        }
        run_status(Self::cargo_cmd().args(["deny", "check"]))
    }
}

fn run_status(cmd: &mut Command) -> Outcome {
    match cmd.status() {
        Ok(s) if s.success() => Outcome::Passed,
        Ok(s) => Outcome::Failed(format!("exited with {s}")),
        Err(e) => Outcome::Failed(format!("could not start: {e}")),
    }
}

impl Executor for RealExecutor {
    fn execute(&mut self, step: &Step) -> Outcome {
        match &step.action {
            Action::Cargo { args, env } => {
                let mut cmd = Self::cargo_cmd();
                cmd.args(args).envs(env.iter().map(|(k, v)| (k, v)));
                run_status(&mut cmd)
            }
            Action::DepRules => {
                Self::dep_rules().unwrap_or_else(|e| Outcome::Failed(format!("{e:#}")))
            }
            Action::Fixtures => Outcome::Skipped("fixture generator arrives in M0.5".into()),
            Action::Deny => Self::deny(),
        }
    }
}

/// Entry point for `cargo xtask ci`; returns the process success flag.
pub fn run() -> bool {
    let results = run_plan(&plan(), &mut RealExecutor);
    let (text, ok) = summarize(&results);
    print!("{text}");
    ok
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fake {
        fail_at: Option<usize>,
        skip_at: Option<usize>,
        ran: Vec<usize>,
    }

    impl Executor for Fake {
        fn execute(&mut self, step: &Step) -> Outcome {
            self.ran.push(step.number);
            if self.fail_at == Some(step.number) {
                Outcome::Failed("boom".into())
            } else if self.skip_at == Some(step.number) {
                Outcome::Skipped("tool missing".into())
            } else {
                Outcome::Passed
            }
        }
    }

    #[test]
    fn ci_plan_lists_spec_steps_in_order() {
        let p = plan();
        assert_eq!(p.len(), 9);
        assert!(p.iter().enumerate().all(|(i, s)| s.number == i + 1));
        let cargo_args = |i: usize| match &p[i].action {
            Action::Cargo { args, .. } => args.join(" "),
            other => format!("{other:?}"),
        };
        assert_eq!(cargo_args(0), "fmt --all -- --check");
        assert_eq!(
            cargo_args(1),
            "clippy --workspace --all-targets --all-features --locked -- -D warnings"
        );
        assert_eq!(p[2].action, Action::DepRules);
        assert_eq!(p[3].action, Action::Fixtures);
        assert_eq!(cargo_args(4), "test --workspace --locked");
        assert_eq!(cargo_args(5), "test --workspace --all-features --locked");
        assert_eq!(
            cargo_args(6),
            "test -p lb-app --no-default-features --locked"
        );
        assert_eq!(cargo_args(7), "doc --workspace --no-deps --locked");
        assert!(
            matches!(&p[7].action, Action::Cargo { env, .. } if env == &[("RUSTDOCFLAGS".to_owned(), "-D warnings".to_owned())])
        );
        assert_eq!(p[8].action, Action::Deny);
    }

    #[test]
    fn ci_stops_at_first_failure() {
        let mut fake = Fake {
            fail_at: Some(3),
            skip_at: None,
            ran: Vec::new(),
        };
        let results = run_plan(&plan(), &mut fake);
        assert_eq!(fake.ran, vec![1, 2, 3]);
        let (text, ok) = summarize(&results);
        assert!(!ok);
        assert!(text.contains("CI RED"));
    }

    #[test]
    fn ci_reports_skipped_steps_loudly_but_stays_green() {
        let mut fake = Fake {
            fail_at: None,
            skip_at: Some(4),
            ran: Vec::new(),
        };
        let results = run_plan(&plan(), &mut fake);
        assert_eq!(fake.ran.len(), 9);
        let (text, ok) = summarize(&results);
        assert!(ok);
        assert!(text.contains("SKIPPED"));
        assert!(text.contains("WARNING: 1 step(s) SKIPPED"));
    }
}
