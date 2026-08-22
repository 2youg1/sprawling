// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Spine semantics: the Roadmap table's shape, the
//! Memo outline's six fields, the tally that turns rows into progress,
//! and the KEEP/ADD/DROP vocabulary. Shape and truth are two doors:
//! `check_*` answers "is this written like a roadmap"; `tally` answers
//! "what actually counts" — a Done row without parsable evidence never
//! reaches the numerator (reconciliation rule 1).

use serde::{Deserialize, Serialize};

use crate::completion::{PlannedProgress, Progress};
use crate::error::{AxCode, AxError};
use crate::locator::Locator;

/// The five states, closed. The spellings below are the canonical table
/// strings, and there is exactly one set of them: a second set would be a
/// second authority over what a resident is allowed to write.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RoadmapStatus {
    NotStarted,
    InProgress,
    Done,
    Blocked,
    AwaitingApproval,
}

/// Status column spellings — the parse table, data not code. These are
/// the words the template asks a resident to write.
pub const ROADMAP_STATUS_SPELLINGS: [(RoadmapStatus, &str); 5] = [
    (RoadmapStatus::NotStarted, "Not started"),
    (RoadmapStatus::InProgress, "In progress"),
    (RoadmapStatus::Done, "Done"),
    (RoadmapStatus::Blocked, "Blocked"),
    (RoadmapStatus::AwaitingApproval, "Awaiting approval"),
];

/// Case is not part of the contract: a row whose only defect is `done`
/// for `Done` states a fact the table can hold, and rejecting it would
/// drop that row out of the denominator for a reason no reader accepts.
fn parse_status(raw: &str) -> Option<RoadmapStatus> {
    ROADMAP_STATUS_SPELLINGS
        .iter()
        .find(|(_, spelling)| spelling.eq_ignore_ascii_case(raw))
        .map(|(status, _)| *status)
}

/// The evidence column, three-way: empty and invalid are different facts
/// — one renders as "in progress", the other as "suspect".
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EvidenceCell {
    Empty,
    Invalid { raw: String },
    Present(Locator),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoadmapRow {
    pub index: u64,
    pub item: String,
    pub status: RoadmapStatus,
    pub evidence: EvidenceCell,
}

/// Deliberately exhaustive shape verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoadmapShape {
    WellFormed { rows: Vec<RoadmapRow> },
    Malformed { problems: Vec<String> },
}

fn split_table_row(line: &str) -> Option<Vec<String>> {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') {
        return None;
    }
    let inner = trimmed.strip_prefix('|')?.strip_suffix('|')?;
    Some(
        inner
            .split('|')
            .map(|cell| cell.trim().to_owned())
            .collect(),
    )
}

fn is_separator(cells: &[String]) -> bool {
    !cells.is_empty()
        && cells
            .iter()
            .all(|c| !c.is_empty() && c.chars().all(|ch| ch == '-' || ch == ':'))
}

