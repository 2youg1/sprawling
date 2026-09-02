// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The performance ratchet: a number may improve freely, and may drift
//! back only within a stated slack.
//!
//! The register is `xtask/budgets.toml`, which holds every budget the
//! design states rather than only the ones a machine can check. A row it
//! cannot check says what it needs; an entry that quietly vanished
//! because nobody could measure it is how a budget stops existing.
//!
//! Only what a machine measures the same way twice is gated here. Sizes
//! qualify. Wall-clock figures do not: gating them would turn a busy
//! runner into a defect report, and the register says so per row.

use std::path::{Path, PathBuf};

use crate::report::{Violation, XtaskError};

/// The register, as the gate reads it.
struct Row {
    name: String,
    budget_bytes: u64,
    best_bytes: u64,
    slack_bytes: u64,
}

/// The register, parsed. Shared with `badge`, which renders the same
/// readings: two parsers would be two answers to "how big is it".
pub(crate) fn register(root: &Path) -> Result<toml::Value, XtaskError> {
    let path = root.join("xtask").join("budgets.toml");
    let text = std::fs::read_to_string(&path).map_err(|source| XtaskError::Io {
        path: path.display().to_string(),
        source,
    })?;
    toml::from_str(&text).map_err(|err| XtaskError::Doc {
        file: path.display().to_string(),
        msg: format!("the performance register does not parse: {err}"),
    })
}

pub(crate) fn check(root: &Path) -> Result<Vec<Violation>, XtaskError> {
    let parsed = register(root)?;

    let mut violations = crate::badge::check(root)?;
    // The single-binary promise, checked as bytes: a release binary that
    // exists must carry the client bundle's file table. The placeholder
    // build (no `just build-web` beforehand) lacks the `web_bg.wasm`
    // entry, and a binary like that must not look shippable.
    if let Some(binary) = binary_path(root)
        && !carries_client(&binary)?
    {
        violations.push(Violation {
            gate: "budget",
            location: binary.display().to_string(),
            rule: "a release binary carries the web client inside itself".to_owned(),
            violation: "the embedded file table has no web_bg.wasm: this binary would serve an \
                        empty page"
                .to_owned(),
            alternative: "run `just build-web`, then rebuild the release binary".to_owned(),
        });
    }
    for row in gated_rows(&parsed) {
        let Some(measured) = measure(root, &row.name)? else {
            // Nothing built to weigh. Silence rather than a violation:
            // `just check` does not build a release binary or a wasm
            // bundle, and a gate that demanded them would be a gate
            // people learn to run with less.
            continue;
        };
        if measured > row.budget_bytes {
            violations.push(Violation {
                gate: "budget",
                location: format!("{} is {measured} B", row.name),
                rule: "a reading stays inside the budget the design states".to_owned(),
                violation: format!("{measured} B exceeds the {} B budget", row.budget_bytes),
                alternative:
                    "make it smaller, or change the budget in the design and say why in the same \
                     change-set"
                        .to_owned(),
            });
            continue;
        }
        let ceiling = row.best_bytes.saturating_add(row.slack_bytes);
        if row.best_bytes > 0 && measured > ceiling {
            violations.push(Violation {
                gate: "budget",
                location: format!("{} is {measured} B", row.name),
                rule: "a number may improve freely and drift only within its slack".to_owned(),
                violation: format!(
                    "{measured} B is more than {} B worse than the best recorded {} B",
                    row.slack_bytes, row.best_bytes
                ),
                alternative: "recover the size, or record the new reading in xtask/budgets.toml \
                              with the reason it moved"
                    .to_owned(),
            });
        }
    }
    Ok(violations)
}

