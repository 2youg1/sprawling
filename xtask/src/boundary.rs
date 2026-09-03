// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Boundary gate: which side of the process boundary a check stands on
//! decides the language it is written in.
//!
//! **White box is Rust, black box is Haskell** (the person's ruling,
//! 2026-09-05). A check that links these crates and enters by their
//! public faces is written in Rust beside the code it judges. A check
//! that reaches the product the way a stranger does - spawning the
//! binary, opening a socket to a served city, speaking the wire from
//! outside - is written in Haskell under `adversary/`.
//!
//! **The parameter that makes this a rule rather than a preference** is
//! what the two kinds of check are able to say. A Rust test shares the
//! product's types, so it inherits every assumption those types encode
//! and cannot testify about a client that does not. `adversary/` is a
//! third client, out of tree, in a language with none of this
//! repository's vocabulary, and what it buys is quantification over
//! traces rather than one trace somebody thought of
//! (ARCHITECTURE.md section 11, V10). A subprocess test written in Rust
//! is the worst of both: it pays the cost of a process boundary and
//! still inherits the assumptions, and it reports as a unit test.
//!
//! **What is measured is direction, not mechanism.** The first cut of
//! this gate looked for sockets and subprocesses in test code, and it
//! convicted `assembly::driving` three times over: that test stands up a
//! fake model provider and speaks HTTP to it, which is the test playing
//! **the outside world** rather than consuming this product. Three
//! waivers in one file is a gate saying its predicate is wrong, and the
//! honest reading is that `TcpStream::connect` says a socket exists
//! without saying which way it points.
//!
//! So the tokens below name the surfaces **this product raises** - its
//! binary and its server. A check reaching one of those is standing
//! outside, whatever else it does; a check that stands up a double and
//! talks to that is inside, however many sockets it opens. Under this
//! reading the whole tree needs no waiver at all.
//!
//! Scope is test code only. `runtime::tools::exec` spawns processes for
//! a living, `bin::wire_client` is a shipped WebSocket client, and
//! `channels::server` is the real `axum::serve`; the rule is about where
//! a *check* stands, never about what production code may call.
//!
//! A line may still be excused with `boundary-ok: <reason>` on itself or
//! on the line above, the same waiver `lexicon` uses and read the same
//! way. **A whole file that predates the rule goes in the register
//! instead**, because a waiver is permanent and a debt should remove
//! itself: a file the register names may cross, and a file it names that
//! has stopped crossing must be struck.

use std::path::Path;

use syn::spanned::Spanned;

use crate::report::{Violation, XtaskError};
use crate::walk;

const EXEMPT_MARK: &str = "boundary-ok:";

/// The surfaces this product raises, which a check can only be reaching
/// from outside.
///
/// Closed, and narrow on purpose. Every entry names *our* binary or
/// *our* server, so its presence in a check admits no second reading.
/// Three tokens were tried here and removed: `TcpStream::connect`,
/// `Command::new` and `process::Command` say that a socket or a process
/// exists, not which side of the boundary it points at, and each of them
/// is what a test double looks like from the inside.
const CROSSINGS: [(&str, &str); 5] = [
    ("CARGO_BIN_EXE", "cargo's own handle on the binary this repository builds"),
    ("SPRAWLING_BIN", "the path `just adversary` hands the shipped binary over"),
    (
        "axum::serve",
        "raising this product's own HTTP surface in order to talk to it over a socket",
    ),
    (
        "tungstenite",
        "a WebSocket client, and the only WebSocket server in this world is ours",
    ),
    (
        "assert_cmd",
        "a command-line harness, which exists to drive a built binary",
    ),
];

/// Where a whole file is test code by its address.
///
/// `citysim` is the deterministic simulator and `fuzz` holds the fuzz
/// targets; both are checks that happen to be shaped like crates, and
/// naming them here is cheaper than teaching the walk what a harness is.
fn is_test_file(rel: &str) -> bool {
    rel.starts_with("citysim/")
        || rel.starts_with("fuzz/")
        || (rel.starts_with("crates/") && rel.contains("/tests/"))
}

/// Where test code hides inside a source file: the line span of every
/// item marked `#[cfg(test)]`.
///
/// Found by parsing rather than by looking for the attribute and reading
/// to the end of the file. A `#[cfg(test)] mod tests` is usually last and
/// usually is the rest of the file, and a gate that assumed so would be
/// wrong in exactly the files where it is not.
fn test_spans(text: &str, rel: &str) -> Result<Vec<(usize, usize)>, XtaskError> {
    let parsed = syn::parse_file(text).map_err(|err| XtaskError::Doc {
        file: rel.to_owned(),
        msg: format!("this file does not parse as Rust: {err}"),
    })?;
    let mut spans = Vec::new();
    collect(&parsed.items, &mut spans);
    Ok(spans)
}

