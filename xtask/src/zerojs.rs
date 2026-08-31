// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Zero-JS gate (redline C1): the repo carries no JS/TS sources, and our own
//! command surfaces (justfile, CI steps, build scripts, shell scripts) never
//! invoke the node toolchain family. GitHub Actions infrastructure is not part
//! of the artifact build chain and is out of scope; wasm-bindgen glue is
//! generated under target/ and never enters the tree.

use std::path::Path;

use crate::report::{Violation, XtaskError};
use crate::walk;

const BANNED_EXTS: [&str; 8] = ["js", "mjs", "cjs", "jsx", "ts", "tsx", "mts", "cts"];
const NODE_TOKENS: [&str; 6] = ["npm", "npx", "node", "yarn", "pnpm", "bun"];

pub(crate) fn check(root: &Path) -> Result<Vec<Violation>, XtaskError> {
    let mut violations = Vec::new();

    for file in walk::files(root)? {
        let rel = walk::rel(root, &file);
        if walk::in_isolation_zone(&rel) {
            continue;
        }
        let ext = file
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or_default();
        if BANNED_EXTS.contains(&ext) {
            violations.push(Violation {
                gate: "zerojs",
                location: rel,
                rule: "all Rust, zero JS/TS in the tree (C1)".to_owned(),
                violation: format!("source file with banned extension .{ext}"),
                alternative: "write it in Rust; the web client compiles to wasm".to_owned(),
            });
            continue;
        }
        if is_command_surface(&rel) {
            scan_commands(&rel, &walk::read_text(&file)?, &mut violations);
        }
    }
    Ok(violations)
}

fn is_command_surface(rel: &str) -> bool {
    rel == "justfile"
        || rel.ends_with("build.rs")
        || rel.ends_with(".sh")
        || (rel.starts_with(".github/") && (rel.ends_with(".yml") || rel.ends_with(".yaml")))
}

fn scan_commands(location: &str, text: &str, out: &mut Vec<Violation>) {
    for (index, line) in text.lines().enumerate() {
        let trimmed = line.trim_start();
        // Comment lines cannot execute anything; judging them would turn
        // documentation into behaviour (xtask-SPEC.md section 11).
        if trimmed.starts_with('#') || trimmed.starts_with("//") {
            continue;
        }
        for token in tokens(line) {
            if NODE_TOKENS.contains(&token) {
                out.push(Violation {
                    gate: "zerojs",
                    location: format!("{location}:{}", index.saturating_add(1)),
                    rule: "our command surfaces never invoke the node family (C1)".to_owned(),
                    violation: format!("token {token:?} in a command surface"),
                    alternative: "use a Rust-toolchain equivalent (cargo, xtask)".to_owned(),
                });
            }
        }
    }
}

/// Tokens keep `_` so `node_modules` stays one token and does not
/// false-positive against `node` (xtask-SPEC.md section 16).
fn tokens(line: &str) -> impl Iterator<Item = &str> {
    line.split(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
        .filter(|t| !t.is_empty())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn node_modules_is_one_token() {
        let toks: Vec<&str> = tokens("ignore node_modules please").collect();
        assert!(toks.contains(&"node_modules"));
        assert!(!toks.contains(&"node"));
    }

    #[test]
    fn npm_invocation_is_caught() {
        let mut out = Vec::new();
        scan_commands("justfile", "build:\n\tnpm install\n", &mut out);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].location, "justfile:2");
    }

    #[test]
    fn comment_lines_are_not_command_surfaces() {
        let mut out = Vec::new();
        scan_commands("ci.yml", "# never invoke npm or node here\n", &mut out);
        scan_commands("build.rs", "// npm is banned\n", &mut out);
        assert!(out.is_empty());
    }
}
