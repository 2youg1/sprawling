// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Length gate: a body nobody reads to the end, and a file nobody can
//! navigate, both turn the build red.
//!
//! Two units, because they fail differently. A long **function** hides a
//! flow of control; a long **file** hides where anything is, and costs
//! every reader who has to find one thing in it.
//!
//! **The file rule is a re-pricing, and here is the parameter that
//! moved.** This gate used to measure functions only, and said so: any
//! honest file threshold lit up eight files across four crates at once,
//! "which is a project rather than a gate". That was a true reading of
//! the cost and a fair reason to wait. The person has now asked for that
//! project, on the ground that the largest file - 12,078 lines - was
//! costing more per iteration than the split would cost once. So the
//! threshold arrives with a register of the files that predate it, each
//! pinned at the length it had on the day the line was drawn.
//!
//! **The register can only shrink, and it cleans itself.** A file it does
//! not name is refused at the budget outright, so the list cannot grow. A
//! file it does name may not exceed its pinned length, so no offender
//! grows. And a file that has come back under the budget must be struck
//! from the register, which turns the exception into something that
//! removes itself rather than something that has to be remembered.
//!
//! **The measurement parses.** Finding where a function begins and ends
//! by counting braces per line is wrong in three ways this repository
//! contains: `#[cfg(test)]` marks an item rather than the rest of a
//! file, `'{'` is a character and not a block, and a string may carry a
//! brace across a line continuation. Each mistake produces a wrong list
//! of offenders, and a gate that measures wrongly sends somebody to
//! break up a function that was never long. `syn` is the parser the
//! compiler's own macro ecosystem uses; it lands in workspace tooling
//! and never in the product binary.
//!
//! Three kinds are not measured, and every exemption is read from an
//! authority that already exists rather than from a list kept here:
//! an item marked `#[cfg(test)]`, a function marked `#[component]`
//! (a Dioxus component's body is markup, with no steps to follow), and
//! any file whose module-map row states the shape `data`
//! (ARCHITECTURE.md section 9, shape 6: data with no branches).
//!
//! The `data` exemption covers the file rule as well as the function
//! rule, for the reason that granted it: a table is looked things up in
//! rather than navigated, so its length costs a reader nothing. That is
//! why `web::lang`, 3,217 lines of every word the client says in two
//! languages, is not in the register below.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use syn::spanned::Spanned;

use crate::modmap;
use crate::report::{Violation, XtaskError};
use crate::walk;

/// Where first-party Rust lives. `tests/` and `benches/` are absent on
/// purpose: test code may relax what production code carries (AGENTS.md),
/// and a long test is a different question from a long function.
const SOURCE_DIRS: [&str; 3] = ["crates", "xtask/src", "citysim/src"];

/// The register row that states the limit, so the number lives with every
/// other budget the design states rather than inside this file.
const ROW: &str = "function_length";

/// The register row that states how long a file may be.
const FILE_ROW: &str = "file_length";

/// The sub-table naming the files that were already over the line when it
/// was drawn, each with the length it had that day.
const PREDATING: &str = "predating";

/// The register row that states how many parameters a function may take.
const ARG_ROW: &str = "argument_count";

