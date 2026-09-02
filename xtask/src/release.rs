// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The published tree: what a reader outside this machine receives.
//!
//! The repository is the artefact. Every document that explains how this
//! is built - `AGENTS.md`, `ARCHITECTURE.md`, each crate's SPEC - ships
//! with the code it explains, because a reader who wants to change the
//! code needs exactly those. **The isolation zone is the one exception**:
//! `local/` holds one machine's handoffs, rulings and probes, it is
//! gitignored, and nothing published may depend on it.
//!
//! Four assertions, all about honesty rather than tidiness. Nothing in
//! the published tree may be an isolation-zone path, nothing in it may
//! link to one - a link that is dead for every reader but one is a
//! sentence written for a reader who does not exist - nothing in it may
//! carry a path off the machine that built it, and nothing in it may
//! cite a document this tree does not contain.
//!
//! The fourth assertion exists because the third one missed a real case.
//! Six files - two settled screens, two SPECs and a gate's own rustdoc -
//! cited `WORKSPACE/FRONTEND-METHOD.md`, the directory this repository
//! happened to sit in on one machine. It is not a home path, so the
//! third assertion said nothing, and it is not a markdown link, so the
//! second said nothing either. A contributor who cloned this tree could
//! not read the document two gates named as their own justification.
//!
//! **A citation of a machine-local file is testimony that one of the two
//! is misfiled**, and the gate cannot decide which. Either the cited
//! document is an authority, and withholding it leaves a reader with
//! source and no reasons; or the citing text is one machine's working
//! note wearing a product file's name, and it belongs in the isolation
//! zone. The second reading is the one that gets missed, so the report
//! offers it first. It is not always available: the file that raised
//! this was `xtask/src/guard.rs`, a gate CI runs on every push, which
//! cannot move anywhere. There the testimony convicted the other side,
//! and the document it named now ships as `docs/frontend-method.md`.

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

/// The extensions that mark a token as a document a reader is told to
/// open, rather than a word with a slash in it.
///
/// Closed on purpose. A path with no extension is a directory, a command
/// or a placeholder, and guessing which of those a reader is meant to
/// follow would make the gate wrong about prose it has no business
/// judging.
const CITED_EXTENSIONS: [&str; 7] = [".md", ".rs", ".toml", ".html", ".css", ".json", ".jsonl"];

/// Every directory name the published tree contains, at any depth.
///
/// Depth is deliberately discarded. A SPEC that writes `tools/exec.rs`
/// means `crates/runtime/src/tools/exec.rs`, and shorthand relative to
/// the crate being specified is how every SPEC in this repository is
/// written; a rule that demanded full paths would be a style opinion
/// wearing a gate's clothes. What the set is for is the other case: a
/// first segment that names no directory here at all cannot be
/// shorthand for anything, because there is nothing for it to be short
/// of.
fn directory_names(published: &[String]) -> BTreeSet<String> {
    let mut names = BTreeSet::new();
    for rel in published {
        let mut parts: Vec<&str> = rel.split('/').collect();
        parts.pop();
        for part in parts {
            names.insert(part.to_owned());
        }
    }
    // Two directories this tree has that its own file list cannot show.
    // `local/` is filtered out of `published` and already has an
    // assertion with a better sentence, so reporting it here would put
    // the weaker one in front of the reader; `target/` is where the
    // build writes, and a document that points at a generated file is
    // pointing at something every clone produces for itself.
    names.insert("local".to_owned());
    names.insert("target".to_owned());
    names
}

/// The maximal runs of path characters in a line.
///
/// A colon is not a path character, so `https://example.invalid/a.md`
/// arrives here as `https` and `//example.invalid/a.md`; the second has
/// an empty first segment and is dropped by the caller. That is the
/// whole of the URL handling, and it is a consequence of the character
/// set rather than a special case bolted beside it.
fn path_tokens(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut current = String::new();
    for ch in line.chars() {
        if ch.is_ascii_alphanumeric() || matches!(ch, '_' | '.' | '-' | '/') {
            current.push(ch);
        } else if !current.is_empty() {
            out.push(std::mem::take(&mut current));
        }
    }
    if !current.is_empty() {
        out.push(current);
    }
    out
}

/// A document this line tells a reader to open, which this tree does not
/// have a directory for.
///
/// Reported rather than resolved: the gate does not claim the file is
/// missing, only that the path is anchored somewhere this repository is
/// not. Those are different failures and the second is the one a clone
/// suffers from.
#[must_use]
pub(crate) fn outside_the_tree(line: &str, dirs: &BTreeSet<String>) -> Option<String> {
    path_tokens(line).into_iter().find(|token| {
        let trimmed = token.trim_end_matches(['.', '-']);
        if !CITED_EXTENSIONS.iter().any(|ext| trimmed.ends_with(ext)) {
            return false;
        }
        let Some((first, _)) = trimmed.split_once('/') else {
            return false;
        };
        // A segment that does not begin with a letter is not a name this
        // tree could have anchored: `../assets/app.css` walks up from
        // the file that carries it and is resolved by the link
        // assertion, and `.sprawling/BUILDING.md` is a path inside a
        // city rather than inside this repository.
        if !first.starts_with(|ch: char| ch.is_ascii_alphabetic()) {
            return false;
        }
        !dirs.contains(first)
    })
}

