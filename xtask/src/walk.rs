// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Deterministic file walker: sorted output, fixed skip set, forward-slash
//! relative paths. Determinism makes gate reports diffable across runs and
//! platforms (xtask-SPEC.md section 10-1).

use std::path::{Path, PathBuf};

use crate::report::XtaskError;

/// Directories a gate never walks into: version control, and the build
/// output of the three toolchains this tree can contain.
///
/// **A gate testifies about committed objects**, and a build directory
/// holds none - `.gitignore` names every one of these. `dist-newstyle`
/// joined the list when the adversarial checker landed and cabal began
/// writing its package database beside the source: the `secret` scan read
/// 108,472 findings out of it and the `release` scan nine more, all of
/// them true statements about generated files that no reader will ever
/// receive. A gate that reports a hundred thousand findings reports
/// nothing, because nobody reads the hundred thousand and first.
///
/// This list is a second authority for "what is in the tree", and git is
/// the first. It stays a list rather than a `.gitignore` reader because
/// four names cost four tokens and a parser costs a parser - but that is
/// the parameter: **the day this list needs a fifth entry that is not a
/// build directory, read the ignore file instead of adding a row.**
const SKIP_DIRS: [&str; 4] = [".git", "target", "node_modules", "dist-newstyle"];

/// All regular files under `root`, sorted by their relative forward-slash path.
pub(crate) fn files(root: &Path) -> Result<Vec<PathBuf>, XtaskError> {
    let mut out = Vec::new();
    collect(root, &mut out)?;
    out.sort_by_key(|p| rel(root, p));
    Ok(out)
}

/// Like [`files`], keeping only the given extensions (lowercase, no dot).
pub(crate) fn files_with_ext(root: &Path, keep: &[&str]) -> Result<Vec<PathBuf>, XtaskError> {
    let all = files(root)?;
    Ok(all
        .into_iter()
        .filter(|p| {
            p.extension()
                .and_then(|e| e.to_str())
                .is_some_and(|e| keep.contains(&e))
        })
        .collect())
}

/// Repo-relative path with forward slashes; used for all matching and reports
/// because the module table is written with forward slashes.
pub(crate) fn rel(root: &Path, path: &Path) -> String {
    let full = path.to_string_lossy().replace('\\', "/");
    let base = root.to_string_lossy().replace('\\', "/");
    match full.strip_prefix(&base) {
        Some(tail) => tail.trim_start_matches('/').to_owned(),
        None => full,
    }
}

/// The isolation zone: root-level `local/` holds handoffs and machine-local
/// notes, is gitignored, and never enters the tree. Gates testify about
/// committed objects only, so repo-root scans exclude it (xtask-SPEC 10-1).
pub(crate) fn in_isolation_zone(rel: &str) -> bool {
    rel == "local" || rel.starts_with("local/")
}

fn collect(dir: &Path, out: &mut Vec<PathBuf>) -> Result<(), XtaskError> {
    let entries = std::fs::read_dir(dir).map_err(|source| XtaskError::Io {
        path: dir.to_string_lossy().into_owned(),
        source,
    })?;
    for entry in entries {
        let entry = entry.map_err(|source| XtaskError::Io {
            path: dir.to_string_lossy().into_owned(),
            source,
        })?;
        let path = entry.path();
        let name = entry.file_name().to_string_lossy().into_owned();
        if !path.is_dir() {
            out.push(path);
        } else if !SKIP_DIRS.contains(&name.as_str()) {
            collect(&path, out)?;
        }
    }
    Ok(())
}

/// Read a file as (lossy) UTF-8; gates judge text, they never panic on bytes.
pub(crate) fn read_text(path: &Path) -> Result<String, XtaskError> {
    let bytes = std::fs::read(path).map_err(|source| XtaskError::Io {
        path: path.to_string_lossy().into_owned(),
        source,
    })?;
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::indexing_slicing)]
mod tests {
    use super::*;

    #[test]
    fn rel_normalizes_separators() {
        let root = Path::new("C:\\repo");
        let file = Path::new("C:\\repo\\crates\\kernel\\src\\lib.rs");
        assert_eq!(rel(root, file), "crates/kernel/src/lib.rs");
    }

    #[test]
    fn isolation_zone_is_the_local_prefix_only() {
        assert!(in_isolation_zone("local/Handoff.md"));
        assert!(in_isolation_zone("local"));
        assert!(!in_isolation_zone("localx/notes.md"));
        assert!(!in_isolation_zone("crates/kernel/src/lib.rs"));
    }
}