pub(crate) fn check(root: &Path) -> Result<Vec<Violation>, XtaskError> {
    let body_limit = limit(root, ROW)?;
    let file_limit = limit(root, FILE_ROW)?;
    let arg_limit = limit(root, ARG_ROW)?;
    let excused = excused(root)?;
    let predating = predating(root)?;
    let shapes = modmap::shapes(root)?;
    let mut violations = Vec::new();
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for file in sources(root)? {
        let rel = walk::rel(root, &file);
        if shapes.get(&rel).is_some_and(|shape| shape == "data") {
            continue;
        }
        let text = walk::read_text(&file)?;
        let lines = text.lines().count();
        seen.insert(rel.clone());
        match predating.get(&rel) {
            None => {
                if lines > file_limit {
                    violations.push(too_long(&rel, lines, file_limit));
                }
            }
            Some(&pinned) => {
                if lines > pinned {
                    violations.push(grew(&rel, lines, pinned));
                } else if lines <= file_limit {
                    violations.push(no_longer_an_exception(&rel, lines, file_limit));
                }
            }
        }
        let parsed = syn::parse_file(&text).map_err(|err| XtaskError::Doc {
            file: rel.clone(),
            msg: format!("this file does not parse as Rust: {err}"),
        })?;
        for found in measure(&parsed.items) {
            if found.lines > body_limit {
                violations.push(over(&rel, &found, body_limit));
            }
            if found.args > arg_limit && !excused.contains(&key(&rel, &found.name)) {
                violations.push(too_many_arguments(&rel, &found, arg_limit));
            }
        }
    }
    // A row naming a file that is no longer measured - renamed, split
    // away, or deleted - is a pin nothing holds. Left alone it would
    // silently re-admit that path if anybody ever recreated it.
    for stale in predating.keys().filter(|rel| !seen.contains(*rel)) {
        violations.push(Violation {
            gate: "length",
            location: format!("xtask/budgets.toml [{FILE_ROW}.{PREDATING}]"),
            rule: "every file the register pins is a file this gate measures".to_owned(),
            violation: format!("{stale} is pinned and no longer here"),
            alternative: "delete the row: the debt it recorded has been paid or moved".to_owned(),
        });
    }
    Ok(violations)
}

fn too_long(rel: &str, lines: usize, limit: usize) -> Violation {
    Violation {
        gate: "length",
        location: rel.to_owned(),
        rule: format!("a source file stays inside {limit} lines (xtask/budgets.toml, {FILE_ROW})"),
        violation: format!("{lines} lines"),
        alternative: "give each responsibility in it its own file and name, and let this one \
                      keep the part that routes between them"
            .to_owned(),
    }
}

fn grew(rel: &str, lines: usize, pinned: usize) -> Violation {
    Violation {
        gate: "length",
        location: rel.to_owned(),
        rule: format!(
            "a file the register pins may only get smaller \
             (xtask/budgets.toml, {FILE_ROW}.{PREDATING})"
        ),
        violation: format!("{lines} lines, pinned at {pinned}"),
        alternative: "put the new code in a file of its own: this one is already over the \
                      budget and is waiting to be split"
            .to_owned(),
    }
}

fn no_longer_an_exception(rel: &str, lines: usize, limit: usize) -> Violation {
    Violation {
        gate: "length",
        location: format!("xtask/budgets.toml [{FILE_ROW}.{PREDATING}]"),
        rule: "an exception that is no longer needed is struck from the register".to_owned(),
        violation: format!("{rel} is {lines} lines, inside the {limit} the rule states"),
        alternative: "delete its row: the split is done, and a spent exception left in place \
                      is a licence nobody decided to keep granting"
            .to_owned(),
    }
}

/// The files that were already over the line when it was drawn, each
/// pinned at the length it had that day.
fn predating(root: &Path) -> Result<BTreeMap<String, usize>, XtaskError> {
    let register = crate::budget::register(root)?;
    let Some(table) = register
        .get(FILE_ROW)
        .and_then(|row| row.get(PREDATING))
        .and_then(toml::Value::as_table)
    else {
        return Ok(BTreeMap::new());
    };
    let mut out = BTreeMap::new();
    for (rel, value) in table {
        let stated = value.as_integer().ok_or_else(|| XtaskError::Doc {
            file: "xtask/budgets.toml".to_owned(),
            msg: format!("{FILE_ROW}.{PREDATING}.{rel} is not a line count"),
        })?;
        let pinned = usize::try_from(stated).map_err(|_| XtaskError::Doc {
            file: "xtask/budgets.toml".to_owned(),
            msg: format!("{FILE_ROW}.{PREDATING}.{rel} is not a line count: {stated}"),
        })?;
        out.insert(rel.clone(), pinned);
    }
    Ok(out)
}

