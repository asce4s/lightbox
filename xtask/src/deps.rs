//! Dependency-rule checker (SPEC §4.2) driven by `cargo metadata --no-deps` JSON.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::BTreeSet;
use std::fmt;

/// Allowed normal workspace dependencies per crate (SPEC §4.2).
///
/// `lb-testkit` may depend on anything but may only ever be consumed as a dev-dependency.
const ALL_LIBS: &[&str] = &[
    "lb-core",
    "lb-catalog",
    "lb-media",
    "lb-imaging",
    "lb-thumbs",
    "lb-scan",
    "lb-search",
    "lb-jobs",
    "lb-faces",
    "lb-video",
    "lb-geo",
    "lb-picasa",
    "lb-ipc",
    "lb-app",
];

/// External crates restricted to a fixed set of workspace crates.
/// `(external crate name, crates allowed to depend on it)`.
const RESTRICTED_EXTERNAL: &[(&str, &[&str])] = &[
    ("rusqlite", &["lb-catalog"]),
    ("rusqlite_migration", &["lb-catalog"]),
    ("gpui-kit", &["lb-ui"]),
];

/// External crates nobody may depend on directly (must stay behind `gpui-kit`).
const FORBIDDEN_EXTERNAL_PREFIXES: &[&str] = &["gpui-pre-", "gpui-component"];
const FORBIDDEN_EXTERNAL_EXACT: &[&str] = &["gpui"];

fn allowed_workspace_deps(krate: &str) -> Option<&'static [&'static str]> {
    Some(match krate {
        "lb-core" => &[],
        "lb-catalog" | "lb-media" | "lb-jobs" | "lb-search" | "lb-geo" | "lb-picasa" | "lb-ipc" => {
            &["lb-core"]
        }
        "lb-imaging" => &["lb-core", "lb-media"],
        "lb-thumbs" => &["lb-core", "lb-media", "lb-imaging"],
        "lb-scan" => &["lb-core", "lb-catalog", "lb-media"],
        "lb-faces" | "lb-video" => &["lb-core", "lb-media"],
        "lb-app" => ALL_LIBS,
        "lb-ui" | "lb-cli" => &["lb-app", "lb-core"],
        "lightbox" => &["lb-app", "lb-ui"],
        "lb-testkit" => ALL_LIBS,
        _ => return None,
    })
}

/// Workspace crates exempt from the rules (tooling).
const EXEMPT: &[&str] = &["xtask"];

#[derive(Debug, Deserialize)]
struct Metadata {
    packages: Vec<Package>,
}

#[derive(Debug, Deserialize)]
struct Package {
    name: String,
    dependencies: Vec<Dependency>,
}

#[derive(Debug, Deserialize)]
struct Dependency {
    name: String,
    kind: Option<String>,
}

/// A single broken dependency rule.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Violation {
    /// Crate that declares the offending dependency.
    pub krate: String,
    /// Human readable explanation.
    pub message: String,
}

impl fmt::Display for Violation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}: {}", self.krate, self.message)
    }
}

