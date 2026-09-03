// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Artifact gate: nothing written for a test is compiled into the thing
//! a person downloads (the person's ruling, 2026-09-05).
//!
//! Two assertions, because test code reaches the binary two ways.
//!
//! **An item named for a test carries a `cfg` that keeps it out.** Four
//! of this repository's five conformance suites sit behind
//! `#[cfg(feature = "conformance")]`; the fifth, `browser`'s, was a plain
//! `pub fn` re-exported from `lib.rs`, and its own lint waiver called it
//! "dev-only by contract" while nothing held that contract. **A contract
//! with no holder is the defect this gate exists for** - it is invisible
//! in review precisely because the comment says the right thing.
//!
//! **The product binary enables no test feature.** The first assertion
//! is worthless if `sprawling` turns `conformance` on: the code would be
//! correctly marked and shipped anyway. `--all-features`, which
//! `just clippy` and `just test` both pass, is the build where every
//! suite must compile; the release build is the one where none of them
//! may exist.
//!
//! Judged by name rather than by content. A doc comment on a production
//! trait may name the suite that holds its adapters - `BrowserPort`'s
//! does - and a gate reading item bodies would convict the trait for
//! documenting itself.

use std::path::Path;

use crate::report::{Violation, XtaskError};
use crate::walk;

/// What a name says when it says "this exists for a test".
///
/// Closed, and matched against an item's own identifier only. Every entry
/// is a word nobody uses for production code in this tree.
const MARKERS: [&str; 6] = [
    "conformance",
    "_for_test",
    "mock_",
    "fake_",
    "stub_",
    "dummy_",
];

/// The features that exist to build test scaffolding, and nothing else.
///
/// `fault` is here with `conformance`: `memory::fault_fs` is the second
/// adapter behind the `Vfs` inner seam, a deterministic power-loss model
/// whose whole purpose is to be injected by a test.
const TEST_FEATURES: [&str; 2] = ["conformance", "fault"];

/// The crate whose feature table becomes the shipped binary.
const PRODUCT: &str = "crates/sprawling/Cargo.toml";

pub(crate) fn check(root: &Path) -> Result<Vec<Violation>, XtaskError> {
    let mut violations = Vec::new();
    for file in walk::files_with_ext(&root.join("crates"), &["rs"])? {
        let rel = walk::rel(root, &file);
        if !rel.contains("/src/") {
            continue;
        }
        let text = walk::read_text(&file)?;
        let parsed = syn::parse_file(&text).map_err(|err| XtaskError::Doc {
            file: rel.clone(),
            msg: format!("this file does not parse as Rust: {err}"),
        })?;
        walk_items(&rel, &parsed.items, &mut violations);
    }
    violations.extend(product_features(root)?);
    Ok(violations)
}

/// Walks items, stopping at anything a `cfg` already excludes.
///
/// A `#[cfg(test)] mod tests` needs no further judgement: everything
/// inside it is already outside the product build, and descending would
/// convict a test double for being named like one.
fn walk_items(rel: &str, items: &[syn::Item], out: &mut Vec<Violation>) {
    for item in items {
        let attrs = attrs_of(item);
        if has_cfg(attrs) {
            continue;
        }
        for name in names_of(item) {
            if let Some(marker) = MARKERS.iter().find(|marker| name.contains(*marker)) {
                out.push(ships(rel, &name, marker));
            }
        }
        if let syn::Item::Mod(module) = item
            && let Some((_, inner)) = &module.content
        {
            walk_items(rel, inner, out);
        }
    }
}

fn attrs_of(item: &syn::Item) -> &[syn::Attribute] {
    match item {
        syn::Item::Fn(it) => &it.attrs,
        syn::Item::Mod(it) => &it.attrs,
        syn::Item::Use(it) => &it.attrs,
        syn::Item::Struct(it) => &it.attrs,
        syn::Item::Enum(it) => &it.attrs,
        syn::Item::Const(it) => &it.attrs,
        syn::Item::Static(it) => &it.attrs,
        syn::Item::Trait(it) => &it.attrs,
        syn::Item::Type(it) => &it.attrs,
        syn::Item::Impl(it) => &it.attrs,
        _ => &[],
    }
}

fn has_cfg(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident("cfg"))
}

/// The identifiers an item introduces, which is what the rule judges.
///
/// A `use` contributes every leaf it names, because a re-export puts the
/// item on this crate's public surface as surely as declaring it does.
fn names_of(item: &syn::Item) -> Vec<String> {
    match item {
        syn::Item::Fn(it) => vec![it.sig.ident.to_string()],
        syn::Item::Mod(it) => vec![it.ident.to_string()],
        syn::Item::Struct(it) => vec![it.ident.to_string()],
        syn::Item::Enum(it) => vec![it.ident.to_string()],
        syn::Item::Const(it) => vec![it.ident.to_string()],
        syn::Item::Static(it) => vec![it.ident.to_string()],
        syn::Item::Trait(it) => vec![it.ident.to_string()],
        syn::Item::Type(it) => vec![it.ident.to_string()],
        syn::Item::Use(it) => {
            let mut out = Vec::new();
            leaves(&it.tree, &mut out);
            out
        }
        _ => Vec::new(),
    }
}

