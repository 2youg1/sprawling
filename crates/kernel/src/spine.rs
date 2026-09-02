// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The spine documents' grammar: how a row of `Roadmap.md` is written
//! and read, and the six fields a Memo outline carries.
//!
//! Shape and truth are two doors. `check_*` answers "is this written
//! like a roadmap"; what the rows then *mean* — how they hang together,
//! what each is worth, what may be started — is `crate::plan`'s, and it
//! is separate because a mistyped row is repaired by editing the line
//! while a circular dependency is repaired by rethinking the work.
//!
//! **Six columns, and the index is a path.** `2.3.1` hangs under `2.3`,
//! so the table states a tree without a second file to say so; `Weight`
//! is a ratio among the rows that share a parent, never a quantity; and
//! `Needs` names the rows that must finish first. The three columns the
//! old four-column table lacked are what let one plan carry multi-level
//! progress, a dependency graph and a ready set instead of pointing at a
//! diagram file nobody parses.
//!
//! **Every write goes through this file.** Assembling a row of Markdown
//! anywhere else would be a second opinion on what a row looks like, and
//! a grammar may only have one.

use serde::{Deserialize, Serialize};

use crate::error::{AxCode, AxError};
use crate::locator::Locator;
use crate::plan::NodeId;

/// The five states, closed. The spellings below are the canonical table
/// strings, and there is exactly one set of them: a second set would be a
/// second authority over what a resident is allowed to write.
///
/// The serde spellings are `snake_case` identifiers rather than the table
/// words: a wire frame is read by a program and a table cell by a person,
/// and making the client carry `Awaiting approval` would put an English
/// sentence where the phrase table belongs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
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

/// The number of columns the roadmap table has. Named because three
/// places state it — the parser, the refusal it writes, and the
/// template — and a plan whose columns disagree with its reader has no
/// denominator at all.
pub const ROADMAP_COLUMNS: usize = 6;

impl RoadmapStatus {
    /// The one spelling, for every writer and every message.
    #[must_use]
    pub fn spelling(self) -> &'static str {
        ROADMAP_STATUS_SPELLINGS
            .iter()
            .find(|(known, _)| *known == self)
            .map_or("Not started", |(_, spelling)| *spelling)
    }
}

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

/// One row, as the table spells it.
///
/// `weight` is a ratio among the rows that share a parent and never a
/// quantity, so doubling every number on one level changes nothing. An
/// empty cell reads as 1: a plan that says nothing about how a level
/// divides is a plan that divides it evenly.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RoadmapRow {
    pub id: NodeId,
    pub item: String,
    pub weight: u32,
    pub needs: Vec<NodeId>,
    pub status: RoadmapStatus,
    pub evidence: EvidenceCell,
}

/// Deliberately exhaustive shape verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoadmapShape {
    WellFormed { rows: Vec<RoadmapRow> },
    Malformed { problems: Vec<String> },
}

/// A child to hang under a node: what it is, and how big a slice of its
/// parent it takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewChild {
    pub item: String,
    pub weight: u32,
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

/// Whether a line is a body row of the roadmap: a table row that is
/// neither the header nor the separator. The one test, shared by the
/// parser and both writers, so "which lines are the plan" has a single
/// answer.
fn body_row(cells: &[String]) -> bool {
    cells.len() == ROADMAP_COLUMNS && !is_separator(cells)
}

/// The lines of the first table, as an inclusive range over
/// `text.split('\n')`. `None` when the text carries no table.
fn locate_table(text: &str) -> Option<(usize, usize)> {
    let mut first = None;
    let mut last = 0;
    for (n, line) in text.split('\n').enumerate() {
        if split_table_row(line).is_some() {
            if first.is_none() {
                first = Some(n);
            }
            last = n;
        } else if first.is_some() {
            break;
        }
    }
    first.map(|start| (start, last))
}