/// Reports every row: what it is, what it costs today, and what it is
/// allowed to cost. A gate that only speaks when it is unhappy leaves a
/// person guessing whether it measured anything at all.
pub(crate) fn report(root: &Path) -> Result<String, XtaskError> {
    let parsed = register(root)?;
    let mut out = String::from(
        "budget                 reading      best      budget
",
    );
    for row in gated_rows(&parsed) {
        let reading = match measure(root, &row.name)? {
            Some(bytes) => bytes.to_string(),
            None => "not built".to_owned(),
        };
        out.push_str(&format!(
            "{:<22} {:>9}  {:>9}  {:>10}
",
            row.name, reading, row.best_bytes, row.budget_bytes
        ));
    }
    let Some(table) = parsed.as_table() else {
        return Ok(out);
    };
    for (name, value) in table {
        if value.get("budget_bytes").is_some() {
            continue; // already weighed in the table above
        }
        let status = value
            .get("status")
            .and_then(toml::Value::as_str)
            .unwrap_or("");
        // A budget stated in something other than bytes still belongs in
        // the report: one that only the gate can see is one nobody
        // remembers is there.
        let stated = value
            .get("budget_lines")
            .and_then(toml::Value::as_integer)
            .map_or_else(String::new, |lines| format!("{lines} lines, "));
        out.push_str(&format!(
            "{name}: {stated}{status}
"
        ));
    }
    Ok(out)
}

/// The rows a machine can weigh: those with a byte budget.
fn gated_rows(parsed: &toml::Value) -> Vec<Row> {
    let mut rows = Vec::new();
    let Some(table) = parsed.as_table() else {
        return rows;
    };
    for (name, value) in table {
        if value.get("status").and_then(toml::Value::as_str) != Some("gated") {
            continue;
        }
        let number = |key: &str| value.get(key).and_then(toml::Value::as_integer);
        let (Some(budget), Some(best), Some(slack)) = (
            number("budget_bytes"),
            number("best_bytes"),
            number("slack_bytes"),
        ) else {
            continue;
        };
        rows.push(Row {
            name: name.clone(),
            budget_bytes: budget.unsigned_abs(),
            best_bytes: best.unsigned_abs(),
            slack_bytes: slack.unsigned_abs(),
        });
    }
    rows
}

/// Weighs one metric, or says it is not built.
pub(crate) fn measure(root: &Path, name: &str) -> Result<Option<u64>, XtaskError> {
    match name {
        "frontend_artifact" => gzipped_total(&root.join("target").join("web-dist")),
        "release_binary" => Ok(binary_bytes(root)),
        // A gated row with no way to weigh it would silently pass; it is
        // an unmeasured row until this match learns it.
        _ => Ok(None),
    }
}

/// What the browser downloads: every file in the bundle, compressed.
/// Recursive, because the bundle nests (`snippets/<crate>/...`), and a
/// weight that skipped subdirectories would flatter the artifact.
fn gzipped_total(dist: &Path) -> Result<Option<u64>, XtaskError> {
    let Ok(entries) = std::fs::read_dir(dist) else {
        return Ok(None);
    };
    let mut total = 0u64;
    let mut found = false;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Some(nested) = gzipped_total(&path)? {
                found = true;
                total = total.saturating_add(nested);
            }
            continue;
        }
        let bytes = std::fs::read(&path).map_err(|source| XtaskError::Io {
            path: path.display().to_string(),
            source,
        })?;
        found = true;
        total = total.saturating_add(gzipped_len(&bytes));
    }
    Ok(found.then_some(total))
}

/// Naive subsequence search; the haystack is read once per gate run.
fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty() && haystack.windows(needle.len()).any(|w| w == needle)
}

/// The compressed length, computed rather than shelled out for, so the
/// number does not depend on which gzip is on the path.
fn gzipped_len(bytes: &[u8]) -> u64 {
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::best());
    use std::io::Write as _;
    if encoder.write_all(bytes).is_err() {
        return u64::MAX;
    }
    match encoder.finish() {
        Ok(compressed) => u64::try_from(compressed.len()).unwrap_or(u64::MAX),
        Err(_) => u64::MAX,
    }
}

