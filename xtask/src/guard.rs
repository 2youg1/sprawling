// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Guard gate: the gates guard themselves. A commit that touches gate
//! machinery (xtask/, lint config, CI, the justfile) or deletes a module-table
//! row must carry a `Verdict:` trailer quoting the user's ruling — this closes
//! the "loosen the gate to pass the gate" shortcut: fix the cause, not the
//! gate.
//!
//! Scope: committed history only. The default is HEAD alone; CI passes
//! `--range`, which is `base..head` for a pull request and `before..after`
//! for a push, so a commit in the middle of a change-set is judged too and
//! not just the tip. Uncommitted edits are judged when they become commits
//! — gates testify about decidable objects only (xtask-SPEC.md section 3).

use std::path::Path;
use std::process::Command;

use crate::report::{Violation, XtaskError};

const PROTECTED_PREFIXES: [&str; 2] = ["xtask/", ".github/"];
const PROTECTED_FILES: [&str; 5] = [
    "deny.toml",
    "Cargo.toml",
    "rust-toolchain.toml",
    "clippy.toml",
    "justfile",
];
/// What a gate *produced*, as opposed to how a gate *decides*.
///
/// A public-surface baseline is the recorded output of `apisync`, and
/// `apisync` refuses a crate whose baseline is stale while naming
/// `cargo xtask apisync --write` as the way to fix it. With this
/// directory guarded, obeying one gate broke the other and every change
/// to any public surface would need a separate ruling - which teaches
/// people that a `Verdict:` trailer is a formality rather than a
/// decision. Regenerating a baseline cannot loosen anything: `apisync`
/// still compares it against the live API, and the diff is in the
/// commit for a reviewer to read.
const PRODUCED_PREFIXES: [&str; 1] = ["xtask/api-baselines/"];
const TRAILER: &str = "Verdict:";

pub(crate) fn check(root: &Path, range: Option<&str>) -> Result<Vec<Violation>, XtaskError> {
    let commits = match range {
        Some(spec) => git_lines(root, &["rev-list", spec])?,
        None => match git_lines(root, &["rev-parse", "--verify", "HEAD"]) {
            Ok(lines) => lines,
            Err(_) => {
                println!("gate guard: no commits yet, nothing to judge");
                return Ok(Vec::new());
            }
        },
    };

    let mut violations = Vec::new();
    for sha in &commits {
        let files = changed_paths(root, sha)?;
        let message = git_text(root, &["show", "-s", "--format=%B", sha])?;
        let mut hits: Vec<String> = files.iter().filter(|f| is_protected(f)).cloned().collect();
        if files.iter().any(|f| f == "ARCHITECTURE.md") && deletes_module_row(root, sha)? {
            hits.push("ARCHITECTURE.md (module-table row deletion)".to_owned());
        }
        let has_trailer = message
            .lines()
            .any(|line| line.trim_start().starts_with(TRAILER));
        if !hits.is_empty() && !has_trailer {
            let short: String = sha.chars().take(12).collect();
            violations.push(Violation {
                gate: "guard",
                location: format!("commit {short}"),
                rule: "gate changes carry a user ruling: `Verdict:` trailer".to_owned(),
                violation: format!("touches {} without a {TRAILER} trailer", hits.join(", ")),
                alternative: "amend the commit message with `Verdict: <user ruling>`, \
                              or move the change out of gate machinery"
                    .to_owned(),
            });
        }
    }
    Ok(violations)
}

fn is_protected(file: &str) -> bool {
    if PRODUCED_PREFIXES.iter().any(|p| file.starts_with(p)) {
        return false;
    }
    PROTECTED_FILES.contains(&file) || PROTECTED_PREFIXES.iter().any(|p| file.starts_with(p))
}

