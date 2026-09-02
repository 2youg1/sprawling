// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Wiring gate: every verb on the wire is reachable from the side that is
//! supposed to reach it, and no other side pretends it is
//! (channels-SPEC.md section 19).
//!
//! **Both directions have failed here, and they fail differently.**
//!
//! Drawing a control the city cannot perform is loud: somebody clicks it
//! and gets a refusal. v0.0.3 shipped three of those - `Takeover`,
//! `Rollback` and `CreatePolicy` were on the wire, drawn in the client,
//! and executable by nothing - and `assembly::not_built` already carried
//! the rule in its own rustdoc: "A verb answered here must not appear as
//! a control in the client." Nothing was reading it.
//!
//! The other direction is silent, and that is what makes it worse. A
//! capability the city can perform and no control reaches produces no
//! complaint, because a button that does not exist is a button nobody
//! fails to press. `Pursue` - the whole of "the city keeps working until
//! the work runs out" - and `SetAutonomy` were both built, tested, and
//! unreachable from a browser. That is the mechanical reason v0.0.3's
//! second completion criterion was never accepted: you could not turn it
//! on.
//!
//! **Three sources, no copies.** The variants come from the real `enum
//! Command` parsed out of whichever `channels` module declares it; whether the city
//! can perform one comes from the arms of `assembly::run_command`;
//! whether a person can ask for one comes from `crates/web/src`. The SPEC
//! contributes the one fact none of the three can state - which side is
//! *supposed* to reach it - and the gate reads it as data, so "the table
//! drifted" and "the enum grew silently" are the same red.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use crate::report::{Violation, XtaskError};
use crate::walk;

const SPEC: &str = "crates/channels/channels-SPEC.md";
const WIRE_DIR: &str = "crates/channels/src";
/// Where the assembly point lives. A directory rather than a file: the
/// gate wants the declaration of `run_command`, not its address, and
/// pinning the address meant that splitting the assembly point moved the
/// arms out from under the gate while leaving the gate green about it.
const WORKER_DIR: &str = "crates/sprawling/src";
const CLIENT: &str = "crates/web/src";

/// Which side is supposed to reach a verb.
///
/// Exhaustive rather than a pair of flags: a verb has exactly one way in,
/// and two booleans would make "drawn and pushed" spellable when it is
/// not a thing.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Reach {
    /// A person's verb. The client draws it and the city performs it.
    Client,
    /// An outside service pushes it. Nothing draws it.
    Push,
    /// The handshake consumes it before `run_command` sees it.
    Handshake,
    /// It has no byte form, so no client can spell it.
    Sealed,
}

impl Reach {
    fn parse(cell: &str) -> Option<Self> {
        match cell {
            "client" => Some(Self::Client),
            "push" => Some(Self::Push),
            "handshake" => Some(Self::Handshake),
            "sealed" => Some(Self::Sealed),
            _ => None,
        }
    }
}

fn violation(rule: &str, subject: String, alternative: &str) -> Violation {
    Violation {
        gate: "wiring",
        location: SPEC.to_owned(),
        rule: rule.to_owned(),
        violation: subject,
        alternative: alternative.to_owned(),
    }
}

/// The variants of an enum, read out of the source that declares it.
///
/// Parsed rather than pattern-matched line by line, for the reason
/// `length` gives: a brace is not always a block, and a gate that reads
/// its own input wrongly produces a list of offenders that is wrong in
/// every entry.
fn variants(root: &Path, enum_name: &str) -> Result<Vec<String>, XtaskError> {
    let dir = root.join(WIRE_DIR);
    let mut sources: Vec<std::path::PathBuf> = walk::files_with_ext(&dir, &["rs"])?;
    sources.sort();
    for source in sources {
        let text = walk::read_text(&source)?;
        let parsed = syn::parse_file(&text).map_err(|err| XtaskError::Doc {
            file: walk::rel(root, &source),
            msg: format!("this file does not parse as Rust: {err}"),
        })?;
        for item in &parsed.items {
            let syn::Item::Enum(declared) = item else {
                continue;
            };
            if declared.ident == enum_name {
                return Ok(declared
                    .variants
                    .iter()
                    .map(|found| found.ident.to_string())
                    .collect());
            }
        }
    }
    Err(XtaskError::Doc {
        file: WIRE_DIR.to_owned(),
        msg: format!("no module here declares `enum {enum_name}`"),
    })
}