/// Finds the first pipe table and checks the six-column contract. Pure
/// string work, no I/O; `crate::plan` builds the tree from what comes
/// back.
#[must_use]
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
        if cells.len() != ROADMAP_COLUMNS {
            problems.push(format!(
                "line {line_no}: {} columns, the roadmap table has exactly {ROADMAP_COLUMNS}",
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
        match read_row(&cells) {
            Ok(row) => rows.push(row),
            Err(why) => problems.push(format!("line {line_no}: {why}")),
        }
    }
    if !header_seen {
        problems.push(format!("no {ROADMAP_COLUMNS}-column table found"));
    }
    if problems.is_empty() {
        RoadmapShape::WellFormed { rows }
    } else {
        RoadmapShape::Malformed { problems }
    }
}

/// Reads one body row, or says in one clause what is wrong with it.
fn read_row(cells: &[String]) -> Result<RoadmapRow, String> {
    let (
        Some(index_cell),
        Some(item_cell),
        Some(weight_cell),
        Some(needs_cell),
        Some(status_cell),
        Some(evidence_cell),
    ) = (
        cells.first(),
        cells.get(1),
        cells.get(2),
        cells.get(3),
        cells.get(4),
        cells.get(5),
    )
    else {
        return Err("the row is short of cells".to_owned());
    };
    let id = NodeId::parse(index_cell).map_err(|err| err.subject().to_owned())?;
    let weight = if weight_cell.is_empty() {
        1
    } else {
        weight_cell
            .parse::<u32>()
            .map_err(|_| format!("weight `{weight_cell}` is not a number"))?
    };
    let mut needs = Vec::new();
    for part in needs_cell.split(',') {
        let trimmed = part.trim();
        if trimmed.is_empty() {
            continue;
        }
        needs.push(NodeId::parse(trimmed).map_err(|err| err.subject().to_owned())?);
    }
    let status = parse_status(status_cell)
        .ok_or_else(|| format!("status `{status_cell}` outside the five-value enum"))?;
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
    Ok(RoadmapRow {
        id,
        item: item_cell.clone(),
        weight,
        needs,
        status,
        evidence,
    })
}

/// One row as the table writes it. The normal form: the same edit twice
/// produces the same bytes.
fn draw_row(row: &RoadmapRow, status: RoadmapStatus, evidence: Option<&Locator>) -> String {
    let needs = row
        .needs
        .iter()
        .map(NodeId::to_string)
        .collect::<Vec<String>>()
        .join(", ");
    let cell = evidence.map_or_else(String::new, Locator::to_string);
    format!(
        "| {} | {} | {} | {needs} | {} | {cell} |",
        row.id,
        row.item,
        row.weight,
        status.spelling()
    )
}

/// Rewrites one row of the first table, returning the whole text.
///
/// This is the table's only status-editing entrance.
///
/// # Errors
/// Refuses a text whose table does not parse, an index no row carries,
/// and a `Done` without evidence — that last one is exactly the row
/// `crate::plan` declines to count, and a writer that can produce it is
/// a writer that can make a plan look finished while the figure stands
/// still.
pub fn set_roadmap_status(
    text: &str,
    id: &NodeId,
    status: RoadmapStatus,
    evidence: Option<&Locator>,
) -> Result<String, AxError> {
    if matches!(status, RoadmapStatus::Done) && evidence.is_none() {
        return Err(AxError::failure(
            AxCode::EvidenceMissing,
            "mark a roadmap row done",
            format!("row {id} has no evidence"),
        )
        .with_recovery("pass a retrievable locator: `cas:<hash>` or `file:<path>@<oid>`"));
    }
    let rows = well_formed(text, "edit a roadmap row")?;
    let row = rows.iter().find(|row| &row.id == id).ok_or_else(|| {
        AxError::failure(
            AxCode::InvalidArgs,
            "edit a roadmap row",
            format!("no row numbered {id}"),
        )
        .with_recovery("read the roadmap and use an index the table carries")
    })?;
    let replacement = draw_row(row, status, evidence);
    Ok(rewrite(text, |cells| {
        cells
            .first()
            .and_then(|raw| NodeId::parse(raw).ok())
            .is_some_and(|found| &found == id)
            .then(|| replacement.clone())
    }))
}

/// Hangs new children under a node, returning the whole text.
///
/// The children land directly below the node's last descendant, so the
/// table stays in reading order and the numbers a person saw yesterday
/// still point at the same work. New ordinals continue after the
/// node's existing children rather than reusing a gap: a plan index is
/// a name, and reusing one would silently move somebody's evidence.
///
/// # Errors
/// Refuses a table that does not parse, an index no row carries, an
/// empty list of children, a child whose text is blank, a weight of
/// zero (a node worth nothing is a row nobody will ever be given), and
/// a split that would push the plan past `plan::NODE_DEPTH_MAX`.
pub fn insert_children(
    text: &str,
    parent: &NodeId,
    children: &[NewChild],
) -> Result<String, AxError> {
    let refuse = |subject: String, recovery: &str| {
        AxError::failure(AxCode::InvalidArgs, "split a plan node", subject)
            .with_recovery(recovery.to_owned())
    };
    if children.is_empty() {
        return Err(refuse(
            "no children given".to_owned(),
            "name at least one child; splitting into nothing would delete the work",
        ));
    }
    let rows = well_formed(text, "split a plan node")?;
    if !rows.iter().any(|row| &row.id == parent) {
        return Err(refuse(
            format!("no row numbered {parent}"),
            "read the roadmap and use an index the table carries",
        ));
    }
    let mut next = rows
        .iter()
        .filter(|row| row.id.parent().as_ref() == Some(parent))
        .map(|row| row.id.ordinal())
        .max()
        .unwrap_or(0);
    let mut drawn = Vec::with_capacity(children.len());
    for child in children {
        if child.item.trim().is_empty() {
            return Err(refuse(
                "a child with no text".to_owned(),
                "say what each child is; a row nobody can read is a row nobody can take",
            ));
        }
        if child.item.contains('|') {
            return Err(refuse(
                format!("`{}` carries a column separator", child.item),
                "write the item without `|`; it would split the row into more cells",
            ));
        }
        if child.weight == 0 {
            return Err(refuse(
                format!("`{}` is weighted zero", child.item),
                "give every child a weight of at least one; a node worth nothing is never given \
                 to anybody",
            ));
        }
        next = next.saturating_add(1);
        let id = parent.child(next)?;
        drawn.push(format!(
            "| {id} | {} | {} |  | {} |  |",
            child.item.trim(),
            child.weight,
            RoadmapStatus::NotStarted.spelling()
        ));
    }
    let Some((first, last)) = locate_table(text) else {
        return Err(refuse(
            "the roadmap table is not there".to_owned(),
            "repair the table before splitting a node",
        ));
    };
    // The insertion point: after the parent's last descendant, or after
    // the parent itself when it has none.
    let mut after = None;
    for (n, line) in text.split('\n').enumerate() {
        if n < first || n > last {
            continue;
        }
        let Some(cells) = split_table_row(line) else {
            continue;
        };
        if !body_row(&cells) {
            continue;
        }
        let Some(id) = cells.first().and_then(|raw| NodeId::parse(raw).ok()) else {
            continue;
        };
        if &id == parent || parent.is_ancestor_of(&id) {
            after = Some(n);
        }
    }
    let seam = after.unwrap_or(last);
    let mut out: Vec<String> = Vec::new();
    for (n, line) in text.split('\n').enumerate() {
        out.push(line.to_owned());
        if n == seam {
            out.extend(drawn.iter().cloned());
        }
    }
    Ok(out.join("\n"))
}

/// The rows of a well-formed table, or the refusal that says which line
/// is wrong.
fn well_formed(text: &str, action: &'static str) -> Result<Vec<RoadmapRow>, AxError> {
    match check_roadmap_shape(text) {
        RoadmapShape::WellFormed { rows } => Ok(rows),
        RoadmapShape::Malformed { problems } => Err(AxError::failure(
            AxCode::InvalidArgs,
            action,
            problems.join("; "),
        )
        .with_recovery(format!(
            "repair the {ROADMAP_COLUMNS}-column table in Roadmap.md first; a plan that does not \
             parse has no denominator"
        ))),
    }
}

/// Replaces the body rows of the first table for which `chosen` returns
/// a replacement, leaving every other byte alone.
fn rewrite(text: &str, mut chosen: impl FnMut(&[String]) -> Option<String>) -> String {
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
        let hit = if done {
            None
        } else {
            cells
                .as_ref()
                .filter(|cells| body_row(cells))
                .and_then(|cells| chosen(cells))
        };
        match hit {
            Some(replacement) => out.push_str(&replacement),
            None => out.push_str(line),
        }
    }
    out
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
#[must_use]
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

/// Scope-change vocabulary: requirements move by KEEP/ADD/DROP, never by
/// piling replacements into a bigger project.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeChange {
    Keep,
    Add,
    Drop,
}