/// Checks `cargo metadata --no-deps --format-version 1` output against SPEC §4.2.
pub fn check(metadata_json: &str) -> Result<Vec<Violation>> {
    let meta: Metadata =
        serde_json::from_str(metadata_json).context("parsing cargo metadata JSON")?;
    let members: BTreeSet<&str> = meta.packages.iter().map(|p| p.name.as_str()).collect();
    let mut out = Vec::new();
    let mut violation = |krate: &str, message: String| {
        out.push(Violation {
            krate: krate.to_owned(),
            message,
        });
    };

    for pkg in &meta.packages {
        if EXEMPT.contains(&pkg.name.as_str()) {
            continue;
        }
        let Some(allowed) = allowed_workspace_deps(&pkg.name) else {
            violation(
                &pkg.name,
                "no dependency rule defined for this crate (update xtask/src/deps.rs)".into(),
            );
            continue;
        };
        for dep in &pkg.dependencies {
            let is_dev = dep.kind.as_deref() == Some("dev");
            let name = dep.name.as_str();
            if members.contains(name) {
                if name == "lb-testkit" {
                    if !is_dev {
                        violation(
                            &pkg.name,
                            "lb-testkit may only be a [dev-dependency]".into(),
                        );
                    }
                    continue;
                }
                if !is_dev && !allowed.contains(&name) {
                    violation(
                        &pkg.name,
                        format!("must not depend on workspace crate {name} (SPEC §4.2)"),
                    );
                }
                continue;
            }
            let forbidden = FORBIDDEN_EXTERNAL_EXACT.contains(&name)
                || FORBIDDEN_EXTERNAL_PREFIXES
                    .iter()
                    .any(|p| name.starts_with(p));
            if forbidden {
                violation(
                    &pkg.name,
                    format!("must not depend on {name} directly; use gpui-kit"),
                );
                continue;
            }
            if let Some((_, owners)) = RESTRICTED_EXTERNAL.iter().find(|(n, _)| *n == name)
                && !is_dev
                && !owners.contains(&pkg.name.as_str())
            {
                violation(
                    &pkg.name,
                    format!("{name} is restricted to {}", owners.join(", ")),
                );
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    type Deps<'a> = &'a [(&'a str, Option<&'a str>)];

    fn metadata(pkgs: &[(&str, Deps)]) -> String {
        let packages: Vec<_> = pkgs
            .iter()
            .map(|(name, deps)| {
                let deps: Vec<_> = deps
                    .iter()
                    .map(|(n, kind)| serde_json::json!({ "name": n, "kind": kind }))
                    .collect();
                serde_json::json!({ "name": name, "dependencies": deps })
            })
            .collect();
        serde_json::json!({ "packages": packages }).to_string()
    }

    #[test]
    fn xtask_dependency_rules_detect_violation() {
        let json = metadata(&[
            ("lb-core", &[]),
            ("lb-catalog", &[("lb-core", None)]),
            (
                "lb-ui",
                &[
                    ("lb-app", None),
                    ("lb-catalog", None),
                    ("gpui-component", None),
                ],
            ),
            ("lb-app", &[("lb-core", None), ("lb-catalog", None)]),
            ("lb-search", &[("rusqlite", None)]),
            ("lb-media", &[("lb-testkit", None)]),
            ("lb-testkit", &[]),
        ]);
        let v = check(&json).unwrap();
        let has = |krate: &str, needle: &str| {
            v.iter()
                .any(|x| x.krate == krate && x.message.contains(needle))
        };
        assert!(has("lb-ui", "lb-catalog"), "{v:?}");
        assert!(has("lb-ui", "gpui-component"), "{v:?}");
        assert!(has("lb-search", "rusqlite"), "{v:?}");
        assert!(has("lb-media", "lb-testkit"), "{v:?}");
        assert!(
            !v.iter()
                .any(|x| x.krate == "lb-app" || x.krate == "lb-catalog"),
            "{v:?}"
        );
    }

    #[test]
    fn xtask_dependency_rules_accept_valid_graph() {
        let json = metadata(&[
            ("lb-core", &[]),
            (
                "lb-catalog",
                &[
                    ("lb-core", None),
                    ("rusqlite", None),
                    ("lb-testkit", Some("dev")),
                ],
            ),
            ("lb-ui", &[("lb-app", None), ("gpui-kit", None)]),
            ("lb-app", &[("lb-core", None), ("lb-catalog", None)]),
            ("lb-testkit", &[("lb-core", None)]),
            ("xtask", &[("lb-core", None)]),
        ]);
        assert_eq!(check(&json).unwrap(), Vec::new());
    }

    #[test]
    fn xtask_dependency_rules_reject_unknown_workspace_crate() {
        let json = metadata(&[("lb-mystery", &[])]);
        let v = check(&json).unwrap();
        assert_eq!(v.len(), 1, "{v:?}");
        assert!(v[0].message.contains("no dependency rule"));
    }

    #[test]
    fn xtask_dependency_rules_reject_malformed_json() {
        assert!(check("not json").is_err());
    }
}