/// One function, as the gate sees it.
pub(crate) struct Found {
    pub(crate) name: String,
    pub(crate) line: usize,
    pub(crate) lines: usize,
    /// Parameters, not counting a receiver. `self` is what the method is
    /// for, never an argument somebody had to decide to pass.
    pub(crate) args: usize,
}

fn too_many_arguments(rel: &str, found: &Found, limit: usize) -> Violation {
    Violation {
        gate: "length",
        location: format!("{rel}:{}", found.line),
        rule: format!(
            "a function takes at most {limit} parameters \
             (xtask/budgets.toml, {ARG_ROW})"
        ),
        violation: format!("{} takes {}", found.name, found.args),
        alternative: "the ones that always travel together are one value: give them a struct \
                      with a name, as `Reporter` did for the four that describe who is \
                      reporting a change to a plan"
            .to_owned(),
    }
}

fn over(rel: &str, found: &Found, limit: usize) -> Violation {
    Violation {
        gate: "length",
        location: format!("{rel}:{}", found.line),
        rule: format!(
            "a production function stays inside {limit} lines \
             (xtask/budgets.toml, function_length)"
        ),
        violation: format!("{} is {} lines", found.name, found.lines),
        alternative: "give a phase of it its own name, and hand the values it produces back \
                      as one value"
            .to_owned(),
    }
}

fn limit(root: &Path, row: &str) -> Result<usize, XtaskError> {
    let register = crate::budget::register(root)?;
    let stated = register
        .get(row)
        .and_then(|found| {
            found
                .get("budget_lines")
                .or_else(|| found.get("budget_arguments"))
        })
        .and_then(toml::Value::as_integer)
        .ok_or_else(|| XtaskError::Doc {
            file: "xtask/budgets.toml".to_owned(),
            msg: format!("{row} states no budget_lines"),
        })?;
    usize::try_from(stated).map_err(|_| XtaskError::Doc {
        file: "xtask/budgets.toml".to_owned(),
        msg: format!("{row}.budget_lines is not a line count: {stated}"),
    })
}

fn sources(root: &Path) -> Result<Vec<std::path::PathBuf>, XtaskError> {
    let mut out = Vec::new();
    for dir in SOURCE_DIRS {
        let base = root.join(dir);
        if !base.exists() {
            continue;
        }
        for file in walk::files_with_ext(&base, &["rs"])? {
            // Only a crate's own sources; `crates/*/tests` and the fuzz
            // targets are test code by another name.
            let rel = walk::rel(root, &file);
            if rel.starts_with("crates/") && !rel.contains("/src/") {
                continue;
            }
            out.push(file);
        }
    }
    Ok(out)
}

/// Walks items, skipping what is not measured, and measures the rest.
///
/// Recurses into `mod` and `impl` blocks because that is where most
/// functions in this repository live; a nested `#[cfg(test)] mod tests`
/// therefore drops out at its own level rather than by file position.
fn measure(items: &[syn::Item]) -> Vec<Found> {
    let mut out = Vec::new();
    for item in items {
        match item {
            syn::Item::Fn(function) => {
                if skipped(&function.attrs) {
                    continue;
                }
                out.push(found(&function.sig, function.block.span()));
            }
            syn::Item::Mod(module) => {
                if skipped(&module.attrs) {
                    continue;
                }
                if let Some((_, items)) = &module.content {
                    out.extend(measure(items));
                }
            }
            syn::Item::Impl(block) => {
                if skipped(&block.attrs) {
                    continue;
                }
                for member in &block.items {
                    let syn::ImplItem::Fn(function) = member else {
                        continue;
                    };
                    if skipped(&function.attrs) {
                        continue;
                    }
                    out.push(found(&function.sig, function.block.span()));
                }
            }
            syn::Item::Trait(declared) => {
                if skipped(&declared.attrs) {
                    continue;
                }
                for member in &declared.items {
                    let syn::TraitItem::Fn(function) = member else {
                        continue;
                    };
                    let Some(body) = &function.default else {
                        continue; // a signature is not a function body
                    };
                    if skipped(&function.attrs) {
                        continue;
                    }
                    out.push(found(&function.sig, body.span()));
                }
            }
            _ => {}
        }
    }
    out
}

