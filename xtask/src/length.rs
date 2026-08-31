// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Function-length gate: a function nobody reads to the end turns the
//! build red.
//!
//! The unit is a production function, not a file. Any honest file
//! threshold lights up eight files across four crates at once, which is
//! a project rather than a gate; the function threshold has one job,
//! which is to stop a whole flow of control from being written into one
//! body again.
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

pub(crate) fn check(root: &Path) -> Result<Vec<Violation>, XtaskError> {
    let limit = limit(root)?;
    let shapes = modmap::shapes(root)?;
    let mut violations = Vec::new();
    for file in sources(root)? {
        let rel = walk::rel(root, &file);
        if shapes.get(&rel).is_some_and(|shape| shape == "data") {
            continue;
        }
        let text = walk::read_text(&file)?;
        let parsed = syn::parse_file(&text).map_err(|err| XtaskError::Doc {
            file: rel.clone(),
            msg: format!("this file does not parse as Rust: {err}"),
        })?;
        for found in measure(&parsed.items) {
            if found.lines > limit {
                violations.push(over(&rel, &found, limit));
            }
        }
    }
    Ok(violations)
}

/// One function, as the gate sees it.
pub(crate) struct Found {
    pub(crate) name: String,
    pub(crate) line: usize,
    pub(crate) lines: usize,
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

fn limit(root: &Path) -> Result<usize, XtaskError> {
    let register = crate::budget::register(root)?;
    let stated = register
        .get(ROW)
        .and_then(|row| row.get("budget_lines"))
        .and_then(toml::Value::as_integer)
        .ok_or_else(|| XtaskError::Doc {
            file: "xtask/budgets.toml".to_owned(),
            msg: format!("{ROW} states no budget_lines"),
        })?;
    usize::try_from(stated).map_err(|_| XtaskError::Doc {
        file: "xtask/budgets.toml".to_owned(),
        msg: format!("{ROW}.budget_lines is not a line count: {stated}"),
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
                out.push(found(
                    &function.sig.ident.to_string(),
                    function.sig.fn_token.span(),
                    function.block.span(),
                ));
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
                    out.push(found(
                        &function.sig.ident.to_string(),
                        function.sig.fn_token.span(),
                        function.block.span(),
                    ));
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
                    out.push(found(
                        &function.sig.ident.to_string(),
                        function.sig.fn_token.span(),
                        body.span(),
                    ));
                }
            }
            _ => {}
        }
    }
    out
}

fn found(name: &str, signature: proc_macro2::Span, body: proc_macro2::Span) -> Found {
    let start = signature.start().line;
    let end = body.end().line;
    Found {
        name: name.to_owned(),
        line: start,
        lines: end.saturating_sub(start).saturating_add(1),
    }
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