/// What the SPEC's reach table says, variant by variant.
fn declared(root: &Path) -> Result<BTreeMap<String, Reach>, XtaskError> {
    let text = walk::read_text(&root.join(SPEC))?;
    let mut out = BTreeMap::new();
    for line in text.lines() {
        let Some(cells) = cells(line) else {
            continue;
        };
        let (Some(first), Some(second)) = (cells.first(), cells.get(1)) else {
            continue;
        };
        let Some(name) = first.strip_prefix('`').and_then(|c| c.strip_suffix('`')) else {
            continue;
        };
        // A row whose reach cell is not one of the four is not a reach
        // row: the same document holds other tables with a backticked
        // first column, and reading them would invent verbs.
        let Some(reach) = Reach::parse(second) else {
            continue;
        };
        out.insert(name.to_owned(), reach);
    }
    Ok(out)
}

fn cells(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    let inner = trimmed.strip_prefix('|')?.strip_suffix('|')?;
    Some(
        inner
            .split('|')
            .map(|cell| cell.trim().to_owned())
            .collect(),
    )
}

/// The verbs the city can actually carry out.
///
/// An arm answering with `not_built` is on the wire and has no executor,
/// which is exactly the state the client may not draw a control for.
fn performed(root: &Path, all: &[String]) -> Result<BTreeSet<String>, XtaskError> {
    let mut found = None;
    for file in walk::files_with_ext(&root.join(WORKER_DIR), &["rs"])? {
        let text = walk::read_text(&file)?;
        if let Some(start) = text.find("fn run_command") {
            found = Some(text.get(start..).unwrap_or_default().to_owned());
            break;
        }
    }
    let Some(body) = found else {
        return Err(XtaskError::Doc {
            file: WORKER_DIR.to_owned(),
            msg: "no `run_command` under here to read the arms of".to_owned(),
        });
    };
    let body = body.as_str();
    let mut out = BTreeSet::new();
    for name in all {
        let arm = format!("channels::Command::{name}");
        let Some(at) = body.find(&arm) else {
            continue;
        };
        // The arm runs to the next one; `not_built` inside that slice is
        // this verb's answer rather than a neighbour's.
        let rest = body.get(at.saturating_add(arm.len())..).unwrap_or_default();
        let end = rest
            .find("\n            channels::Command::")
            .unwrap_or(rest.len());
        if !rest.get(..end).unwrap_or_default().contains("not_built") {
            out.insert(name.clone());
        }
    }
    Ok(out)
}

/// The verbs a person can ask for, read from the client that draws them.
fn emitted(root: &Path, all: &[String]) -> Result<BTreeSet<String>, XtaskError> {
    let mut seen = BTreeSet::new();
    let base = root.join(CLIENT);
    if !base.exists() {
        return Ok(seen);
    }
    for file in walk::files_with_ext(&base, &["rs"])? {
        let text = walk::read_text(&file)?;
        for name in all {
            if text.contains(&format!("Command::{name}")) {
                seen.insert(name.clone());
            }
        }
    }
    Ok(seen)
}