fn found(signature: &syn::Signature, body: proc_macro2::Span) -> Found {
    let start = signature.fn_token.span().start().line;
    let end = body.end().line;
    Found {
        name: signature.ident.to_string(),
        line: start,
        lines: end.saturating_sub(start).saturating_add(1),
        // A receiver is not a parameter. `&self` is what makes the
        // function a method, not a value somebody chose to thread
        // through it, and counting it would charge every method one
        // argument it never had a say in.
        args: signature
            .inputs
            .iter()
            .filter(|input| matches!(input, syn::FnArg::Typed(_)))
            .count(),
    }
}

/// How a function is named in the register: the file it lives in and its
/// own name. Two functions in one file cannot share a name, and a name
/// alone would excuse every `new` in the workspace at once.
fn key(rel: &str, name: &str) -> String {
    format!("{rel}::{name}")
}

/// The functions whose parameter lists predate the rule.
fn excused(root: &Path) -> Result<BTreeSet<String>, XtaskError> {
    let register = crate::budget::register(root)?;
    let Some(listed) = register
        .get(ARG_ROW)
        .and_then(|row| row.get(PREDATING))
        .and_then(|table| table.get("names"))
        .and_then(toml::Value::as_array)
    else {
        return Ok(BTreeSet::new());
    };
    let mut out = BTreeSet::new();
    for value in listed {
        let named = value.as_str().ok_or_else(|| XtaskError::Doc {
            file: "xtask/budgets.toml".to_owned(),
            msg: format!("{ARG_ROW}.{PREDATING} holds something that is not a name"),
        })?;
        out.insert(named.to_owned());
    }
    Ok(out)
}