fn collect(items: &[syn::Item], out: &mut Vec<(usize, usize)>) {
    for item in items {
        if marked_test(attrs_of(item)) {
            let span = item.span();
            out.push((span.start().line, span.end().line));
            continue;
        }
        if let syn::Item::Mod(module) = item
            && let Some((_, inner)) = &module.content
        {
            collect(inner, out);
        }
    }
}

fn attrs_of(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Fn(it) => &it.attrs,
        syn::Item::Mod(it) => &it.attrs,
        syn::Item::Impl(it) => &it.attrs,
        syn::Item::Struct(it) => &it.attrs,
        syn::Item::Enum(it) => &it.attrs,
        syn::Item::Use(it) => &it.attrs,
        syn::Item::Const(it) => &it.attrs,
        syn::Item::Static(it) => &it.attrs,
        syn::Item::Trait(it) => &it.attrs,
        syn::Item::Type(it) => &it.attrs,
        _ => &[],
    }
}

fn marked_test(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        attr.path().is_ident("cfg")
            && attr
                .meta
                .require_list()
                .is_ok_and(|list| list.tokens.to_string().contains("test"))
    })
}

/// The register row naming the files that crossed before the rule did.
const ROW: &str = "boundary";
const PREDATING: &str = "predating";

pub(crate) fn check(root: &Path) -> Result<Vec<Violation>, XtaskError> {
    let excused = excused(root)?;
    let mut crossing = std::collections::BTreeSet::new();
    let mut violations = Vec::new();
    for file in walk::files_with_ext(root, &["rs"])? {
        let rel = walk::rel(root, &file);
        if walk::in_isolation_zone(&rel) || rel == "xtask/src/boundary.rs" {
            continue;
        }
        let text = walk::read_text(&file)?;
        let whole = is_test_file(&rel);
        let spans = if whole {
            Vec::new()
        } else if rel.starts_with("crates/") || rel.starts_with("xtask/src/") {
            test_spans(&text, &rel)?
        } else {
            continue;
        };
        if !whole && spans.is_empty() {
            continue;
        }
        let mut found = Vec::new();
        Sweep {
            rel: &rel,
            text: &text,
            whole,
            spans: &spans,
        }
        .scan(&mut found);
        if found.is_empty() {
            continue;
        }
        crossing.insert(rel.clone());
        if !excused.contains(&rel) {
            violations.extend(found);
        }
    }
    // A row naming a file that has stopped crossing is a licence nobody
    // decided to keep granting, and it would silently re-admit the file
    // if anybody put a socket back into it.
    for spent in excused.difference(&crossing) {
        violations.push(Violation {
            gate: "boundary",
            location: format!("xtask/budgets.toml [{ROW}.{PREDATING}]"),
            rule: "an exception that is no longer needed is struck from the register".to_owned(),
            violation: format!("{spent} no longer crosses the process boundary"),
            alternative: "delete its row: the check has moved to adversary/ or come back \
                          in-process, and the debt it recorded is paid"
                .to_owned(),
        });
    }
    Ok(violations)
}

/// The files that were already crossing when the rule was drawn.
fn excused(root: &Path) -> Result<std::collections::BTreeSet<String>, XtaskError> {
    let register = crate::budget::register(root)?;
    let Some(listed) = register
        .get(ROW)
        .and_then(|row| row.get(PREDATING))
        .and_then(|table| table.get("files"))
        .and_then(toml::Value::as_array)
    else {
        return Ok(std::collections::BTreeSet::new());
    };
    let mut out = std::collections::BTreeSet::new();
    for value in listed {
        let named = value.as_str().ok_or_else(|| XtaskError::Doc {
            file: "xtask/budgets.toml".to_owned(),
            msg: format!("{ROW}.{PREDATING} holds something that is not a path"),
        })?;
        out.insert(named.to_owned());
    }
    Ok(out)
}

/// One file, and where in it the test code is: four values that always
/// travel together and are never chosen independently.
struct Sweep<'a> {
    rel: &'a str,
    text: &'a str,
    /// Whether the whole file is test code by its address.
    whole: bool,
    /// The line spans of `#[cfg(test)]` items, when it is not.
    spans: &'a [(usize, usize)],
}

