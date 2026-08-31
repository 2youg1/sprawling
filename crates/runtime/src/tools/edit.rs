// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The edit tool: optimistic concurrency against a version the caller
//! must already have seen.
//!
//! Two refusals carry the design. A `base_version` that no longer
//! matches means the file moved under the caller — a blind overwrite
//! would erase whoever moved it, so the edit is refused with the
//! current version named. And an `old` string that matches zero times
//! or many times is refused with the count, because "replace the first
//! one" is a guess about which occurrence was meant.
//!
//! The echo is a unified diff. Per-edit diffs are the restoration
//! granularity: a checkpoint restores a wave, but a diff restores one
//! change, and that is the difference between undoing a mistake and
//! undoing an afternoon.

use std::path::{Path, PathBuf};

use kernel::{
    AxCode, AxError, B3Hash, CostTier, Effect, Payload, RenderIntent, Temporal, Tool, ToolCall,
    ToolMeta, ToolName, ToolOutcome,
};
use serde_json::{Map, Value};

pub struct EditTool {
    city_root: PathBuf,
    /// The run's write domain. Every model-chosen path is judged against
    /// it before the filesystem is touched.
    writable: kernel::WriteDomain,
    meta: ToolMeta,
}

/// The version stamp a caller must present: the first 16 hex of the
/// content hash. Short enough to read aloud, long enough that a
/// collision is not the failure mode anyone will meet.
pub fn version_of(bytes: &[u8]) -> String {
    let full = B3Hash::digest(bytes).to_string();
    full.get(..16).unwrap_or(&full).to_owned()
}

/// The version word that means "I expect this file to not exist yet".
/// Sixteen hex digits can never spell it, so it cannot collide with a
/// real version.
const CREATES: &str = "new";

impl EditTool {
    pub fn new(
        city_root: &Path,
        domain: kernel::Address,
        writable: kernel::WriteDomain,
    ) -> Result<EditTool, AxError> {
        let mut params = Map::new();
        params.insert("type".to_owned(), Value::String("object".to_owned()));
        let mut properties = Map::new();
        for (field, description) in [
            ("path", "file to edit, relative to the city root"),
            (
                "base_version",
                "version you last saw; refuses if it moved. Pass \"new\" to create the file",
            ),
            (
                "old",
                "exact text to replace; must match exactly once. \"\" when creating",
            ),
            ("new", "replacement text, or the whole file when creating"),
        ] {
            let mut spec = Map::new();
            spec.insert("type".to_owned(), Value::String("string".to_owned()));
            spec.insert(
                "description".to_owned(),
                Value::String(description.to_owned()),
            );
            properties.insert(field.to_owned(), Value::Object(spec));
        }
        params.insert("properties".to_owned(), Value::Object(properties));
        params.insert(
            "required".to_owned(),
            Value::Array(
                ["path", "base_version", "old", "new"]
                    .into_iter()
                    .map(|f| Value::String(f.to_owned()))
                    .collect(),
            ),
        );
        Ok(EditTool {
            city_root: city_root.to_path_buf(),
            writable,
            meta: ToolMeta {
                name: ToolName::parse("edit")?,
                disclosure:
                    "Replace an exact string in a file, guarded by the version you last saw."
                        .to_owned(),
                params: Payload::new(params)?,
                effect: Effect::Write { domain },
                cost_tier: CostTier::Light,
                timeout: None,
                render: RenderIntent::Diff {
                    locations: Vec::new(),
                },
                temporal: Temporal::Timeless,
            },
        })
    }
}

fn arg<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str, AxError> {
    args.get(key).and_then(Value::as_str).ok_or_else(|| {
        AxError::failure(
            AxCode::InvalidArgs,
            "edit file",
            format!("missing string argument `{key}`"),
        )
        .with_recovery("pass path, base_version, old and new - all strings")
    })
}

impl Tool for EditTool {
    fn meta(&self) -> &ToolMeta {
        &self.meta
    }