/// Finds the first pipe table and checks the four-column contract. Pure
/// string work, no I/O; the projection (S3) calls this before any
/// reconciliation.
pub fn check_roadmap_shape(text: &str) -> RoadmapShape {
    let mut rows = Vec::new();
    let mut problems = Vec::new();
    let mut in_table = false;
    let mut header_seen = false;
    for (n, line) in text.lines().enumerate() {
        let line_no = n.saturating_add(1);
        let Some(cells) = split_table_row(line) else {
            if in_table {
                break; // first table ended
            }
            continue;
        };
        in_table = true;
        if cells.len() != 4 {
            problems.push(format!(
                "line {line_no}: {} columns, the roadmap table has exactly 4",
                cells.len()
            ));
            continue;
        }
        if !header_seen {
            header_seen = true;
            continue; // header row
        }
        if is_separator(&cells) {
            continue;
        }
        let (Some(index_cell), Some(item_cell), Some(status_cell), Some(evidence_cell)) =
            (cells.first(), cells.get(1), cells.get(2), cells.get(3))
        else {
            continue;
        };
        let Ok(index) = index_cell.parse::<u64>() else {
            problems.push(format!(
                "line {line_no}: index `{index_cell}` is not a number"
            ));
            continue;
        };
        let Some(status) = parse_status(status_cell) else {
            problems.push(format!(
                "line {line_no}: status `{status_cell}` outside the five-value enum"
            ));
            continue;
        };
        let evidence = if evidence_cell.is_empty() {
            EvidenceCell::Empty
        } else {
            match Locator::parse(evidence_cell) {
                Ok(locator) => EvidenceCell::Present(locator),
                Err(_) => EvidenceCell::Invalid {
                    raw: evidence_cell.clone(),
                },
            }
        };
        rows.push(RoadmapRow {
            index,
            item: item_cell.clone(),
            status,
            evidence,
        });
    }
    if !header_seen {
        problems.push("no four-column table found".to_owned());
    }
    if problems.is_empty() {
        RoadmapShape::WellFormed { rows }
    } else {
        RoadmapShape::Malformed { problems }
    }
}

fn spelling_of(status: RoadmapStatus) -> &'static str {
    ROADMAP_STATUS_SPELLINGS
        .iter()
        .find(|(known, _)| *known == status)
        .map_or("Not started", |(_, spelling)| *spelling)
}

/// Rewrites one row of the first table, returning the whole text.
///
/// This is the table's only editing entrance. Assembling a row of
/// Markdown anywhere else would be a second opinion on what a row looks
/// like, and the grammar may only have one. Rewritten rows are
/// normalised to `| index | item | status | evidence |`, so the same
/// edit applied twice produces the same bytes.
///
/// # Errors
/// Refuses a text whose table does not parse, an index no row carries,
/// and a `Done` without evidence — that last one is exactly the row
/// [`tally`] declines to count, and a writer that can produce it is a
/// writer that can make a plan look finished while the figure stands
/// still.
pub fn set_roadmap_status(
    text: &str,
    index: u64,
    status: RoadmapStatus,
    evidence: Option<&Locator>,
) -> Result<String, AxError> {
    if matches!(status, RoadmapStatus::Done) && evidence.is_none() {
        return Err(AxError::failure(
            AxCode::EvidenceMissing,
            "mark a roadmap row done",
            format!("row {index} has no evidence"),
        )
        .with_recovery("pass a retrievable locator: `cas:<hash>` or `file:<path>@<oid>`"));
    }
    let RoadmapShape::WellFormed { rows } = check_roadmap_shape(text) else {
        return Err(AxError::failure(
            AxCode::InvalidArgs,
            "edit a roadmap row",
            "the roadmap table does not parse",
        )
        .with_recovery("repair the four-column table before editing rows"));
    };
    let row = rows.iter().find(|row| row.index == index).ok_or_else(|| {
        AxError::failure(
            AxCode::InvalidArgs,
            "edit a roadmap row",
            format!("no row numbered {index}"),
        )
        .with_recovery("read the roadmap and use an index the table carries")
    })?;
    let cell = evidence.map_or_else(String::new, |locator| locator.to_string());
    let replacement = format!(
        "| {index} | {} | {} | {cell} |",
        row.item,
        spelling_of(status)
    );
    let mut out = String::with_capacity(text.len());
    let mut in_table = false;
    let mut done = false;
    let mut first = true;
    for line in text.split('\n') {
        if !first {
            out.push('\n');
        }
        first = false;
        let cells = split_table_row(line);
        if cells.is_some() {
            in_table = true;
        } else if in_table {
            in_table = false;
            done = true; // the first table ended; later tables are not ours
        }
        let hit = !done
            && cells.as_ref().is_some_and(|cells| {
                cells.len() == 4
                    && !is_separator(cells)
                    && cells
                        .first()
                        .and_then(|raw| raw.parse::<u64>().ok())
                        .is_some_and(|found| found == index)
            });
        if hit {
            out.push_str(&replacement);
        } else {
            out.push_str(line);
        }
    }
    Ok(out)
}