/// Whether a line of a source file is prose rather than code.
///
/// Only prose is read for cited documents. In code, a string that looks
/// like a path is usually an address inside a city - `lab/room1/notes.md`
/// names a wall in a test fixture, not a file in this repository - and
/// there are seventy-nine of those. Reading them would produce a list of
/// offenders that is wrong in every entry, which is worse than not
/// looking.
fn is_prose(rel: &str, line: &str) -> bool {
    if rel.ends_with(".md") || rel.ends_with(".html") {
        return true;
    }
    rel.ends_with(".rs") && line.trim_start().starts_with("//")
}

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
    let listed = published(root)?;
    let dirs = directory_names(&listed);
    let kept: BTreeSet<String> = listed.into_iter().collect();
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
            if is_prose(rel, line)
                && let Some(cited) = outside_the_tree(line, &dirs)
            {
                violations.push(Violation {
                    gate: "release",
                    location: format!("{rel}:{}", number.saturating_add(1)),
                    rule: "a published document may not cite a path anchored outside this \
                           repository"
                        .to_owned(),
                    violation: format!("`{cited}` starts at a directory this tree does not have"),
                    alternative: "decide which of the two is misfiled: move this file to \
                                  local/ if it is one machine's working note, bring the \
                                  cited document into the tree if it is an authority, or \
                                  write down here what it says. A reader who clones this \
                                  repository has nowhere else to look"
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
    fn a_document_anchored_outside_the_repository_is_caught_and_shorthand_is_not() {
        let dirs: BTreeSet<String> = ["crates", "docs", "src", "tools", "xtask", "local", "target"]
            .into_iter()
            .map(str::to_owned)
            .collect();

        // The case that happened. Six files cited this; no other
        // assertion in this gate said anything about it.
        assert_eq!(
            outside_the_tree("see `WORKSPACE/FRONTEND-METHOD.md` section 4", &dirs).as_deref(),
            Some("WORKSPACE/FRONTEND-METHOD.md")
        );
        assert!(outside_the_tree("WORKSPACE/sprawling-ui/UX-DECISIONS.md", &dirs).is_some());

        // Crate-relative shorthand is how every SPEC here is written.
        // Flagging it would send eleven people to rewrite prose that was
        // never wrong.
        assert!(outside_the_tree("// tools/exec.rs", &dirs).is_none());
        assert!(outside_the_tree("`read src/lex.rs` differs by", &dirs).is_none());
        assert!(outside_the_tree("crates/web/src/board.rs", &dirs).is_none());
        assert!(outside_the_tree("see local/Handoff.md", &dirs).is_none());

        // A URL is somebody else's tree. The colon ends the token, so
        // what reaches the test has an empty first segment.
        assert!(outside_the_tree("https://example.invalid/a/b.md", &dirs).is_none());
        // A relative walk upwards is the link assertion's business, and
        // a path inside a city is not a path inside this repository.
        assert!(outside_the_tree("[a](../../docs/frontend-method.md)", &dirs).is_none());
        assert!(outside_the_tree("edit `.sprawling/BUILDING.md`", &dirs).is_none());
        // The build directory is not published and still exists.
        assert!(outside_the_tree("written to target/screens/tokens.css", &dirs).is_none());
        // A word with no extension is not a citation.
        assert!(outside_the_tree("and/or, either/or", &dirs).is_none());
        // Trailing punctuation must not hide the extension.
        assert!(outside_the_tree("open WORKSPACE/notes.md.", &dirs).is_some());
    }

    #[test]
    fn only_prose_is_read_for_citations() {
        // A city address in a fixture is not a path in this repository,
        // and there are seventy-nine of them.
        assert!(!is_prose(
            "crates/city/src/building.rs",
            "    assert!(a(\"lab/x.md\"));"
        ));
        assert!(is_prose(
            "crates/city/src/building.rs",
            "//! see docs/glossary.md"
        ));
        assert!(is_prose("crates/web/web-SPEC.md", "anything at all"));
        assert!(is_prose(
            "crates/web/screens/board.html",
            "<!-- a comment -->"
        ));
    }

    #[test]
    fn directory_names_carry_no_depth() {
        let names = directory_names(&[
            "crates/runtime/src/tools/exec.rs".to_owned(),
            "README.md".to_owned(),
        ]);
        for present in ["crates", "runtime", "src", "tools", "local"] {
            assert!(names.contains(present), "{present} is a directory here");
        }
        assert!(!names.contains("WORKSPACE"));
        // The file itself is not a directory.
        assert!(!names.contains("exec.rs"));
        assert!(!names.contains("README.md"));
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