/// A module row counts as *removed* only when its `crates/**.rs` path appears
/// in a deleted diff line and in no added line. A status flip shows up as
/// delete-plus-add of the same path — the most common legal edit — and must
/// not demand a ruling (xtask-SPEC.md section 10-4).
fn deletes_module_row(root: &Path, sha: &str) -> Result<bool, XtaskError> {
    let diff = git_text(root, &["show", "--format=", sha, "--", "ARCHITECTURE.md"])?;
    let mut removed: Vec<String> = Vec::new();
    let mut added: Vec<String> = Vec::new();
    for line in diff.lines() {
        if let Some(rest) = line.strip_prefix('-')
            && let Some(path) = row_path(rest)
        {
            removed.push(path);
        } else if let Some(rest) = line.strip_prefix('+')
            && let Some(path) = row_path(rest)
        {
            added.push(path);
        }
    }
    Ok(removed.iter().any(|path| !added.contains(path)))
}

/// Extract the module-row path (cell 2) from a table line, if it is one.
fn row_path(line: &str) -> Option<String> {
    let mut cells = line.split('|').map(str::trim);
    let _leading = cells.next()?;
    let first = cells.next()?;
    let second = cells.next()?;
    if first.contains("::") && second.starts_with("crates/") && second.ends_with(".rs") {
        Some(second.to_owned())
    } else {
        None
    }
}

/// The paths one commit touches (added, removed or edited).
pub(crate) fn changed_paths(root: &Path, sha: &str) -> Result<Vec<String>, XtaskError> {
    git_lines(
        root,
        &[
            "diff-tree",
            "--no-commit-id",
            "--name-only",
            "-r",
            "--root",
            sha,
        ],
    )
}

/// Paths with their one-letter status (A/M/D/R...) for gates that must
/// tell creation apart from modification.
pub(crate) fn changed_paths_with_status(
    root: &Path,
    sha: &str,
) -> Result<Vec<(char, String)>, XtaskError> {
    let lines = git_lines(
        root,
        &[
            "diff-tree",
            "--no-commit-id",
            "--name-status",
            "-r",
            "--root",
            sha,
        ],
    )?;
    Ok(lines
        .iter()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let status = parts.next()?.chars().next()?;
            let path = parts.next_back()?.to_owned();
            Some((status, path))
        })
        .collect())
}

fn git_text(root: &Path, args: &[&str]) -> Result<String, XtaskError> {
    let output = Command::new("git")
        .args(args)
        .current_dir(root)
        .output()
        .map_err(|source| XtaskError::Io {
            path: format!("git {}", args.join(" ")),
            source,
        })?;
    if !output.status.success() {
        return Err(XtaskError::Cmd {
            cmd: format!("git {}", args.join(" ")),
            msg: String::from_utf8_lossy(&output.stderr).into_owned(),
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

pub(crate) fn git_lines(root: &Path, args: &[&str]) -> Result<Vec<String>, XtaskError> {
    Ok(git_text(root, args)?
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::{is_protected, row_path};

    #[test]
    fn status_flip_is_not_a_row_removal() {
        // A flipped status deletes and re-adds the same path.
        let removed =
            row_path("| kernel::gate | crates/kernel/src/gate.rs | x | 8.2 | S2 | 未建 |");
        let added = row_path("| kernel::gate | crates/kernel/src/gate.rs | x | 8.2 | S2 | 已建 |");
        assert_eq!(removed, added);
        assert!(removed.is_some());
        // Prose mentioning crates/ is not a row.
        assert_eq!(row_path(" see crates/kernel/src/gate.rs "), None);
    }

    #[test]
    fn protection_matches_roots_and_prefixes_only() {
        assert!(is_protected("deny.toml"));
        assert!(is_protected("Cargo.toml"));
        assert!(is_protected("xtask/src/guard.rs"));
        assert!(is_protected(".github/workflows/ci.yml"));
        // member manifests are not the root manifest
        assert!(!is_protected("crates/kernel/Cargo.toml"));
        assert!(!is_protected("ARCHITECTURE.md"));
        // What a gate produced is not how a gate decides. `apisync`
        // orders this file to be regenerated whenever a public surface
        // moves; guarding it would make the two gates contradict, and a
        // ruling demanded on every public-surface change is a ruling
        // nobody reads.
        assert!(!is_protected("xtask/api-baselines/channels.txt"));
        assert!(
            is_protected("xtask/src/apisync.rs"),
            "the gate's own machinery stays guarded"
        );
    }
}