fn binary_bytes(root: &Path) -> Option<u64> {
    let path = binary_path(root)?;
    std::fs::metadata(path).ok().map(|meta| meta.len())
}

/// Whether a built binary carries the real client or only the page
/// shell. One statement of that fact, because the gate refuses on it and
/// so does `package`, and two readings would drift apart.
///
/// # Errors
/// Propagates the failure of reading the binary: a release artifact that
/// cannot be read is a finding, not a thing to skip over.
pub(crate) fn carries_client(binary: &Path) -> Result<bool, XtaskError> {
    let bytes = std::fs::read(binary).map_err(|source| XtaskError::Io {
        path: binary.display().to_string(),
        source,
    })?;
    Ok(contains(&bytes, b"web_bg.wasm"))
}

pub(crate) fn binary_path(root: &Path) -> Option<PathBuf> {
    for name in ["sprawling", "sprawling.exe"] {
        let path: PathBuf = root.join("target").join("release").join(name);
        if path.is_file() {
            return Some(path);
        }
    }
    None
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

    #[test]
    fn only_rows_marked_gated_are_weighed() {
        let register: toml::Value = toml::from_str(
            r#"
            [gated_one]
            budget_bytes = 100
            best_bytes = 50
            slack_bytes = 10
            status = "gated"

            [measured_only]
            budget_bytes = 100
            best_bytes = 50
            slack_bytes = 10
            status = "measured, not gated: the counter differs per platform"

            [no_bytes]
            budget_ms = 5
            status = "gated"
            "#,
        )
        .unwrap();
        let rows = gated_rows(&register);
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "gated_one");
        assert_eq!(rows[0].budget_bytes, 100);
    }

    #[test]
    fn a_metric_this_gate_cannot_weigh_reports_nothing_rather_than_passing() {
        let root = std::env::temp_dir();
        assert_eq!(measure(&root, "ledger_append").unwrap(), None);
        assert_eq!(measure(&root, "frontend_artifact").unwrap(), None);
    }

    #[test]
    fn the_shipped_register_names_every_budget_and_each_says_how_it_is_measured() {
        let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
        let text = std::fs::read_to_string(root.join("xtask").join("budgets.toml")).unwrap();
        let register: toml::Value = toml::from_str(&text).unwrap();
        let table = register.as_table().unwrap();
        assert_eq!(table.len(), 11, "the design states eleven budgets");
        for (name, row) in table {
            assert!(
                row.get("status").and_then(toml::Value::as_str).is_some(),
                "{name} does not say whether it is gated"
            );
            assert!(
                row.get("measured_by")
                    .and_then(toml::Value::as_str)
                    .is_some(),
                "{name} does not say how it is measured"
            );
        }
    }

    #[test]
    fn compression_is_ours_rather_than_whatever_gzip_is_on_the_path() {
        assert!(gzipped_len(b"") > 0);
        let repetitive = vec![b'a'; 10_000];
        assert!(gzipped_len(&repetitive) < 1_000);
    }

    #[test]
    fn the_bundle_weight_counts_nested_directories() {
        let dir = std::env::temp_dir().join(format!("budget-walk-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("snippets").join("x")).unwrap();
        std::fs::write(dir.join("web.js"), vec![b'a'; 4_000]).unwrap();
        std::fs::write(
            dir.join("snippets").join("x").join("y.js"),
            vec![b'b'; 4_000],
        )
        .unwrap();
        let total = gzipped_total(&dir).unwrap().unwrap();
        let flat = gzipped_len(&vec![b'a'; 4_000]);
        assert!(total > flat, "the nested file must be weighed too");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_binary_without_the_client_table_is_named_by_the_gate() {
        assert!(contains(b"...web_bg.wasm...", b"web_bg.wasm"));
        assert!(!contains(b"a placeholder build", b"web_bg.wasm"));
    }
}
