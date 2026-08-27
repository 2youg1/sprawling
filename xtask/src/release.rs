// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! The published tree: what a reader outside this machine receives.
//!
//! The repository is the artefact. Every document that explains how this
//! is built - `AGENTS.md`, `ARCHITECTURE.md`, each crate's SPEC - ships
//! with the code it explains, because a reader who wants to change the
//! code needs exactly those. **The isolation zone is the one exception**:
//! `local/` holds one machine's handoffs, rulings and probes, it is
//! gitignored, and nothing published may depend on it.
//!
//! Three assertions, all about honesty rather than tidiness. Nothing in
//! the published tree may be an isolation-zone path, nothing in it may
//! link to one - a link that is dead for every reader but one is a
//! sentence written for a reader who does not exist - and nothing in it
//! may carry a path off the machine that built it.

use std::collections::BTreeSet;
use std::path::Path;

use crate::report::{Violation, XtaskError};
use crate::walk;

/// Path prefixes that stay behind when the tree is published. Closed,
/// and now one entry long.
///
/// It used to name the construction authorities too. They ship: a
/// private working note is one thing, and a document that says why the
/// code has this shape is another - withholding the second leaves a
/// reader with source and no reasons. What stays behind is only what
/// belongs to one machine.
const SCAFFOLDING: [&str; 1] = ["local/"];

/// The path shapes that name somebody's home directory, lower-cased.
///
/// Not every absolute path: `/tmp`, `/etc` and `C:/windows` are facts
/// about a kind of machine, and the tests that prove an absolute path is
/// refused have to write one. What may not ship is a path that names a
/// person - their account, their working directory, the layout of their
/// disk.
const HOME_SHAPES: [&str; 7] = [
    ":\\users\\",
    ":/users/",
    ":\\home\\",
    ":/home/",
    "/home/",
    "/users/",
    "/root/",
];

/// This file, which necessarily spells the shapes it looks for. The same
/// exemption the secret and colour gates carry, and for the same reason:
/// a detector cannot be written, or explained, without writing down what
/// it detects. The specification is on the list for the second reason.
const DETECTORS: [&str; 2] = ["xtask/src/release.rs", "xtask/xtask-SPEC.md"];

/// Whether a line carries a path that names somebody's home directory.
///
/// What this is for is the publication step: a tree that ships with the
/// author's account name in it has told every reader something about the
/// machine it was built on, and nobody decided to tell them.
///
/// Matched case-insensitively and reported with enough of the tail to
/// find it, but never the whole line - a violation message that quotes
/// the path in full would put it in the CI log too.
///
/// The match arrives as a byte offset and the report is taken in
/// characters, so the two are reconciled before the tail is cut: on a
/// line of Chinese prose the byte offset runs past the end of the
/// character sequence, and the report came out empty.
#[must_use]
pub(crate) fn machine_path(line: &str) -> Option<String> {
    let lowered = line.to_lowercase();
    let at = HOME_SHAPES
        .iter()
        .filter_map(|shape| lowered.find(shape))
        .min()?;
    let chars_before = lowered.char_indices().take_while(|(i, _)| *i < at).count();
    Some(lowered.chars().skip(chars_before).take(20).collect())
}

/// Whether a repo-relative path stays behind when the public tree is
/// generated.
///
/// Pure and total, because this is the whole classification: everything
/// the rule does not name is product surface, and a new scaffolding file
/// that nobody classified is therefore visible in the artefact rather
/// than silently dropped from it.
#[must_use]
pub(crate) fn is_scaffolding(rel: &str) -> bool {
    SCAFFOLDING.iter().any(|prefix| {
        // A directory prefix has to match the directory itself as well
        // as what is inside it: `docs/templates/` and `docs/templates`
        // name the same thing, and a link that used the second spelling
        // is exactly the one that would survive a check written only
        // for the first.
        let bare = prefix.trim_end_matches('/');
        rel == *prefix || rel == bare || rel.starts_with(prefix)
    })
}

/// The paths a public artefact would carry, in walk order.
pub(crate) fn published(root: &Path) -> Result<Vec<String>, XtaskError> {
    Ok(walk::files(root)?
        .iter()
        .map(|path| walk::rel(root, path))
        .filter(|rel| !is_scaffolding(rel))
        .collect())
}