pub(crate) fn check(root: &Path) -> Result<Vec<Violation>, XtaskError> {
    let all = variants(root, "Command")?;
    let table = declared(root)?;
    let performed = performed(root, &all)?;
    let emitted = emitted(root, &all)?;
    let mut violations = Vec::new();

    for name in &all {
        let Some(reach) = table.get(name) else {
            violations.push(violation(
                "every Command states which side reaches it",
                format!("`{name}` is on the wire and absent from the reach table"),
                "add a row to channels-SPEC.md section 19-2: a verb nobody classified is a \
                 verb nobody decided to expose",
            ));
            continue;
        };
        let drawn = emitted.contains(name);
        let built = performed.contains(name);
        match reach {
            Reach::Client if drawn && !built => violations.push(violation(
                "a control the client draws is a verb the city can carry out",
                format!("`{name}` is drawn and answered with `not_built`"),
                "take the control out until the executor exists: a refusal is what the city \
                 owes a peer that asks anyway, not a substitute for the button being gone",
            )),
            Reach::Client if !drawn && built => violations.push(violation(
                "a verb the city can carry out is reachable from the client",
                format!("`{name}` is built and no control reaches it"),
                "draw the control, or change its reach in channels-SPEC.md section 19-2 and \
                 say why a person may not ask for it. A capability nobody can reach draws no \
                 complaint, because nobody fails to press a button that is not there",
            )),
            Reach::Sealed if drawn => violations.push(violation(
                "a sealed verb has no byte form, so no client may spell it",
                format!("`{name}` is sealed and the client names it"),
                "a credential reaches the vault in process; entering one remotely is meant to \
                 be unspellable",
            )),
            Reach::Push | Reach::Handshake if drawn => violations.push(violation(
                "a verb that arrives from outside is not a control",
                format!("`{name}` arrives by push or handshake and the client draws it"),
                "remove the control, or reclassify it in channels-SPEC.md section 19-2",
            )),
            _ => {}
        }
    }

    let known: BTreeSet<&String> = all.iter().collect();
    for named in table.keys().filter(|name| !known.contains(name)) {
        violations.push(violation(
            "the reach table names verbs that are on the wire",
            format!("`{named}` is classified and is not a Command"),
            "delete the row: it survived the verb it described",
        ));
    }
    Ok(violations)
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

    fn root() -> std::path::PathBuf {
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(Path::to_path_buf)
            .expect("xtask lives one level under the repo root")
    }

    /// The three sources are read from the real thing, so this fails the
    /// day one of them moves rather than the day somebody notices.
    #[test]
    fn the_wire_the_worker_and_the_client_are_all_readable() {
        let root = root();
        let all = variants(&root, "Command").unwrap();
        assert!(all.len() > 20, "the wire declares {} verbs", all.len());
        assert!(all.iter().any(|name| name == "Dispatch"));
        let performed = performed(&root, &all).unwrap();
        assert!(performed.contains("Dispatch"), "dispatch has an executor");
        assert!(
            !performed.contains("Takeover"),
            "takeover is answered with not_built"
        );
        let emitted = emitted(&root, &all).unwrap();
        assert!(emitted.contains("Dispatch"), "the client draws dispatch");
    }

    /// Every verb is classified and every classification names a verb.
    #[test]
    fn the_reach_table_and_the_enum_are_the_same_list() {
        let root = root();
        let all: BTreeSet<String> = variants(&root, "Command").unwrap().into_iter().collect();
        let table: BTreeSet<String> = declared(&root).unwrap().into_keys().collect();
        assert_eq!(
            all, table,
            "the enum and channels-SPEC section 19-2 disagree"
        );
    }

    #[test]
    fn a_reach_cell_reads_as_itself_and_nothing_else() {
        assert!(Reach::parse("client").is_some());
        assert!(Reach::parse("push").is_some());
        assert!(Reach::parse("handshake").is_some());
        assert!(Reach::parse("sealed").is_some());
        // Another table in the same document, with a backticked first
        // column, must not be read as verbs.
        assert!(Reach::parse("value").is_none());
        assert!(Reach::parse("").is_none());
    }

    #[test]
    fn the_repository_itself_passes_the_check_it_ships() {
        let violations = check(&root()).expect("the check runs");
        assert!(
            violations.is_empty(),
            "verbs are wired to the wrong side:\n{}",
            violations
                .iter()
                .map(|found| found.violation.clone())
                .collect::<Vec<String>>()
                .join("\n")
        );
    }
}