impl Sweep<'_> {
    fn scan(&self, out: &mut Vec<Violation>) {
        let mut previous_exempts = false;
        for (index, line) in self.text.lines().enumerate() {
            let number = index.saturating_add(1);
            let marked = line.contains(EXEMPT_MARK);
            let exempt = marked || previous_exempts;
            previous_exempts = marked;
            if exempt || !self.is_test_code(number) {
                continue;
            }
            for (token, what) in CROSSINGS {
                if line.contains(token) {
                    out.push(crossed(self.rel, number, token, what));
                    break;
                }
            }
        }
    }

    fn is_test_code(&self, line: usize) -> bool {
        self.whole
            || self
                .spans
                .iter()
                .any(|(first, last)| line >= *first && line <= *last)
    }
}

fn crossed(rel: &str, line: usize, token: &str, what: &str) -> Violation {
    Violation {
        gate: "boundary",
        location: format!("{rel}:{line}"),
        rule: "a check that crosses the process boundary is written in Haskell under \
               adversary/; Rust checks link the crates and enter by their public faces"
            .to_owned(),
        violation: format!("`{token}` in test code: {what}"),
        alternative: "move it to adversary/ as a property over traces, or drive the same \
                      thing in-process through the door channels::server uses - and if \
                      neither is what you meant, mark the line `boundary-ok: <reason>`"
            .to_owned(),
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::indexing_slicing,
    reason = "test code"
)]
mod tests {
    use super::*;

    fn found(rel: &str, source: &str) -> Vec<Violation> {
        let mut out = Vec::new();
        let spans = test_spans(source, rel).unwrap();
        Sweep {
            rel,
            text: source,
            whole: is_test_file(rel),
            spans: &spans,
        }
        .scan(&mut out);
        out
    }

    /// The finding that corrected this gate. A test standing up a fake
    /// provider and speaking HTTP to it is playing the outside world,
    /// which is the opposite of reaching this product from outside.
    #[test]
    fn a_socket_to_a_double_the_test_raised_is_not_a_crossing() {
        let source = "\
            #[cfg(test)]\n\
            mod tests {\n\
                fn hostile_upstream() {\n\
                    let (url, _p) = fake_openai(&[\"m\"], vec![]);\n\
                    drop(std::net::TcpStream::connect(&url).unwrap());\n\
                }\n\
            }\n";
        assert!(found("crates/sprawling/src/assembly/driving.rs", source).is_empty());
    }

    /// Reaching the binary this repository builds is standing outside it,
    /// and that check belongs to Haskell.
    #[test]
    fn driving_the_built_binary_is_refused() {
        let source = "\
            fn run() {}\n\
            #[cfg(test)]\n\
            mod tests {\n\
                #[test]\n\
                fn drives_the_binary() {\n\
                    let _ = std::process::Command::new(env!(\"CARGO_BIN_EXE_sprawling\"));\n\
                }\n\
            }\n";
        let out = found("crates/sprawling/src/main.rs", source);
        assert_eq!(out.len(), 1, "{out:#?}");
        assert!(out[0].location.ends_with(":6"), "{:?}", out[0].location);
    }

    /// A whole file under `crates/*/tests/` is test code by its address,
    /// with no attribute to find - and raising this product's own server
    /// to speak HTTP at it is the crossing `enrolment.rs` makes.
    #[test]
    fn a_file_under_tests_is_test_code_by_where_it_lives() {
        let source = "fn helper() {\n    let _ = axum::serve(listener, app);\n}\n";
        assert_eq!(found("crates/channels/tests/enrolment.rs", source).len(), 1);
        assert!(found("crates/channels/src/server.rs", source).is_empty());
    }

    /// The waiver is read on the line and on the line above, which is
    /// how `lexicon` reads its own.
    #[test]
    fn a_waiver_on_the_line_or_the_line_above_is_honoured() {
        let source = "\
            #[cfg(test)]\n\
            mod tests {\n\
                fn a() {\n\
                    // boundary-ok: a reason\n\
                    let _ = env!(\"CARGO_BIN_EXE_sprawling\");\n\
                }\n\
                fn b() {\n\
                    let _ = env!(\"CARGO_BIN_EXE_sprawling\"); // boundary-ok: same\n\
                }\n\
            }\n";
        assert!(found("crates/runtime/src/x.rs", source).is_empty());
    }

    /// The repository passes the check it ships. A gate whose own tree
    /// is red teaches people that red is the normal colour.
    #[test]
    fn the_repository_itself_passes_the_check_it_ships() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(Path::to_path_buf)
            .expect("xtask lives one level under the repo root");
        let found = check(&root).unwrap();
        assert!(found.is_empty(), "{found:#?}");
    }
}