/// Every markdown link target a file mentions, as written.
fn link_targets(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let bytes: Vec<char> = text.chars().collect();
    let mut index = 0usize;
    while index < bytes.len() {
        if bytes.get(index) == Some(&'(')
            && index > 0
            && bytes.get(index.saturating_sub(1)) == Some(&']')
        {
            let mut end = index.saturating_add(1);
            let mut target = String::new();
            while let Some(ch) = bytes.get(end) {
                if *ch == ')' {
                    break;
                }
                target.push(*ch);
                end = end.saturating_add(1);
            }
            if !target.is_empty() {
                out.push(target);
            }
            index = end;
        }
        index = index.saturating_add(1);
    }
    out
}

/// Checks that a filtered tree would stand on its own.
pub(crate) fn check(root: &Path) -> Result<Vec<Violation>, XtaskError> {
    let mut violations = Vec::new();
    let kept: BTreeSet<String> = published(root)?.into_iter().collect();
    for rel in &kept {
        let full = root.join(rel);
        // Anything that reads as text is checked for machine paths; a
        // hardcoded home directory in a source file or a manifest is
        // worse than one in prose, not better. Bytes that are not UTF-8
        // are fixtures, and they are skipped here rather than guessed at.
        let Ok(text) = std::fs::read_to_string(&full) else {
            continue;
        };
        for (number, line) in text.lines().enumerate() {
            if DETECTORS.contains(&rel.as_str()) {
                break;
            }
            if let Some(found) = machine_path(line) {
                violations.push(Violation {
                    gate: "release",
                    location: format!("{rel}:{}", number.saturating_add(1)),
                    rule: "a published file may not carry a path that exists only on the \
                           machine that wrote it"
                        .to_owned(),
                    violation: format!("names `{found}`"),
                    alternative: "write the path relative to the city or the repository, or \
                                  use a placeholder a reader can substitute"
                        .to_owned(),
                });
            }
        }
        if !rel.ends_with(".md") {
            continue;
        }
        // Naming the isolation zone in prose is the same failure as
        // linking to it: the sentence sends a reader to a directory that
        // exists on one machine. Checked on the whole text rather than
        // on links alone, because "see local/Handoff.md" without a link
        // is the version that is easiest to write and hardest to notice.
        if text.contains("local/") && !DETECTORS.contains(&rel.as_str()) {
            violations.push(Violation {
                gate: "release",
                location: rel.clone(),
                rule: "a published document may not send a reader into the isolation zone, \
                       which exists on one machine only"
                    .to_owned(),
                violation: "names `local/`".to_owned(),
                alternative: "say the thing itself here, or drop the sentence: a reader who \
                              cannot follow it is worse off than one who never saw it"
                    .to_owned(),
            });
        }
        for target in link_targets(&text) {
            // Only repository-relative links can dangle; a URL is
            // somebody else's problem and an anchor is this file's own.
            if target.starts_with("http") || target.starts_with('#') {
                continue;
            }
            let cleaned = target.split('#').next().unwrap_or(&target).to_owned();
            if cleaned.is_empty() {
                continue;
            }
            let pointed = resolve(rel, &cleaned);
            if is_scaffolding(&pointed) {
                violations.push(Violation {
                    gate: "release",
                    location: format!("{rel} -> {target}"),
                    rule: "a product document may not depend on scaffolding, because the \
                           link is dead in the public artefact"
                        .to_owned(),
                    violation: format!("{pointed} is filtered out of the public tree"),
                    alternative: "move the sentence into the product document, or drop it: a \
                                  link a reader cannot follow is worse than no link"
                        .to_owned(),
                });
            }
        }
    }
    Ok(violations)
}

/// A link target as a repo-relative path, resolved against the file that
/// carries it.
fn resolve(from: &str, target: &str) -> String {
    if target.starts_with('/') {
        return target.trim_start_matches('/').to_owned();
    }
    let mut parts: Vec<&str> = from.split('/').collect();
    parts.pop();
    for segment in target.split('/') {
        match segment {
            "." | "" => {}
            ".." => {
                parts.pop();
            }
            other => parts.push(other),
        }
    }
    parts.join("/")
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]
mod tests {
    use super::*;