    fn invoke(&mut self, call: &ToolCall) -> Result<ToolOutcome, AxError> {
        if call.name != self.meta.name {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "edit file",
                format!("call routed to the wrong tool: {}", call.name.as_str()),
            ));
        }
        let args = call.args.as_map();
        let rel = arg(args, "path")?;
        let base_version = arg(args, "base_version")?;
        let old = arg(args, "old")?;
        let new = arg(args, "new")?;

        // The path is judged before the filesystem is touched, and by the
        // same two authorities everything else uses: Address::parse kills
        // traversal, WriteDomain::admits kills everything outside the
        // run's declared prefixes (reserved space included). Without this
        // the write gate only ever saw the tool's own declaration, and a
        // model-chosen path went wherever it pointed.
        let target = kernel::Address::parse(rel).map_err(|err| {
            AxError::failure(
                AxCode::InvalidArgs,
                "edit file",
                format!("{rel}: {}", err.subject()),
            )
            .with_recovery("pass a city-relative path with no `..` and no leading slash")
        })?;
        if let kernel::DomainVerdict::Outside { prefixes } = self.writable.admits(&target) {
            return Err(AxError::failure(
                AxCode::OutsideWriteDomain,
                "edit file",
                format!("{rel} is outside this run's write domain"),
            )
            .with_recovery(format!("write under: {}", prefixes.join(", "))));
        }

        let path = self.city_root.join(rel);
        if base_version == CREATES {
            return self.create(rel, &path, old, new);
        }
        let bytes = std::fs::read(&path).map_err(|err| {
            AxError::failure(AxCode::InvalidArgs, "edit file", format!("{rel}: {err}"))
                .with_recovery(
                    "check the path, relative to the city root; to create a new file pass \
                     base_version:\"new\"",
                )
        })?;
        let current_version = version_of(&bytes);
        if current_version != base_version {
            return Err(AxError::failure(
                AxCode::VersionConflict,
                "edit file",
                format!("{rel} is at {current_version}, not {base_version}"),
            )
            .with_recovery("read the file again, then edit against the version you just saw"));
        }
        let text = String::from_utf8(bytes).map_err(|_| {
            AxError::failure(
                AxCode::InvalidArgs,
                "edit file",
                format!("{rel} is not valid UTF-8"),
            )
        })?;
        let hits = text.matches(old).count();
        if hits != 1 {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "edit file",
                format!("`old` matches {hits} times in {rel}, not once"),
            )
            .with_recovery("include enough surrounding text to name exactly one occurrence"));
        }
        let updated = text.replacen(old, new, 1);
        std::fs::write(&path, updated.as_bytes()).map_err(|err| {
            AxError::failure(AxCode::StorageFatal, "edit file", format!("{rel}: {err}"))
        })?;
        let new_version = version_of(updated.as_bytes());

        let mut result = Map::new();
        result.insert("path".to_owned(), Value::String(rel.to_owned()));
        result.insert(
            "base_version".to_owned(),
            Value::String(base_version.to_owned()),
        );
        result.insert("new_version".to_owned(), Value::String(new_version));
        result.insert(
            "diff".to_owned(),
            Value::String(unified_diff(rel, &text, &updated)),
        );
        Ok(ToolOutcome {
            result: Payload::new(result)?,
        })
    }
}

impl EditTool {
    /// The creating arm: the caller declared the file absent, so an
    /// existing file is a version conflict like any other - it names the
    /// version the file is really at.
    fn create(&self, rel: &str, path: &Path, old: &str, new: &str) -> Result<ToolOutcome, AxError> {
        if let Ok(existing) = std::fs::read(path) {
            return Err(AxError::failure(
                AxCode::VersionConflict,
                "edit file",
                format!("{rel} already exists at {}, not new", version_of(&existing)),
            )
            .with_recovery("read the file and edit against that version, or choose another path"));
        }
        if !old.is_empty() {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "edit file",
                "creating a file replaces nothing: `old` must be \"\"",
            )
            .with_recovery("pass old:\"\" and the whole file in `new`"));
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).map_err(|err| {
                AxError::failure(AxCode::StorageFatal, "edit file", format!("{rel}: {err}"))
            })?;
        }
        std::fs::write(path, new.as_bytes()).map_err(|err| {
            AxError::failure(AxCode::StorageFatal, "edit file", format!("{rel}: {err}"))
        })?;
        let mut result = Map::new();
        result.insert("path".to_owned(), Value::String(rel.to_owned()));
        result.insert("base_version".to_owned(), Value::String(CREATES.to_owned()));
        result.insert(
            "new_version".to_owned(),
            Value::String(version_of(new.as_bytes())),
        );
        result.insert("diff".to_owned(), Value::String(unified_diff(rel, "", new)));
        Ok(ToolOutcome {
            result: Payload::new(result)?,
        })
    }
}