/// The Memo outline's six fixed fields.
pub const MEMO_OUTLINE_FIELDS: [&str; 6] = [
    "Current goal",
    "Current stage",
    "Next action",
    "Blocked by",
    "Decision index",
    "Checkpoint index",
];

/// Deliberately exhaustive shape verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MemoShape {
    WellFormed,
    Malformed { missing: Vec<&'static str> },
}

/// A field is present when some line, stripped of leading markdown
/// furniture (#, -, *, spaces), starts with it. Case is not part of the
/// contract, for the reason given at `parse_status`.
pub fn check_memo_shape(text: &str) -> MemoShape {
    let stripped: Vec<String> = text
        .lines()
        .map(|line| {
            line.trim_start_matches(['#', '-', '*', ' ', '\t'])
                .to_lowercase()
        })
        .collect();
    let missing: Vec<&'static str> = MEMO_OUTLINE_FIELDS
        .into_iter()
        .filter(|field| {
            let needle = field.to_lowercase();
            !stripped.iter().any(|line| line.starts_with(&needle))
        })
        .collect();
    if missing.is_empty() {
        MemoShape::WellFormed
    } else {
        MemoShape::Malformed { missing }
    }
}

/// Reconciliation rule 1 in numbers: Done counts only with parsable
/// evidence; Blocked and AwaitingApproval both read as blocked. Counts
/// saturate at u32::MAX by contract (the renderer's domain).
pub fn tally(rows: &[RoadmapRow]) -> Progress {
    let mut done: u32 = 0;
    let mut blocked: u32 = 0;
    let mut total: u32 = 0;
    for row in rows {
        total = total.saturating_add(1);
        match (&row.status, &row.evidence) {
            (RoadmapStatus::Done, EvidenceCell::Present(_)) => done = done.saturating_add(1),
            (RoadmapStatus::Blocked | RoadmapStatus::AwaitingApproval, _) => {
                blocked = blocked.saturating_add(1);
            }
            _ => {}
        }
    }
    Progress::Planned(PlannedProgress {
        done,
        blocked,
        total,
    })
}

/// Scope-change vocabulary: requirements move by KEEP/ADD/DROP, never by
/// piling replacements into a bigger project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeChange {
    Keep,
    Add,
    Drop,
}

/// The three write moments; consumers arrive with the S3 turn layer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WriteMoment {
    BeforeReport,
    AfterFeedback,
    OnPlanChange,
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

    const GOOD: &str = "\
# Roadmap

