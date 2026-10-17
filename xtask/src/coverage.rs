//! `cargo xtask coverage`: line-coverage thresholds of SPEC §12.7 over `cargo llvm-cov` output.

use anyhow::{Context, Result, bail};
use serde_json::Value;
use std::collections::BTreeMap;
use std::path::Path;
use std::process::Command;

/// Crates with their own minimum line coverage, in percent.
pub const CRATE_THRESHOLDS: [(&str, f64); 6] = [
    ("lb-core", 80.0),
    ("lb-catalog", 80.0),
    ("lb-search", 80.0),
    ("lb-imaging", 80.0),
    ("lb-scan", 80.0),
    ("lb-app", 80.0),
];

/// Minimum line coverage of the whole workspace (without the excluded crates), in percent.
pub const WORKSPACE_THRESHOLD: f64 = 70.0;

/// Paths that do not count towards any threshold: tests, benches, tooling and the UI crate.
pub const IGNORE_REGEX: &str = "(/tests/|/benches/|/xtask/|/lb-ui/|/apps/|/lb-testkit/)";

/// Where the JSON summary is written.
pub const REPORT_PATH: &str = "target/coverage.json";

/// Executable lines seen and hit.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Lines {
    /// Executable lines.
    pub count: u64,
    /// Lines run at least once.
    pub covered: u64,
}

impl Lines {
    /// Percentage of covered lines; an empty set counts as fully covered.
    pub fn percent(self) -> f64 {
        if self.count == 0 {
            100.0
        } else {
            self.covered as f64 * 100.0 / self.count as f64
        }
    }
}

/// Outcome of comparing a coverage report with the thresholds.
#[derive(Debug, Clone, PartialEq)]
pub struct Evaluation {
    /// Lines per crate (`crates/<name>`), for crates that appear in the report.
    pub crates: BTreeMap<String, Lines>,
    /// Lines of every file in the report.
    pub total: Lines,
    /// Human-readable threshold failures.
    pub failures: Vec<String>,
}

/// The crate directory a source path belongs to, e.g. `lb-app` for `.../crates/lb-app/src/x.rs`.
pub fn crate_of(path: &str) -> Option<&str> {
    let start = path.find("crates/").or_else(|| path.find("crates\\"))?;
    path[start + "crates/".len()..]
        .split(['/', '\\'])
        .next()
        .filter(|s| !s.is_empty())
}

/// Reads a `cargo llvm-cov --json` export and applies the thresholds.
pub fn evaluate(report: &Value) -> Result<Evaluation> {
    let files = report
        .pointer("/data/0/files")
        .and_then(Value::as_array)
        .context("coverage report has no data[0].files")?;
    let mut crates: BTreeMap<String, Lines> = BTreeMap::new();
    let mut total = Lines::default();
    for file in files {
        let name = file
            .get("filename")
            .and_then(Value::as_str)
            .context("file entry without a filename")?;
        let lines = file.pointer("/summary/lines");
        let count = lines
            .and_then(|l| l.get("count"))
            .and_then(Value::as_u64)
            .context("file entry without a line count")?;
        let covered = lines
            .and_then(|l| l.get("covered"))
            .and_then(Value::as_u64)
            .context("file entry without covered lines")?;
        total.count += count;
        total.covered += covered;
        if let Some(krate) = crate_of(name) {
            let entry = crates.entry(krate.to_owned()).or_default();
            entry.count += count;
            entry.covered += covered;
        }
    }
    let mut failures = Vec::new();
    for (name, minimum) in CRATE_THRESHOLDS {
        match crates.get(name) {
            None => failures.push(format!("{name}: missing from the coverage report")),
            Some(lines) if lines.percent() < minimum => failures.push(format!(
                "{name}: {:.1}% of lines is below {minimum:.0}%",
                lines.percent()
            )),
            Some(_) => {}
        }
    }
    if total.percent() < WORKSPACE_THRESHOLD {
        failures.push(format!(
            "workspace: {:.1}% of lines is below {WORKSPACE_THRESHOLD:.0}%",
            total.percent()
        ));
    }
    Ok(Evaluation {
        crates,
        total,
        failures,
    })
}