    #[test]
    fn one_machine_stays_behind_and_everything_that_explains_the_code_goes_out() {
        for behind in ["local", "local/Handoff.md", "local/shots/01.png"] {
            assert!(is_scaffolding(behind), "{behind} belongs to one machine");
        }
        for out in [
            "README.md",
            "README.zh-CN.md",
            "AGENTS.md",
            "ARCHITECTURE.md",
            "CLAUDE.md",
            "LICENSE",
            "docs/glossary.md",
            "docs/third-party.md",
            "docs/templates/JOB.md",
            "crates/kernel/kernel-SPEC.md",
            "crates/kernel/src/lib.rs",
            "xtask/lexicon.toml",
            "fixtures/golden.jsonl",
        ] {
            assert!(!is_scaffolding(out), "{out} is published");
        }
        // `localise.rs` is not the isolation zone, and a prefix rule
        // written without care would say it is.
        assert!(!is_scaffolding("crates/web/src/localise.rs"));
    }

    #[test]
    fn anything_the_rule_does_not_name_is_published() {
        // The failure direction is deliberate. An unclassified file
        // shows up in the artefact, where somebody sees it; the other
        // way round it disappears, where nobody does.
        assert!(!is_scaffolding("docs/some-new-page.md"));
        assert!(!is_scaffolding("crates/kernel/src/brand_new.rs"));
    }

    #[test]
    fn links_are_read_as_written_and_resolved_against_their_file() {
        let text = "see [the plan](../local/Handoff.md) and [glossary](glossary.md#terms)";
        let targets = link_targets(text);
        assert_eq!(targets, ["../local/Handoff.md", "glossary.md#terms"]);
        assert_eq!(
            resolve("docs/CONTRIBUTING.md", "../local/Handoff.md"),
            "local/Handoff.md"
        );
        assert_eq!(
            resolve("docs/CONTRIBUTING.md", "glossary.md"),
            "docs/glossary.md"
        );
        assert_eq!(resolve("README.md", "docs/glossary.md"), "docs/glossary.md");
    }

    #[test]
    fn a_path_from_one_machine_is_caught_and_a_url_is_not() {
        // The false positive this shape invites is every URL in the
        // repository: `https://` ends in a letter, a colon and a slash.
        assert!(machine_path("see https://example.invalid/a/b").is_none());
        assert!(machine_path("run it in /tmp/city or /etc/hosts").is_none());
        assert!(machine_path("~/cities/first is fine").is_none());
        assert!(machine_path("nothing here").is_none());
        // A test that proves an absolute path is refused has to write
        // one, and `C:/windows` names a kind of machine rather than a
        // person. Flagging those would make the gate wrong about the
        // three files that assert the refusal.
        assert!(machine_path(r"C:\repo\crates").is_none());
        assert!(machine_path("C:/windows/system32").is_none());

        assert!(machine_path(r"C:\Users\someone\WORKSPACE\city").is_some());
        assert!(machine_path("d:/users/someone/work").is_some());
        assert!(machine_path("/home/someone/cities/first").is_some());
        assert!(machine_path("/Users/someone/cities").is_some());
        assert!(machine_path("stored under /root/city").is_some());
        // The report carries enough to find it and not the whole line.
        let found = machine_path(r"the log said C:\Users\someone\a\b\c\d\e").unwrap();
        assert!(found.chars().count() <= 20);
        assert!(!found.contains("the log said"));
        // A line of Chinese prose puts the byte offset well past the
        // character count; the report must still carry the path.
        let chinese = machine_path(r"日志里写的是 C:\Users\someone\city").unwrap();
        assert!(chinese.starts_with(":\\users\\"), "{chinese}");
    }

    #[test]
    fn a_bracket_that_is_not_a_link_is_not_a_target() {
        assert!(link_targets("an array [1, 2] (three)").is_empty());
        assert!(link_targets("nothing here at all").is_empty());
    }

    #[test]
    fn the_repository_itself_passes_the_check_it_ships() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(Path::to_path_buf)
            .expect("xtask lives one level under the repo root");
        let violations = check(&root).expect("the check runs");
        assert!(
            violations.is_empty(),
            "product documents link to scaffolding:\n{}",
            violations
                .iter()
                .map(|v| format!("{}: {}", v.location, v.violation))
                .collect::<Vec<String>>()
                .join("\n")
        );
    }
}