fn leaves(tree: &syn::UseTree, out: &mut Vec<String>) {
    match tree {
        syn::UseTree::Path(path) => leaves(&path.tree, out),
        syn::UseTree::Name(name) => out.push(name.ident.to_string()),
        syn::UseTree::Rename(rename) => {
            out.push(rename.ident.to_string());
            out.push(rename.rename.to_string());
        }
        syn::UseTree::Group(group) => {
            for item in &group.items {
                leaves(item, out);
            }
        }
        syn::UseTree::Glob(_) => {}
    }
}

fn ships(rel: &str, name: &str, marker: &str) -> Violation {
    Violation {
        gate: "artifact",
        location: rel.to_owned(),
        rule: "an item named for a test carries a cfg that keeps it out of the product build"
            .to_owned(),
        violation: format!("`{name}` says `{marker}` and nothing excludes it"),
        alternative: "put it behind `#[cfg(feature = \"conformance\")]`, declare that feature \
                      in the crate's manifest, and carry the same cfg on the re-export - \
                      `kernel::ledger` is the shape to copy"
            .to_owned(),
    }
}

/// The shipped binary's own feature table, which must name no test
/// feature at all - not as a default, and not as an entry a person could
/// switch on.
fn product_features(root: &Path) -> Result<Vec<Violation>, XtaskError> {
    let path = root.join(PRODUCT);
    let text = walk::read_text(&path)?;
    let parsed: toml::Value = toml::from_str(&text).map_err(|err| XtaskError::Doc {
        file: PRODUCT.to_owned(),
        msg: err.to_string(),
    })?;
    let Some(features) = parsed.get("features").and_then(toml::Value::as_table) else {
        return Ok(Vec::new());
    };
    let mut out = Vec::new();
    for (name, enables) in features {
        let mut named: Vec<String> = vec![name.clone()];
        if let Some(list) = enables.as_array() {
            named.extend(list.iter().filter_map(toml::Value::as_str).map(str::to_owned));
        }
        for entry in named {
            if let Some(found) = TEST_FEATURES
                .iter()
                .find(|test| entry == **test || entry.ends_with(&format!("/{test}")))
            {
                out.push(Violation {
                    gate: "artifact",
                    location: PRODUCT.to_owned(),
                    rule: "the shipped binary enables no feature that exists to build test \
                           scaffolding"
                        .to_owned(),
                    violation: format!("feature `{name}` reaches `{found}` through `{entry}`"),
                    alternative: "enable it from the test that needs it - `--all-features` \
                                  is the build every suite compiles in, and the release \
                                  build is the one where none of them exists"
                        .to_owned(),
                });
            }
        }
    }
    Ok(out)
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

    fn found(source: &str) -> Vec<Violation> {
        let parsed = syn::parse_file(source).unwrap();
        let mut out = Vec::new();
        walk_items("crates/x/src/y.rs", &parsed.items, &mut out);
        out
    }

    /// The defect this gate was written for, in the shape it had.
    #[test]
    fn an_unguarded_conformance_suite_is_refused() {
        let out = found("pub fn assert_port_conformance() {}\n");
        assert_eq!(out.len(), 1, "{out:#?}");
        assert!(out[0].violation.contains("conformance"));
    }

    /// The shape four of the five suites already had.
    #[test]
    fn the_same_suite_behind_a_feature_is_accepted() {
        assert!(
            found("#[cfg(feature = \"conformance\")]\npub mod conformance {}\n").is_empty()
        );
    }

    /// A re-export puts an item on the public surface as surely as
    /// declaring it does, and `browser::lib` is where the leak was
    /// visible.
    #[test]
    fn a_re_export_is_a_declaration_for_this_purpose() {
        let out = found("pub use port::{BrowserPort, assert_port_conformance};\n");
        assert_eq!(out.len(), 1, "{out:#?}");
        assert!(
            found("#[cfg(feature = \"conformance\")]\npub use port::assert_port_conformance;\n")
                .is_empty()
        );
    }

    /// A trait that documents which suite holds its adapters is not a
    /// trait that ships one. Judging by content rather than by name
    /// would convict every such doc comment.
    #[test]
    fn documenting_a_suite_is_not_shipping_one() {
        let source = "\
            /// Adapters are held to [`assert_port_conformance`].\n\
            pub trait BrowserPort {}\n";
        assert!(found(source).is_empty());
    }

    /// Everything inside a `#[cfg(test)]` module is already outside the
    /// product build, and a test double is supposed to be named like one.
    #[test]
    fn a_double_inside_a_test_module_is_left_alone() {
        let source = "#[cfg(test)]\nmod tests {\n    pub fn mock_clock() {}\n}\n";
        assert!(found(source).is_empty());
    }

    /// The repository passes the check it ships.
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
