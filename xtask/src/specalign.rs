// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Gate: kernel enums and the kernel-SPEC tables agree variant by
//! variant (C8). The gate consumes the real enums — `AxCode::ALL` and
//! `EventKind::ALL` — and reads the SPEC as data, so "the table drifted"
//! and "the enum grew silently" are the same red. Asserts: every AxCode
//! appears exactly once in the 8-1 table with its declared carrier;
//! every EventKind exactly once in the 8-4 table with its window class.

use std::collections::BTreeMap;
use std::path::Path;

use kernel::{AxCode, Carrier, EventKind, WindowClass};

use crate::report::{Violation, XtaskError};
use crate::walk;

const SPEC_PATH: &str = "crates/kernel/kernel-SPEC.md";

fn violation(rule: &str, violation: String, alternative: &str) -> Violation {
    Violation {
        gate: "specalign",
        location: SPEC_PATH.to_owned(),
        rule: rule.to_owned(),
        violation,
        alternative: alternative.to_owned(),
    }
}

/// Splits a pipe-table line into trimmed cells, or None for non-rows.
fn cells(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    let inner = trimmed.strip_prefix('|')?.strip_suffix('|')?;
    Some(inner.split('|').map(|c| c.trim().to_owned()).collect())
}

fn unticked(cell: &str) -> Option<&str> {
    cell.strip_prefix('`')?.strip_suffix('`')
}

/// The carrier cell a code must show, derived from the enum itself.
fn expected_carrier(code: AxCode) -> Result<String, XtaskError> {
    match code.carrier() {
        Carrier::Loadtime => Ok("loadtime".to_owned()),
        Carrier::Event(kind) => serde_json::to_value(kind)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .ok_or_else(|| XtaskError::Doc {
                file: "kernel enums".to_owned(),
                msg: "EventKind must serialize to a string".to_owned(),
            }),
    }
}

/// Normalizes a SPEC carrier cell: "`tool_result`" -> tool_result,
/// "装载期（无 carrier）" -> loadtime.
fn spec_carrier(cell: &str) -> String {
    if let Some(name) = unticked(cell) {
        name.to_owned()
    } else if cell.contains("carrier") || cell.contains("\u{88c5}\u{8f7d}\u{671f}") {
        "loadtime".to_owned()
    } else {
        cell.to_owned()
    }
}

pub(crate) fn check(root: &Path) -> Result<Vec<Violation>, XtaskError> {
    let text = walk::read_text(&root.join(SPEC_PATH))?;
    let mut ax_rows: BTreeMap<String, String> = BTreeMap::new();
    let mut kind_rows: BTreeMap<String, String> = BTreeMap::new();
    let mut violations = Vec::new();

    for line in text.lines() {
        let Some(row) = cells(line) else { continue };
        let (Some(second), third) = (row.get(1), row.get(2)) else {
            continue;
        };
        let Some(name) = unticked(second) else {
            continue;
        };
        let Some(third) = third else { continue };
        if name.starts_with("E_") {
            if ax_rows
                .insert(name.to_owned(), spec_carrier(third))
                .is_some()
            {
                violations.push(violation(
                    "each AxCode owns exactly one carrier row (C9)",
                    format!("`{name}` appears more than once in the 8-1 table"),
                    "delete the duplicate row",
                ));
            }
        } else if third.contains("in-window") || third.contains("record-only") {
            let class = if third.contains("in-window") {
                "in-window"
            } else {
                "record-only"
            };
            if kind_rows
                .insert(name.to_owned(), class.to_owned())
                .is_some()
            {
                violations.push(violation(
                    "each EventKind belongs to exactly one partition (3.1)",
                    format!("`{name}` appears more than once in the 8-4 table"),
                    "delete the duplicate row",
                ));
            }
        }
    }

    for code in AxCode::ALL {
        let spelling = code.as_str();
        match ax_rows.remove(spelling) {
            None => violations.push(violation(
                "every AxCode variant has its 8-1 table row (C8)",
                format!("`{spelling}` is in the enum but not in the table"),
                "add the row (and its carrier) in the same change-set as the variant",
            )),
            Some(cell) => {
                let expected = expected_carrier(code)?;
                if cell != expected {
                    violations.push(violation(
                        "the table carrier equals the enum's declaration (C9)",
                        format!("`{spelling}`: table says `{cell}`, enum says `{expected}`"),
                        "fix whichever side no longer matches the other",
                    ));
                }
            }
        }
    }
    for (orphan, _) in ax_rows {
        violations.push(violation(
            "the 8-1 table lists enum variants only (C8)",
            format!("`{orphan}` is in the table but not in the enum"),
            "remove the row or add the variant in the same change-set",
        ));
    }

    for kind in EventKind::ALL {
        let value = serde_json::to_value(kind).map_err(|err| XtaskError::Doc {
            file: "kernel enums".to_owned(),
            msg: format!("EventKind serialization failed: {err}"),
        })?;
        let Some(spelling) = value.as_str().map(str::to_owned) else {
            return Err(XtaskError::Doc {
                file: "kernel enums".to_owned(),
                msg: "EventKind must serialize to a string".to_owned(),
            });
        };
        let expected = match kind.window_class() {
            WindowClass::InWindow => "in-window",
            WindowClass::RecordOnly => "record-only",
        };
        match kind_rows.remove(&spelling) {
            None => violations.push(violation(
                "every EventKind variant has its 8-4 table row (C8)",
                format!("`{spelling}` is in the enum but not in the table"),
                "add the row (and its partition) in the same change-set as the variant",
            )),
            Some(class) => {
                if class != expected {
                    violations.push(violation(
                        "the table partition equals the enum's window class (3.1)",
                        format!("`{spelling}`: table says {class}, enum says {expected}"),
                        "fix whichever side no longer matches the other",
                    ));
                }
            }
        }
    }
    for (orphan, _) in kind_rows {
        violations.push(violation(
            "the 8-4 table lists enum variants only (C8)",
            format!("`{orphan}` is in the table but not in the enum"),
            "remove the row or add the variant in the same change-set",
        ));
    }

    Ok(violations)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, reason = "test code")]
mod tests {
    use super::{cells, spec_carrier, unticked};

    #[test]
    fn table_rows_split_and_untick() {
        let row = cells("| a | `E_TIMEOUT` | `tool_result` |").unwrap();
        assert_eq!(row.len(), 3);
        assert_eq!(unticked(row.get(1).unwrap()).unwrap(), "E_TIMEOUT");
        assert!(cells("not a row").is_none());
        assert!(unticked("plain").is_none());
    }

    #[test]
    fn carrier_cells_normalize() {
        assert_eq!(spec_carrier("`gate_denied`"), "gate_denied");
        assert_eq!(spec_carrier("装载期（无 carrier）"), "loadtime");
    }
}
