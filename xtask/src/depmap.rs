// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Dependency gate (redline C5). Two assertions, one authority each:
//! actual crate edges (normal + build deps) are a subset of the `depmap`
//! fenced block in ARCHITECTURE.md section 2 — no hidden edge, kernel stays
//! at zero; and `pub trait` appears only in seam files listed in section 3 —
//! one adapter is a hypothetical seam, so a trait outside the seam list is
//! decoration, not architecture.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;
use std::process::Command;

use crate::report::{Violation, XtaskError};
use crate::walk;

const ARCH: &str = "ARCHITECTURE.md";
/// Workspace members outside the product graph.
const NON_PRODUCT: [&str; 2] = ["xtask", "citysim"];

pub(crate) fn check(root: &Path) -> Result<Vec<Violation>, XtaskError> {
    let text = walk::read_text(&root.join(ARCH))?;
    let allowed = parse_block(&text)?;
    let seams = seam_files(&text);
    let mut violations = Vec::new();

    check_edges(root, &allowed, &mut violations)?;
    check_pub_traits(root, &seams, &mut violations)?;
    Ok(violations)
}

/// Parse the ```depmap fenced block: `name:` or `name: dep, dep`.
fn parse_block(text: &str) -> Result<BTreeMap<String, BTreeSet<String>>, XtaskError> {
    let mut map = BTreeMap::new();
    let mut inside = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if trimmed == "```depmap" {
            inside = true;
            continue;
        }
        if inside {
            if trimmed == "```" {
                return Ok(map);
            }
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            let (name, deps) = trimmed.split_once(':').ok_or_else(|| XtaskError::Doc {
                file: ARCH.to_owned(),
                msg: format!("depmap block line without a colon: {trimmed:?}"),
            })?;
            let set: BTreeSet<String> = deps
                .split(',')
                .map(str::trim)
                .filter(|d| !d.is_empty())
                .map(str::to_owned)
                .collect();
            map.insert(name.trim().to_owned(), set);
        }
    }
    Err(XtaskError::Doc {
        file: ARCH.to_owned(),
        msg: "no closed ```depmap fenced block found in section 2".to_owned(),
    })
}

/// Seam files: section 3 table rows with four data cells and a crates/ path.
fn seam_files(text: &str) -> BTreeSet<String> {
    let mut set = BTreeSet::new();
    for line in text.lines() {
        if !line.trim_start().starts_with('|') {
            continue;
        }
        let cells: Vec<&str> = line.split('|').map(str::trim).collect();
        if cells.len() != 6 {
            continue;
        }
        if let Some(path) = cells.get(2)
            && path.starts_with("crates/")
            && path.ends_with(".rs")
        {
            set.insert((*path).to_owned());
        }
    }
    set
}

fn check_edges(
    root: &Path,
    allowed: &BTreeMap<String, BTreeSet<String>>,
    violations: &mut Vec<Violation>,
) -> Result<(), XtaskError> {
    let metadata = cargo_metadata(root)?;
    let packages = metadata
        .get("packages")
        .and_then(serde_json::Value::as_array)
        .ok_or_else(|| XtaskError::Doc {
            file: "cargo metadata".to_owned(),
            msg: "no packages array".to_owned(),
        })?;

    let members: BTreeSet<String> = packages
        .iter()
        .filter_map(|p| p.get("name").and_then(serde_json::Value::as_str))
        .map(str::to_owned)
        .collect();

    for package in packages {
        let name = package
            .get("name")
            .and_then(serde_json::Value::as_str)
            .unwrap_or_default();
        if NON_PRODUCT.contains(&name) {
            continue;
        }
        let Some(allowed_deps) = allowed.get(name) else {
            violations.push(Violation {
                gate: "depmap",
                location: format!("crates/{name}"),
                rule: "every product crate is registered in the depmap block (section 2)"
                    .to_owned(),
                violation: "crate missing from the depmap block".to_owned(),
                alternative: "register the crate and its allowed edges (verdict required)"
                    .to_owned(),
            });
            continue;
        };
        let deps = package
            .get("dependencies")
            .and_then(serde_json::Value::as_array)
            .map(Vec::as_slice)
            .unwrap_or_default();
        for dep in deps {
            let dep_name = dep
                .get("name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            let kind = dep.get("kind").and_then(serde_json::Value::as_str);
            if kind == Some("dev") {
                continue; // tests may use citysim and friends freely
            }
            if members.contains(dep_name) && !allowed_deps.contains(dep_name) {
                violations.push(Violation {
                    gate: "depmap",
                    location: format!("crates/{name}/Cargo.toml"),
                    rule: "crate edges are a subset of the documented topology \
                           (section 2, C5); dependencies point inward only"
                        .to_owned(),
                    violation: format!("hidden edge {name} -> {dep_name}"),
                    alternative: "remove the dependency, or change the topology \
                                  first (a ruling, then the topology)"
                        .to_owned(),
                });
            }
        }
    }
    Ok(())
}

fn check_pub_traits(
    root: &Path,
    seams: &BTreeSet<String>,
    violations: &mut Vec<Violation>,
) -> Result<(), XtaskError> {
    for file in walk::files_with_ext(&root.join("crates"), &["rs"])? {
        let rel = walk::rel(root, &file);
        let text = walk::read_text(&file)?;
        for (index, line) in text.lines().enumerate() {
            if line.trim_start().starts_with("pub trait ") && !seams.contains(&rel) {
                violations.push(Violation {
                    gate: "depmap",
                    location: format!("{rel}:{}", index.saturating_add(1)),
                    rule: "pub trait only at registered seams (section 3; \
                           one adapter = hypothetical seam)"
                        .to_owned(),
                    violation: "pub trait outside the seam list".to_owned(),
                    alternative: "use pub(crate) trait, or register a real seam \
                                  with a second adapter (verdict required)"
                        .to_owned(),
                });
            }
        }
    }
    Ok(())
}

fn cargo_metadata(root: &Path) -> Result<serde_json::Value, XtaskError> {
    let output = Command::new("cargo")
        .args(["metadata", "--format-version", "1", "--no-deps"])
        .current_dir(root)
        .output()
        .map_err(|source| XtaskError::Io {
            path: "cargo metadata".to_owned(),
            source,
        })?;
    if !output.status.success() {
        return Err(XtaskError::Cmd {
            cmd: "cargo metadata".to_owned(),
            msg: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    serde_json::from_slice(&output.stdout).map_err(|err| XtaskError::Cmd {
        cmd: "cargo metadata".to_owned(),
        msg: err.to_string(),
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn block_parses_names_and_edges() {
        let text = "x\n```depmap\nkernel:\nruntime: kernel, memory\n```\ny\n";
        let map = parse_block(text).unwrap();
        assert!(map.get("kernel").unwrap().is_empty());
        assert_eq!(map.get("runtime").unwrap().len(), 2);
    }

    #[test]
    fn missing_block_is_a_doc_error() {
        assert!(parse_block("no block here").is_err());
    }

    #[test]
    fn seam_rows_are_four_cell_rows_with_paths() {
        let text = "| a | crates/kernel/src/ledger.rs | b | c |\n\
                    | kernel::gate | crates/kernel/src/gate.rs | x | 8.2 | S2 | 未建 |\n";
        let seams = seam_files(text);
        assert!(seams.contains("crates/kernel/src/ledger.rs"));
        assert_eq!(seams.len(), 1);
    }
}