/// A minimal unified diff: enough to see what changed and where, and no
/// dependency for the eighty lines it takes.
fn unified_diff(path: &str, before: &str, after: &str) -> String {
    let old_lines: Vec<&str> = before.lines().collect();
    let new_lines: Vec<&str> = after.lines().collect();
    let prefix = common_prefix(&old_lines, &new_lines);
    let suffix = common_suffix(&old_lines, &new_lines, prefix);
    let old_end = old_lines.len().saturating_sub(suffix);
    let new_end = new_lines.len().saturating_sub(suffix);

    let mut out = format!("--- a/{path}\n+++ b/{path}\n");
    let old_count = old_end.saturating_sub(prefix);
    let new_count = new_end.saturating_sub(prefix);
    out.push_str(&format!(
        "@@ -{},{} +{},{} @@\n",
        prefix.saturating_add(1),
        old_count,
        prefix.saturating_add(1),
        new_count
    ));
    for line in old_lines.get(prefix..old_end).unwrap_or_default() {
        out.push_str(&format!("-{line}\n"));
    }
    for line in new_lines.get(prefix..new_end).unwrap_or_default() {
        out.push_str(&format!("+{line}\n"));
    }
    out
}

fn common_prefix(old: &[&str], new: &[&str]) -> usize {
    let mut i = 0usize;
    while let (Some(a), Some(b)) = (old.get(i), new.get(i)) {
        if a != b {
            break;
        }
        i = i.saturating_add(1);
    }
    i
}

