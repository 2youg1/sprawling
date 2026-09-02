// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Guard gate: the gates guard themselves. A commit that changes gate
//! machinery (xtask/, lint config, CI, the justfile) or deletes a module-table
//! row **in the same commit as the source those gates judge** must carry a
//! `Verdict:` trailer quoting the user's ruling — this closes the "loosen the
//! gate to pass the gate" shortcut: fix the cause, not the gate.
//!
//! **A commit whose whole diff is gate machinery is a re-pricing, and
//! re-pricing is ordinary work** (AGENTS.md, `guard` row). The gate used to
//! demand a ruling for every touch of `xtask/`, which made changing the price
//! of a rule as expensive as breaking one — and that width is the recorded
//! mechanical reason the no-JavaScript rule outlived its argument by a year
//! (`WORKSPACE/FRONTEND-METHOD.md` section 4). What is left is the one shape
//! nobody may take without a ruling: a gate loosened while carrying the work
//! it would otherwise have to pass.
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
/// What the gates judge, as opposed to how the gates decide.
///
/// A gate change travelling with the source it judges is the shortcut:
/// the work is in the commit, the rule that would have refused it is in
/// the same commit, and one green run reports both. A gate change
/// travelling alone is a re-pricing — visible as a diff that says
/// nothing except that a rule now costs something different, which is
/// what a reviewer needs to see and what a ruling would only hide behind
/// a signature.
///
/// **The hole this leaves is deliberate and recorded**: two commits, one
/// loosening and one passing, are not caught. Closing it means charging
/// a ruling for every re-pricing, and that price is what the rule this
/// gate serves was changed to stop paying.
const JUDGED_PREFIXES: [&str; 3] = ["crates/", "citysim/", "fuzz/"];
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
        let removes_a_row =
            files.iter().any(|f| f == "ARCHITECTURE.md") && deletes_module_row(root, sha)?;
        let gates = gate_faces(&files, removes_a_row);
        let judged = judged_faces(&files);
        let has_trailer = message
            .lines()
            .any(|line| line.trim_start().starts_with(TRAILER));
        if !gates.is_empty() && !judged.is_empty() && !has_trailer {
            let short: String = sha.chars().take(12).collect();
            violations.push(Violation {
                gate: "guard",
                location: format!("commit {short}"),
                rule: "a gate change travelling with the source it judges carries a user \
                       ruling: `Verdict:` trailer"
                    .to_owned(),
                violation: format!(
                    "changes {} alongside {} without a {TRAILER} trailer",
                    gates.join(", "),
                    judged.join(", ")
                ),
                alternative: "amend the commit message with `Verdict: <user ruling>`, or split \
                              the re-pricing into a commit of its own — a gate change that \
                              travels alone needs no ruling"
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

fn is_judged(file: &str) -> bool {
    JUDGED_PREFIXES.iter().any(|p| file.starts_with(p))
}

/// The gate machinery one commit changes, named so the refusal can say
/// which side of the pair it saw.
///
/// Pure over a file list, so the rule is asserted against paths rather
/// than against a repository with commits in it.
fn gate_faces(files: &[String], removes_a_module_row: bool) -> Vec<String> {
    let mut faces: Vec<String> = files.iter().filter(|f| is_protected(f)).cloned().collect();
    if removes_a_module_row {
        faces.push("ARCHITECTURE.md (module-table row deletion)".to_owned());
    }
    faces
}

/// The judged source one commit changes. Bounded at three, because a
/// refusal that lists eighty paths is a refusal nobody reads.
fn judged_faces(files: &[String]) -> Vec<String> {
    let mut faces: Vec<String> = files
        .iter()
        .filter(|f| is_judged(f))
        .take(3)
        .cloned()
        .collect();
    if files.iter().filter(|f| is_judged(f)).count() > faces.len() {
        faces.push("and others".to_owned());
    }
    faces
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
    use super::{gate_faces, is_protected, judged_faces, row_path};

    fn paths(list: &[&str]) -> Vec<String> {
        list.iter().map(|p| (*p).to_owned()).collect()
    }

    /// The shortcut this gate exists to close: the work and the rule
    /// that would have refused it, in one commit and one green run.
    #[test]
    fn a_gate_loosened_beside_the_work_it_judges_is_the_shape_that_needs_a_ruling() {
        let mixed = paths(&["clippy.toml", "crates/kernel/src/plan.rs"]);
        assert!(!gate_faces(&mixed, false).is_empty());
        assert!(!judged_faces(&mixed).is_empty());
    }

    /// Re-pricing a rule in a commit of its own is ordinary work
    /// (AGENTS.md, `guard` row). The old width charged a ruling for it,
    /// and that price is the recorded mechanical reason a rule outlived
    /// its argument.
    #[test]
    fn a_re_pricing_that_travels_alone_needs_no_ruling() {
        let alone = paths(&[
            "xtask/src/guard.rs",
            "xtask/xtask-SPEC.md",
            "AGENTS.md",
            "README.md",
        ]);
        assert!(!gate_faces(&alone, false).is_empty(), "still a gate change");
        assert!(
            judged_faces(&alone).is_empty(),
            "nothing the gates judge rides along"
        );
    }

    /// A card that only changes product source never meets this gate,
    /// whatever it does to the code.
    #[test]
    fn ordinary_source_work_is_not_a_gate_change() {
        let work = paths(&["crates/web/src/board.rs", "crates/web/screens/board.html"]);
        assert!(gate_faces(&work, false).is_empty());
    }

    /// What a gate produced is not how a gate decides: a public-surface
    /// change regenerates a baseline beside the source it describes, and
    /// the pair must not read as a loosening.
    #[test]
    fn a_regenerated_baseline_beside_its_own_source_is_not_a_gate_change() {
        let pair = paths(&[
            "xtask/api-baselines/kernel.txt",
            "crates/kernel/src/share.rs",
        ]);
        assert!(gate_faces(&pair, false).is_empty());
    }

    /// A refusal that lists eighty paths is a refusal nobody reads.
    #[test]
    fn the_judged_side_of_a_refusal_is_bounded() {
        let many = paths(&[
            "crates/a/src/one.rs",
            "crates/a/src/two.rs",
            "crates/a/src/three.rs",
            "crates/a/src/four.rs",
            "crates/a/src/five.rs",
        ]);
        let named = judged_faces(&many);
        assert_eq!(named.len(), 4);
        assert_eq!(named.last().map(String::as_str), Some("and others"));
    }

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