| # | Item | Status | Evidence |
|---|------|--------|----------|
| 1 | chain verification | Done | cas:b3-abababababababababababababababababababababababababababababababab |
| 2 | range retrieval | In progress | |
| 3 | fixtures | Awaiting approval | |
| 4 | claimed without evidence | Done | |
| 5 | evidence that does not parse | done | not-a-locator |
";

    #[test]
    fn a_good_table_parses_and_the_tally_is_honest() {
        let shape = check_roadmap_shape(GOOD);
        let RoadmapShape::WellFormed { rows } = shape else {
            panic!("expected well-formed");
        };
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].status, RoadmapStatus::Done);
        assert!(matches!(rows[0].evidence, EvidenceCell::Present(_)));
        assert!(matches!(rows[3].evidence, EvidenceCell::Empty));
        assert!(matches!(rows[4].evidence, EvidenceCell::Invalid { .. }));
        // Five rows; only the evidenced Done counts; awaiting approval
        // reads as blocked; row 5 proves case is not part of the contract.
        let Progress::Planned(planned) = tally(&rows) else {
            panic!("tally over rows is planned")
        };
        assert_eq!((planned.done, planned.blocked, planned.total), (1, 1, 5));
    }

    #[test]
    fn a_rewritten_row_keeps_the_table_parsable_and_is_byte_stable() {
        let once = set_roadmap_status(GOOD, 2, RoadmapStatus::InProgress, None).unwrap();
        let twice = set_roadmap_status(&once, 2, RoadmapStatus::InProgress, None).unwrap();
        assert_eq!(once, twice, "the same edit twice is the same bytes");
        let RoadmapShape::WellFormed { rows } = check_roadmap_shape(&once) else {
            panic!("an edited roadmap still parses");
        };
        assert_eq!(rows.len(), 5, "editing one row does not lose the others");
        assert_eq!(rows[1].status, RoadmapStatus::InProgress);
        assert_eq!(rows[1].item, "range retrieval", "the item text survives");
    }

    #[test]
    fn done_without_evidence_is_refused_where_it_is_written() {
        let refusal = set_roadmap_status(GOOD, 2, RoadmapStatus::Done, None).unwrap_err();
        assert_eq!(refusal.code(), &AxCode::EvidenceMissing);
        assert!(refusal.recovery().contains("cas:"));
        let locator = Locator::parse(&format!("cas:b3-{}", "ab".repeat(32))).unwrap();
        let text = set_roadmap_status(GOOD, 2, RoadmapStatus::Done, Some(&locator)).unwrap();
        let RoadmapShape::WellFormed { rows } = check_roadmap_shape(&text) else {
            panic!("an edited roadmap still parses");
        };
        let Progress::Planned(planned) = tally(&rows) else {
            panic!("tally over rows is planned")
        };
        assert_eq!(planned.done, 2, "an evidenced Done reaches the numerator");
    }

    #[test]
    fn an_index_no_row_carries_is_refused_by_number() {
        let refusal = set_roadmap_status(GOOD, 99, RoadmapStatus::InProgress, None).unwrap_err();
        assert_eq!(refusal.code(), &AxCode::InvalidArgs);
        assert!(refusal.subject().contains("99"));
    }

    #[test]
    fn a_second_table_below_the_roadmap_is_not_edited() {
        let text = format!(
            "{GOOD}
## Notes

| # | Item | Status | Evidence |
|---|---|---|---|
| 2 | other table | Not started | |
"
        );
        let edited = set_roadmap_status(&text, 2, RoadmapStatus::Blocked, None).unwrap();
        assert!(
            edited.contains("| 2 | other table | Not started | |"),
            "only the first table is the roadmap"
        );
        assert!(edited.contains("| 2 | range retrieval | Blocked |  |"));
    }

    #[test]
    fn wrong_column_count_and_alien_status_are_named_problems() {
        let text = "\
| # | Item | Status |
|---|------|--------|
| 1 | x | nearly there |
";
        let RoadmapShape::Malformed { problems } = check_roadmap_shape(text) else {
            panic!("expected malformed");
        };
        assert!(problems.iter().any(|p| p.contains("3 columns")));
    }

    #[test]
    fn free_text_status_is_rejected_by_the_enum() {
        let text = "\
| # | Item | Status | Evidence |
|---|------|--------|----------|
| 1 | x | nearly there | |
";
        let RoadmapShape::Malformed { problems } = check_roadmap_shape(text) else {
            panic!("expected malformed");
        };
        assert!(problems.iter().any(|p| p.contains("nearly there")));
    }

    #[test]
    fn no_table_at_all_is_malformed() {
        assert!(matches!(
            check_roadmap_shape("just prose"),
            RoadmapShape::Malformed { .. }
        ));
    }

    #[test]
    fn memo_outline_names_what_is_missing() {
        let memo = "\
## Current goal
ship S2
## current stage
S2.08
## Next action
approval
## Blocked by
none
## Decision index
d-1
";
        let MemoShape::Malformed { missing } = check_memo_shape(memo) else {
            panic!("the checkpoint index is absent, shape must be malformed");
        };
        assert_eq!(missing, ["Checkpoint index"]);
        let full = format!("{memo}## Checkpoint index\nc-1\n");
        assert_eq!(check_memo_shape(&full), MemoShape::WellFormed);
    }
}