fn common_suffix(old: &[&str], new: &[&str], prefix: usize) -> usize {
    let mut i = 0usize;
    loop {
        let old_left = old.len().saturating_sub(i);
        let new_left = new.len().saturating_sub(i);
        if old_left <= prefix || new_left <= prefix {
            break;
        }
        let (Some(a), Some(b)) = (
            old.get(old_left.saturating_sub(1)),
            new.get(new_left.saturating_sub(1)),
        ) else {
            break;
        };
        if a != b {
            break;
        }
        i = i.saturating_add(1);
    }
    i
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
    use kernel::Address;

    fn tool(root: &Path) -> EditTool {
        let work = Address::parse("work").unwrap();
        let domain = kernel::WriteDomain::new(vec![work.clone()]).unwrap();
        std::fs::create_dir_all(root.join("work")).unwrap();
        EditTool::new(root, work, domain).unwrap()
    }

    fn call(path: &str, base: &str, old: &str, new: &str) -> ToolCall {
        let mut args = Map::new();
        for (k, v) in [
            ("path", path),
            ("base_version", base),
            ("old", old),
            ("new", new),
        ] {
            args.insert(k.to_owned(), Value::String(v.to_owned()));
        }
        ToolCall {
            id: "call-1".to_owned(),
            name: ToolName::parse("edit").unwrap(),
            args: Payload::new(args).unwrap(),
        }
    }

    #[test]
    fn an_edit_against_the_version_it_saw_lands_and_echoes_a_diff() {
        let tmp = tempfile::tempdir().unwrap();
        let mut tool = tool(tmp.path());
        std::fs::write(tmp.path().join("work/a.txt"), "one\ntwo\nthree\n").unwrap();
        let version = version_of(b"one\ntwo\nthree\n");
        let outcome = tool
            .invoke(&call("work/a.txt", &version, "two", "TWO"))
            .unwrap();
        let result = serde_json::to_value(&outcome.result).unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("work/a.txt")).unwrap(),
            "one\nTWO\nthree\n"
        );
        let diff = result["diff"].as_str().unwrap();
        assert!(diff.contains("-two"), "{diff}");
        assert!(diff.contains("+TWO"), "{diff}");
        assert_eq!(
            result["new_version"].as_str().unwrap(),
            version_of(b"one\nTWO\nthree\n")
        );
    }

    #[test]
    fn a_file_that_moved_refuses_and_names_the_version_it_is_at_now() {
        let tmp = tempfile::tempdir().unwrap();
        let mut tool = tool(tmp.path());
        std::fs::write(tmp.path().join("work/a.txt"), "current\n").unwrap();
        let err = match tool.invoke(&call("work/a.txt", "0000000000000000", "current", "next")) {
            Err(err) => err,
            Ok(_) => panic!("a stale base_version must refuse"),
        };
        assert_eq!(*err.code(), AxCode::VersionConflict);
        assert!(err.subject().contains(&version_of(b"current\n")));
        // The file is untouched: a refused edit changes nothing.
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("work/a.txt")).unwrap(),
            "current\n"
        );
    }

    #[test]
    fn base_version_new_creates_the_file_and_parents() {
        let tmp = tempfile::tempdir().unwrap();
        let mut tool = tool(tmp.path());
        let outcome = tool
            .invoke(&call("work/room/notes.md", "new", "", "first line\n"))
            .unwrap();
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("work/room/notes.md")).unwrap(),
            "first line\n"
        );
        let result = serde_json::to_value(&outcome.result).unwrap();
        assert_eq!(
            result["new_version"].as_str().unwrap(),
            version_of(b"first line\n")
        );
        assert!(result["diff"].as_str().unwrap().contains("+first line"));
    }

    #[test]
    fn creating_over_an_existing_file_is_a_version_conflict_naming_the_real_version() {
        let tmp = tempfile::tempdir().unwrap();
        let mut tool = tool(tmp.path());
        std::fs::write(tmp.path().join("work/a.txt"), "already\n").unwrap();
        let err = match tool.invoke(&call("work/a.txt", "new", "", "other\n")) {
            Err(err) => err,
            Ok(_) => panic!("an existing file must refuse creation"),
        };
        assert_eq!(*err.code(), AxCode::VersionConflict);
        assert!(err.subject().contains(&version_of(b"already\n")));
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("work/a.txt")).unwrap(),
            "already\n"
        );
    }

    #[test]
    fn creating_with_a_nonempty_old_is_refused_with_the_form_to_use() {
        let tmp = tempfile::tempdir().unwrap();
        let mut tool = tool(tmp.path());
        let err = match tool.invoke(&call("work/b.txt", "new", "something", "content")) {
            Err(err) => err,
            Ok(_) => panic!("a create replaces nothing"),
        };
        assert_eq!(*err.code(), AxCode::InvalidArgs);
        assert!(err.recovery().contains("old:\"\""));
    }

    #[test]
    fn a_path_outside_the_write_domain_is_refused_before_the_disk_is_touched() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("outside.txt"), "x").unwrap();
        let mut tool = tool(tmp.path());
        for hostile in ["outside.txt", "elsewhere/f.md", ".sprawling/ledger/x"] {
            let err = match tool.invoke(&call(hostile, "new", "", "y")) {
                Err(err) => err,
                Ok(_) => panic!("{hostile} must be refused"),
            };
            assert_eq!(*err.code(), AxCode::OutsideWriteDomain, "{hostile}");
            assert!(err.recovery().contains("work"), "{hostile}: {err}");
        }
        for illegal in ["../evil.txt", "/abs.txt", "work//x"] {
            let err = match tool.invoke(&call(illegal, "new", "", "y")) {
                Err(err) => err,
                Ok(_) => panic!("{illegal} must be refused"),
            };
            assert_eq!(*err.code(), AxCode::InvalidArgs, "{illegal}");
        }
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("outside.txt")).unwrap(),
            "x"
        );
    }

    #[test]
    fn a_missing_file_names_the_create_form_in_its_recovery() {
        let tmp = tempfile::tempdir().unwrap();
        let mut tool = tool(tmp.path());
        let version = version_of(b"whatever");
        let err = match tool.invoke(&call("work/absent.md", &version, "a", "b")) {
            Err(err) => err,
            Ok(_) => panic!("a missing file must refuse a non-create edit"),
        };
        assert!(err.recovery().contains("base_version:\"new\""), "{err}");
    }

    #[test]
    fn zero_or_many_matches_refuse_with_the_count() {
        let tmp = tempfile::tempdir().unwrap();
        let mut tool = tool(tmp.path());
        std::fs::write(tmp.path().join("work/a.txt"), "x\nx\n").unwrap();
        let version = version_of(b"x\nx\n");
        let err = match tool.invoke(&call("work/a.txt", &version, "x", "y")) {
            Err(err) => err,
            Ok(_) => panic!("an ambiguous match must refuse"),
        };
        assert_eq!(*err.code(), AxCode::InvalidArgs);
        assert!(err.subject().contains("2 times"), "{}", err.subject());

        let err = match tool.invoke(&call("work/a.txt", &version, "absent", "y")) {
            Err(err) => err,
            Ok(_) => panic!("a missing match must refuse"),
        };
        assert!(err.subject().contains("0 times"), "{}", err.subject());
    }

    #[test]
    fn a_call_for_another_tool_is_refused_not_routed() {
        let tmp = tempfile::tempdir().unwrap();
        let mut tool = tool(tmp.path());
        let mut wrong = call("work/a.txt", "v", "a", "b");
        wrong.name = ToolName::parse("exec").unwrap();
        let err = match tool.invoke(&wrong) {
            Err(err) => err,
            Ok(_) => panic!("identity is fail-closed"),
        };
        assert_eq!(*err.code(), AxCode::InvalidArgs);
    }
}