/// Whether this item is one of the two the gate does not measure.
fn skipped(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if attr.path().is_ident("component") {
            return true;
        }
        if !attr.path().is_ident("cfg") {
            return false;
        }
        attr.meta
            .require_list()
            .is_ok_and(|list| list.tokens.to_string().contains("test"))
    })
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

    /// A receiver is not a parameter, and a data clump is.
    #[test]
    fn a_receiver_is_not_an_argument_and_everything_else_is() {
        let source = "\
            struct S;\n\
            impl S {\n\
              fn free() {}\n\
              fn borrowed(&self) {}\n\
              fn owned(self, a: u8) {}\n\
              fn mutable(&mut self, a: u8, b: u8, c: u8, d: u8) {}\n\
              fn clump(&self, a: u8, b: u8, c: u8, d: u8, e: u8) {}\n\
            }\n";
        let parsed = syn::parse_file(source).unwrap();
        let counted: Vec<(String, usize)> = measure(&parsed.items)
            .into_iter()
            .map(|found| (found.name, found.args))
            .collect();
        assert_eq!(
            counted,
            [
                ("free".to_owned(), 0),
                ("borrowed".to_owned(), 0),
                ("owned".to_owned(), 1),
                ("mutable".to_owned(), 4),
                ("clump".to_owned(), 5),
            ]
        );
    }

    /// Every excused signature must still exist and must still be over
    /// the budget. A name that no longer names anything is a licence
    /// waiting to be spent by whoever writes that function next.
    #[test]
    fn every_excused_signature_is_a_real_one_that_is_still_over() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(Path::to_path_buf)
            .expect("xtask lives one level under the repo root");
        let budget = limit(&root, ARG_ROW).unwrap();
        let excused = excused(&root).unwrap();
        assert!(!excused.is_empty(), "the register records the debt it owes");
        let shapes = modmap::shapes(&root).unwrap();
        let mut still_over = BTreeSet::new();
        for file in sources(&root).unwrap() {
            let rel = walk::rel(&root, &file);
            if shapes.get(&rel).is_some_and(|shape| shape == "data") {
                continue;
            }
            let text = walk::read_text(&file).unwrap();
            let parsed = syn::parse_file(&text).unwrap();
            for item in measure(&parsed.items) {
                if item.args > budget {
                    still_over.insert(key(&rel, &item.name));
                }
            }
        }
        let spent: Vec<&String> = excused.difference(&still_over).collect();
        assert!(
            spent.is_empty(),
            "these are excused and no longer need to be; strike them:\n{spent:#?}"
        );
    }

    /// The register is the authority for both numbers, and both are read
    /// from it by name. A row that stops stating its budget must fail
    /// loudly rather than fall back to something this file believes.
    #[test]
    fn both_limits_come_from_the_register_and_neither_has_a_default() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(Path::to_path_buf)
            .expect("xtask lives one level under the repo root");
        assert_eq!(limit(&root, ROW).unwrap(), 200);
        assert_eq!(limit(&root, FILE_ROW).unwrap(), 1000);
        assert!(limit(&root, "a_row_nobody_wrote").is_err());
    }

    /// Every pin is a debt, so every pin must be above the budget it
    /// excuses. A row at or below it is not an exception, it is a licence
    /// somebody forgot to spend, and `no_longer_an_exception` reports it.
    #[test]
    fn every_pinned_file_is_over_the_budget_it_is_excused_from() {
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .parent()
            .map(Path::to_path_buf)
            .expect("xtask lives one level under the repo root");
        let budget = limit(&root, FILE_ROW).unwrap();
        let pinned = predating(&root).unwrap();
        // No assertion that the register is non-empty. It held while
        // there was a file left to split and turned red the moment the
        // last one landed, which made finishing the work look like
        // breaking the gate. What is worth holding is the property each
        // row must have, over however many rows there are - and an empty
        // register is this rule's finished state, not its failure.
        for (rel, lines) in &pinned {
            assert!(
                *lines > budget,
                "{rel} is pinned at {lines}, which the {budget}-line rule already allows"
            );
            assert!(
                root.join(rel).exists(),
                "{rel} is pinned and not in the tree"
            );
        }
    }

    fn lengths(source: &str) -> Vec<(String, usize)> {
        let parsed = syn::parse_file(source).unwrap();
        measure(&parsed.items)
            .into_iter()
            .map(|found| (found.name, found.lines))
            .collect()
    }

    #[test]
    fn a_brace_inside_a_literal_is_not_a_block() {
        // The counting bug that turned a 3-line function into the rest
        // of its file: `'{'` is a character, and `"{"` is a string.
        let found = lengths(
            "fn detect(t: &str) -> bool {\n    t.starts_with('{') || t.ends_with(\"}\")\n}\nfn after() {}\n",
        );
        assert_eq!(
            found,
            vec![("detect".to_owned(), 3), ("after".to_owned(), 1)]
        );
    }

    #[test]
    fn cfg_test_marks_an_item_and_not_the_rest_of_the_file() {
        let found = lengths(
            "#[cfg(test)]\nfn helper() {\n    let _ = 1;\n}\nfn production() {\n    let _ = 2;\n}\n",
        );
        assert_eq!(found, vec![("production".to_owned(), 3)]);
    }

    #[test]
    fn a_component_is_markup_and_a_plain_function_is_not() {
        let found = lengths(
            "#[component]\nfn Page() -> Element {\n    rsx! { div {} }\n}\nfn plain() -> u8 {\n    1\n}\n",
        );
        assert_eq!(found, vec![("plain".to_owned(), 3)]);
    }

    #[test]
    fn methods_inside_an_impl_are_measured_one_by_one() {
        let found = lengths(
            "struct S;\nimpl S {\n    fn a(&self) {}\n    #[cfg(test)]\n    fn b(&self) {}\n}\n",
        );
        assert_eq!(found, vec![("a".to_owned(), 1)]);
    }
}