/// Prints a coverage table.
pub fn print(eval: &Evaluation) {
    for (name, lines) in &eval.crates {
        println!(
            "{name:<14} {:>6.1}%  ({}/{} lines)",
            lines.percent(),
            lines.covered,
            lines.count
        );
    }
    println!(
        "{:<14} {:>6.1}%  ({}/{} lines)",
        "workspace",
        eval.total.percent(),
        eval.total.covered,
        eval.total.count
    );
}

/// Runs the test suite under `cargo llvm-cov` (or reads `from_json`) and checks the thresholds.
pub fn run(from_json: Option<&str>) -> Result<()> {
    let path = match from_json {
        Some(p) => p.to_owned(),
        None => {
            let cargo = std::env::var("CARGO").unwrap_or_else(|_| "cargo".into());
            let status = Command::new(cargo)
                .args([
                    "llvm-cov",
                    "--workspace",
                    "--locked",
                    "--json",
                    "--summary-only",
                ])
                .args(["--output-path", REPORT_PATH])
                .args(["--ignore-filename-regex", IGNORE_REGEX])
                .status()
                .context(
                    "could not run cargo llvm-cov (install it with `cargo install cargo-llvm-cov`)",
                )?;
            if !status.success() {
                bail!("cargo llvm-cov failed ({status})");
            }
            REPORT_PATH.to_owned()
        }
    };
    let text =
        std::fs::read_to_string(Path::new(&path)).with_context(|| format!("reading {path}"))?;
    let eval = evaluate(&serde_json::from_str(&text)?)?;
    print(&eval);
    if eval.failures.is_empty() {
        Ok(())
    } else {
        bail!(
            "coverage below threshold:\n  {}",
            eval.failures.join("\n  ")
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn file(name: &str, count: u64, covered: u64) -> Value {
        json!({ "filename": name, "summary": { "lines": { "count": count, "covered": covered } } })
    }

    fn report(files: Vec<Value>) -> Value {
        json!({ "data": [{ "files": files }] })
    }

    fn passing() -> Vec<Value> {
        CRATE_THRESHOLDS
            .iter()
            .map(|(n, _)| file(&format!("/w/crates/{n}/src/lib.rs"), 100, 90))
            .collect()
    }

    #[test]
    fn crate_names_come_from_the_path() {
        assert_eq!(crate_of("/w/crates/lb-app/src/x.rs"), Some("lb-app"));
        assert_eq!(
            crate_of("C:\\w\\crates\\lb-core\\src\\x.rs"),
            Some("lb-core")
        );
        assert_eq!(crate_of("/w/apps/lightbox/src/main.rs"), None);
    }

    #[test]
    fn coverage_thresholds_pass_and_fail() {
        let ok = evaluate(&report(passing())).unwrap();
        assert!(ok.failures.is_empty(), "{:?}", ok.failures);
        assert!((ok.total.percent() - 90.0).abs() < 1e-9);

        let mut files = passing();
        files.push(file("/w/crates/lb-app/src/y.rs", 1000, 0));
        let bad = evaluate(&report(files)).unwrap();
        assert!(bad.failures.iter().any(|f| f.starts_with("lb-app")));
        assert!(bad.failures.iter().any(|f| f.starts_with("workspace")));

        let missing = evaluate(&report(vec![file("/w/crates/lb-core/src/a.rs", 10, 10)])).unwrap();
        assert!(
            missing
                .failures
                .iter()
                .any(|f| f.contains("lb-search: missing"))
        );
    }

    #[test]
    fn malformed_reports_are_errors() {
        assert!(evaluate(&json!({})).is_err());
        assert!(evaluate(&report(vec![json!({ "filename": "x" })])).is_err());
    }
}