/// The three moments the plan may be written.
///
/// Its consumer is the projection that holds the parsed tree: because
/// the set of moments is closed, a reader can hold the tree between them
/// instead of parsing every building's file for every question.
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

| # | Item | Weight | Needs | Status | Evidence |
|---|------|--------|-------|--------|----------|
| 1 | chain verification | 1 |  | Done | cas:b3-abababababababababababababababababababababababababababababababab |
| 2 | range retrieval | 3 | 1 | In progress |  |
| 3 | fixtures | 1 |  | Awaiting approval |  |
| 4 | claimed without evidence | 1 |  | Done |  |
| 5 | evidence that does not parse | 1 |  | done | not-a-locator |
";

    fn node(raw: &str) -> NodeId {
        NodeId::parse(raw).unwrap()
    }

    fn locator() -> Locator {
        Locator::parse(&format!("cas:b3-{}", "ab".repeat(32))).unwrap()
    }

    fn rows_of(text: &str) -> Vec<RoadmapRow> {
        match check_roadmap_shape(text) {
            RoadmapShape::WellFormed { rows } => rows,
            RoadmapShape::Malformed { problems } => panic!("expected well-formed: {problems:?}"),
        }
    }

    #[test]
    fn a_good_table_parses_every_column() {
        let rows = rows_of(GOOD);
        assert_eq!(rows.len(), 5);
        assert_eq!(rows[0].status, RoadmapStatus::Done);
        assert!(matches!(rows[0].evidence, EvidenceCell::Present(_)));
        assert_eq!(rows[1].weight, 3);
        assert_eq!(rows[1].needs, vec![node("1")]);
        assert_eq!(rows[2].weight, 1, "an empty weight cell reads as one");
        assert!(rows[2].needs.is_empty());
        assert!(matches!(rows[3].evidence, EvidenceCell::Empty));
        assert!(matches!(rows[4].evidence, EvidenceCell::Invalid { .. }));
        assert_eq!(rows[4].status, RoadmapStatus::Done, "case is not contract");
    }

    #[test]
    fn a_rewritten_row_keeps_the_table_parsable_and_is_byte_stable() {
        let once = set_roadmap_status(GOOD, &node("2"), RoadmapStatus::InProgress, None).unwrap();
        let twice = set_roadmap_status(&once, &node("2"), RoadmapStatus::InProgress, None).unwrap();
        assert_eq!(once, twice, "the same edit twice is the same bytes");
        let rows = rows_of(&once);
        assert_eq!(rows.len(), 5, "editing one row does not lose the others");
        assert_eq!(rows[1].status, RoadmapStatus::InProgress);
        assert_eq!(rows[1].item, "range retrieval", "the item text survives");
        assert_eq!(rows[1].weight, 3, "and so do the weight and the needs");
        assert_eq!(rows[1].needs, vec![node("1")]);
    }

    #[test]
    fn done_without_evidence_is_refused_where_it_is_written() {
        let refusal = set_roadmap_status(GOOD, &node("2"), RoadmapStatus::Done, None).unwrap_err();
        assert_eq!(refusal.code(), &AxCode::EvidenceMissing);
        assert!(refusal.recovery().contains("cas:"));
        let text =
            set_roadmap_status(GOOD, &node("2"), RoadmapStatus::Done, Some(&locator())).unwrap();
        assert!(text.contains("| 2 | range retrieval | 3 | 1 | Done | cas:b3-"));
    }

    #[test]
    fn an_index_no_row_carries_is_refused_by_number() {
        let refusal =
            set_roadmap_status(GOOD, &node("99"), RoadmapStatus::InProgress, None).unwrap_err();
        assert_eq!(refusal.code(), &AxCode::InvalidArgs);
        assert!(refusal.subject().contains("99"));
    }

    #[test]
    fn a_second_table_below_the_roadmap_is_not_edited() {
        let text = format!(
            "{GOOD}
## Notes

| # | Item | Weight | Needs | Status | Evidence |
|---|---|---|---|---|---|
| 2 | other table | 1 |  | Not started |  |
"
        );
        let edited = set_roadmap_status(&text, &node("2"), RoadmapStatus::Blocked, None).unwrap();
        assert!(
            edited.contains("| 2 | other table | 1 |  | Not started |  |"),
            "only the first table is the roadmap"
        );
        assert!(edited.contains("| 2 | range retrieval | 3 | 1 | Blocked |  |"));
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
| # | Item | Weight | Needs | Status | Evidence |
|---|------|--------|-------|--------|----------|
| 1 | x | 1 |  | nearly there |  |
";
        let RoadmapShape::Malformed { problems } = check_roadmap_shape(text) else {
            panic!("expected malformed");
        };
        assert!(problems.iter().any(|p| p.contains("nearly there")));
    }

    /// The four-column table is the old grammar, and a plan written in
    /// it has no weights and no dependencies. It is reported rather than
    /// half-read: reading four columns as six would put the status word
    /// in the weight cell.
    #[test]
    fn the_old_four_column_table_is_reported_rather_than_half_read() {
        let old = "\
| # | Item | Status | Evidence |
|---|------|--------|----------|
| 1 | x | Not started | |
";
        let RoadmapShape::Malformed { problems } = check_roadmap_shape(old) else {
            panic!("expected malformed");
        };
        assert!(problems.iter().any(|p| p.contains("4 columns")));
    }

    #[test]
    fn a_bad_index_and_a_bad_weight_are_named_by_line() {
        let text = "\
| # | Item | Weight | Needs | Status | Evidence |
|---|------|--------|-------|--------|----------|
| one | x | 1 |  | Not started |  |
| 2 | y | heavy |  | Not started |  |
| 3 | z | 1 | nine | Not started |  |
";
        let RoadmapShape::Malformed { problems } = check_roadmap_shape(text) else {
            panic!("expected malformed");
        };
        assert_eq!(problems.len(), 3);
        assert!(problems[0].contains("line 3"));
        assert!(problems[1].contains("heavy"));
        assert!(problems[2].contains("line 5"));
    }

    #[test]
    fn no_table_at_all_is_malformed() {
        assert!(matches!(
            check_roadmap_shape("just prose"),
            RoadmapShape::Malformed { .. }
        ));
    }

    #[test]
    fn children_land_below_their_parent_and_number_on_from_the_last() {
        let text = "\
| # | Item | Weight | Needs | Status | Evidence |
|---|------|--------|-------|--------|----------|
| 1 | build | 1 |  | Not started |  |
| 2 | ship | 1 |  | Not started |  |
";
        let split = insert_children(
            text,
            &node("1"),
            &[
                NewChild {
                    item: "design".to_owned(),
                    weight: 1,
                },
                NewChild {
                    item: "code".to_owned(),
                    weight: 3,
                },
            ],
        )
        .unwrap();
        let rows = rows_of(&split);
        let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, ["1", "1.1", "1.2", "2"], "reading order survives");
        assert_eq!(rows[2].weight, 3);

        let again = insert_children(
            &split,
            &node("1"),
            &[NewChild {
                item: "test".to_owned(),
                weight: 1,
            }],
        )
        .unwrap();
        let grown = rows_of(&again);
        let ids: Vec<&str> = grown.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(
            ids,
            ["1", "1.1", "1.2", "1.3", "2"],
            "a second split numbers on rather than reusing an index"
        );
    }

    #[test]
    fn splitting_a_deep_branch_puts_the_children_after_the_whole_branch() {
        let text = "\
| # | Item | Weight | Needs | Status | Evidence |
|---|------|--------|-------|--------|----------|
| 1 | build | 1 |  | Not started |  |
| 1.1 | design | 1 |  | Not started |  |
| 1.1.1 | sketch | 1 |  | Not started |  |
| 2 | ship | 1 |  | Not started |  |
";
        let split = insert_children(
            text,
            &node("1"),
            &[NewChild {
                item: "code".to_owned(),
                weight: 1,
            }],
        )
        .unwrap();
        let rows = rows_of(&split);
        let ids: Vec<&str> = rows.iter().map(|row| row.id.as_str()).collect();
        assert_eq!(ids, ["1", "1.1", "1.1.1", "1.2", "2"]);
    }

    #[test]
    fn a_split_that_would_delete_or_hide_work_is_refused() {
        let text = "\
| # | Item | Weight | Needs | Status | Evidence |
|---|------|--------|-------|--------|----------|
| 1 | build | 1 |  | Not started |  |
";
        assert!(insert_children(text, &node("1"), &[]).is_err());
        assert!(
            insert_children(
                text,
                &node("1"),
                &[NewChild {
                    item: "  ".to_owned(),
                    weight: 1
                }]
            )
            .is_err()
        );
        let zero = insert_children(
            text,
            &node("1"),
            &[NewChild {
                item: "x".to_owned(),
                weight: 0,
            }],
        )
        .unwrap_err();
        assert!(zero.recovery().contains("at least one"));
        let piped = insert_children(
            text,
            &node("1"),
            &[NewChild {
                item: "a | b".to_owned(),
                weight: 1,
            }],
        )
        .unwrap_err();
        assert!(piped.subject().contains("column separator"));
        assert!(
            insert_children(
                text,
                &node("9"),
                &[NewChild {
                    item: "x".to_owned(),
                    weight: 1
                }]
            )
            .is_err()
        );
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
