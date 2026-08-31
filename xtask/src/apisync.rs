// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Gate: a public API change must pass through the crate's SPEC in the
//! same change-set. Two assertions chain it: (1) the committed baseline
//! equals the live `cargo public-api` surface, so an API change forces a
//! baseline edit; (2) a baseline edit in a commit requires the crate's
//! SPEC touched in that commit. Width is not judged — that needs a
//! human; only "interface changed => documentation moved" is machinable.

use std::path::{Path, PathBuf};
use std::process::Command;

use crate::report::{Violation, XtaskError};
use crate::{guard, walk};

const BASELINE_DIR: &str = "xtask/api-baselines";

/// The sync contract covers exactly the lib crates that have a SPEC —
/// SPEC-first means the SPEC exists before the surface does; a bin crate
/// has no public API to baseline (its SPEC still gates via assertion 2's
/// path discipline when it grows one).
fn spec_crates(root: &Path) -> Result<Vec<String>, XtaskError> {
    let crates_dir = root.join("crates");
    let entries = std::fs::read_dir(&crates_dir).map_err(|source| XtaskError::Io {
        path: crates_dir.display().to_string(),
        source,
    })?;
    let mut found = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|source| XtaskError::Io {
            path: crates_dir.display().to_string(),
            source,
        })?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let has_spec = entry.path().join(format!("{name}-SPEC.md")).is_file();
        let is_lib = entry.path().join("src").join("lib.rs").is_file();
        if has_spec && is_lib {
            found.push(name);
        }
    }
    found.sort();
    Ok(found)
}

/// The live surface, normalized to trimmed non-empty lines. Derived and
/// blanket impls are omitted (-sss): they move with the toolchain, not
/// with our decisions.
fn live_api(root: &Path, krate: &str) -> Result<String, XtaskError> {
    let output = Command::new("cargo")
        .args([
            "public-api",
            "-p",
            krate,
            "--simplified",
            "--omit",
            "blanket-impls,auto-trait-impls,auto-derived-impls",
        ])
        .current_dir(root)
        .output()
        .map_err(|err| XtaskError::Cmd {
            cmd: "cargo public-api".to_owned(),
            msg: format!(
                "{err}; install the environment prerequisites: \
                 `cargo install cargo-public-api --locked` and \
                 `rustup toolchain install nightly`"
            ),
        })?;
    if !output.status.success() {
        return Err(XtaskError::Cmd {
            cmd: format!("cargo public-api -p {krate}"),
            msg: String::from_utf8_lossy(&output.stderr)
                .lines()
                .last()
                .unwrap_or("failed")
                .to_owned(),
        });
    }
    Ok(normalize(&String::from_utf8_lossy(&output.stdout)))
}

fn normalize(raw: &str) -> String {
    let mut lines: Vec<&str> = raw
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty())
        .collect();
    // The tool's order is deterministic, but sorting makes the baseline
    // immune to ordering changes across tool versions.
    lines.sort_unstable();
    let mut joined = lines.join("\n");
    joined.push('\n');
    joined
}

fn baseline_path(root: &Path, krate: &str) -> PathBuf {
    root.join(BASELINE_DIR).join(format!("{krate}.txt"))
}

pub(crate) fn check(root: &Path, range: Option<&str>) -> Result<Vec<Violation>, XtaskError> {
    let mut violations = Vec::new();
    let crates = spec_crates(root)?;

    // (1) Baseline freshness: live surface == committed baseline.
    for krate in &crates {
        let path = baseline_path(root, krate);
        let rel = walk::rel(root, &path);
        let Ok(committed) = std::fs::read_to_string(&path) else {
            violations.push(Violation {
                gate: "apisync",
                location: rel,
                rule: "every SPEC-bearing crate carries a public-api baseline".to_owned(),
                violation: format!("no baseline for `{krate}`"),
                alternative: "run `cargo xtask apisync --write` and commit the baseline \
                              together with the SPEC"
                    .to_owned(),
            });
            continue;
        };
        let live = live_api(root, krate)?;
        if normalize(&committed) != live {
            violations.push(Violation {
                gate: "apisync",
                location: rel,
                rule: "the committed baseline equals the live public API".to_owned(),
                violation: format!("`{krate}` public API drifted from its baseline"),
                alternative: "run `cargo xtask apisync --write`, review the diff, and \
                              touch the crate SPEC in the same change-set"
                    .to_owned(),
            });
        }
    }

    // (2) Same-change-set discipline: baseline edits require SPEC edits.
    let commits = match range {
        Some(spec) => guard::git_lines(root, &["rev-list", spec])?,
        // An unborn repository has nothing to judge.
        None => guard::git_lines(root, &["rev-list", "-1", "HEAD"]).unwrap_or_default(),
    };
    for commit in &commits {
        let changed = guard::changed_paths_with_status(root, commit)?;
        for krate in &crates {
            let baseline_rel = format!("{BASELINE_DIR}/{krate}.txt");
            let spec_rel = format!("crates/{krate}/{krate}-SPEC.md");
            // Creation ('A') is the sync point being established, not an
            // interface change; only a modified baseline demands the SPEC.
            let baseline_modified = changed
                .iter()
                .any(|(status, p)| p == &baseline_rel && *status != 'A');
            if baseline_modified && !changed.iter().any(|(_, p)| p == &spec_rel) {
                violations.push(Violation {
                    gate: "apisync",
                    location: format!("commit {commit}"),
                    rule: "a public API change passes through the crate SPEC (C8 discipline)"
                        .to_owned(),
                    violation: format!(
                        "baseline `{baseline_rel}` changed without touching `{spec_rel}`"
                    ),
                    alternative: "amend the commit to update the SPEC alongside the \
                                  interface, or revert the interface change"
                        .to_owned(),
                });
            }
        }
    }
    Ok(violations)
}

/// `cargo xtask apisync --write`: regenerate every baseline. The only
/// files this gate ever writes.
pub(crate) fn write(root: &Path) -> Result<(), XtaskError> {
    let dir = root.join(BASELINE_DIR);
    std::fs::create_dir_all(&dir).map_err(|source| XtaskError::Io {
        path: dir.display().to_string(),
        source,
    })?;
    for krate in spec_crates(root)? {
        let live = live_api(root, &krate)?;
        let path = baseline_path(root, &krate);
        std::fs::write(&path, live).map_err(|source| XtaskError::Io {
            path: path.display().to_string(),
            source,
        })?;
        println!("baseline written: {}", walk::rel(root, &path));
    }
    Ok(())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, reason = "test code")]
mod tests {
    use super::normalize;

    #[test]
    fn normalization_sorts_trims_and_ends_with_one_newline() {
        let raw = "pub fn b()\n\npub fn a()   \n";
        assert_eq!(normalize(raw), "pub fn a()\npub fn b()\n");
        assert_eq!(normalize(""), "\n");
    }
}
