// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The claim tool: the face `Roadmap.md` shows a model.
//!
//! The plan file is the only register. A second table of who-holds-what
//! would be a second answer to the same question, and the one that
//! drifts is always the one nobody reads — while this file is read by
//! the person, counted by `tally`, and edited here.
//!
//! Which transitions are legal is decided from the row's current state
//! rather than from what the caller says it is doing. `claim` takes only
//! `Not started`; `finish` and `release` take only `In progress`. The
//! refusal names the state the row is actually in and points at the next
//! free row, because "no" teaches a model to rephrase and try again,
//! while "3 is being worked on, 5 is free" does not.

use std::cell::RefCell;
use std::rc::Rc;

use kernel::{
    Address, AxCode, AxError, CostTier, Effect, EvidenceCell, Locator, Payload, RenderIntent,
    RoadmapRow, RoadmapShape, RoadmapStatus, Temporal, Tool, ToolCall, ToolMeta, ToolName,
    ToolOutcome, check_roadmap_shape, set_roadmap_status,
};
use serde_json::{Map, Value};

/// What the run did to the plan. Exhaustive on purpose, like the other
/// desks': every variant is a line the worker has to write, so a new one
/// must be a compile error where the writing happens rather than a
/// runtime arm nobody reaches until a claim quietly goes unrecorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimEffect {
    Claimed {
        index: u64,
        item: String,
    },
    Finished {
        index: u64,
        item: String,
        evidence: Locator,
    },
    Released {
        index: u64,
        item: String,
    },
}

impl ClaimEffect {
    /// The index the effect concerns, which is what the worker re-reads
    /// the file against before writing.
    #[must_use]
    pub fn index(&self) -> u64 {
        match self {
            ClaimEffect::Claimed { index, .. }
            | ClaimEffect::Finished { index, .. }
            | ClaimEffect::Released { index, .. } => *index,
        }
    }

    /// The status the row must already be in for this effect to still be
    /// the truth when the worker applies it.
    #[must_use]
    pub fn expected_before(&self) -> RoadmapStatus {
        match self {
            ClaimEffect::Claimed { .. } => RoadmapStatus::NotStarted,
            ClaimEffect::Finished { .. } | ClaimEffect::Released { .. } => {
                RoadmapStatus::InProgress
            }
        }
    }

    /// The `roadmap_*` payload: what a rebuild reads back.
    ///
    /// # Errors
    /// Propagates the payload's refusal to hold what it was given.
    pub fn payload(&self, who: &str) -> Result<Payload, AxError> {
        let mut map = Map::new();
        map.insert("by".to_owned(), Value::String(who.to_owned()));
        map.insert("row".to_owned(), Value::Number(self.index().into()));
        let (verb, item) = match self {
            ClaimEffect::Claimed { item, .. } => ("claimed", item),
            ClaimEffect::Finished { item, evidence, .. } => {
                map.insert("evidence".to_owned(), Value::String(evidence.to_string()));
                ("finished", item)
            }
            ClaimEffect::Released { item, .. } => ("released", item),
        };
        map.insert("verb".to_owned(), Value::String(verb.to_owned()));
        map.insert("item".to_owned(), Value::String(item.clone()));
        Payload::new(map)
    }
}

/// The run's side of the plan: the file as it stood when the run was
/// dispatched, plus what this run has changed that the ledger does not
/// know yet.
#[derive(Debug)]
pub struct ClaimDesk {
    who: String,
    room: Address,
    text: String,
    changed: bool,
    /// The row this run holds. One at a time: a run holding two rows
    /// makes both rows' progress unreadable, because the row is the unit
    /// of "what is being worked on now".
    held: Option<u64>,
    effects: Vec<ClaimEffect>,
}

impl ClaimDesk {
    #[must_use]
    pub fn new(who: String, room: Address, roadmap: String) -> ClaimDesk {
        ClaimDesk {
            who,
            room,
            text: roadmap,
            changed: false,
            held: None,
            effects: Vec::new(),
        }
    }

    /// What the worker has to record, drained so it cannot run twice.
    pub fn take_effects(&mut self) -> Vec<ClaimEffect> {
        std::mem::take(&mut self.effects)
    }

    /// The plan as this run left it, or `None` when the run did not
    /// touch it. Writing an unchanged file would put a modification time
    /// on a plan nobody edited.
    #[must_use]
    pub fn roadmap(&self) -> Option<&str> {
        if self.changed { Some(&self.text) } else { None }
    }

    #[must_use]
    pub fn who(&self) -> &str {
        &self.who
    }

    fn rows(&self) -> Result<Vec<RoadmapRow>, AxError> {
        match check_roadmap_shape(&self.text) {
            RoadmapShape::WellFormed { rows } => Ok(rows),
            RoadmapShape::Malformed { problems } => Err(AxError::failure(
                AxCode::InvalidArgs,
                "read the plan",
                problems.join("; "),
            )
            .with_recovery(
                "repair the four-column table in Roadmap.md, then claim a row; a plan that does \
                 not parse has no denominator",
            )),
        }
    }

    fn next_free(rows: &[RoadmapRow]) -> Option<u64> {
        rows.iter()
            .find(|row| matches!(row.status, RoadmapStatus::NotStarted))
            .map(|row| row.index)
    }

    fn somewhere_else(rows: &[RoadmapRow]) -> String {
        Self::next_free(rows).map_or_else(
            || "no row is unclaimed; report to the person instead of picking one".to_owned(),
            |index| format!("claim row {index}, which is still `Not started`"),
        )
    }

    fn row_at(rows: &[RoadmapRow], index: u64) -> Result<RoadmapRow, AxError> {
        rows.iter()
            .find(|row| row.index == index)
            .cloned()
            .ok_or_else(|| {
                AxError::failure(
                    AxCode::InvalidArgs,
                    "find a plan row",
                    format!("no row numbered {index}"),
                )
                .with_recovery(format!(
                    "list the plan first; it carries {} rows",
                    rows.len()
                ))
            })
    }

    fn list(&self) -> Result<Payload, AxError> {
        let rows = self.rows()?;
        let mut free = Vec::new();
        let mut working = Vec::new();
        for row in &rows {
            let mut entry = Map::new();
            entry.insert("row".to_owned(), Value::Number(row.index.into()));
            entry.insert("item".to_owned(), Value::String(row.item.clone()));
            match row.status {
                RoadmapStatus::NotStarted => free.push(Value::Object(entry)),
                RoadmapStatus::InProgress => working.push(Value::Object(entry)),
                RoadmapStatus::Done | RoadmapStatus::Blocked | RoadmapStatus::AwaitingApproval => {}
            }
        }
        let mut result = Map::new();
        result.insert("unclaimed".to_owned(), Value::Array(free));
        result.insert("in_progress".to_owned(), Value::Array(working));
        // The denominator is on screen for the person; it is here so the
        // model does not have to count the array to know where it stands.
        result.insert(
            "rows_total".to_owned(),
            Value::Number(u64::try_from(rows.len()).unwrap_or(u64::MAX).into()),
        );
        Payload::new(result)
    }

    fn apply(
        &mut self,
        index: u64,
        to: RoadmapStatus,
        evidence: Option<&Locator>,
    ) -> Result<(), AxError> {
        self.text = set_roadmap_status(&self.text, index, to, evidence)?;
        self.changed = true;
        Ok(())
    }

    fn claim(&mut self, index: u64) -> Result<Payload, AxError> {
        if let Some(already) = self.held {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "claim a plan row",
                format!("this run already holds row {already}"),
            )
            .with_recovery(format!(
                "finish or release row {already} before claiming another; a run works on one row"
            )));
        }
        let rows = self.rows()?;
        let row = Self::row_at(&rows, index)?;
        if !matches!(row.status, RoadmapStatus::NotStarted) {
            return Err(AxError::failure(
                AxCode::GoalConflict,
                "claim a plan row",
                format!("row {index} is `{}`", spelling(row.status)),
            )
            .with_recovery(Self::somewhere_else(&rows)));
        }
        self.apply(index, RoadmapStatus::InProgress, None)?;
        self.held = Some(index);
        self.effects.push(ClaimEffect::Claimed {
            index,
            item: row.item.clone(),
        });
        let mut result = Map::new();
        result.insert("row".to_owned(), Value::Number(index.into()));
        result.insert("item".to_owned(), Value::String(row.item));
        result.insert("held_by".to_owned(), Value::String(self.who.clone()));
        Payload::new(result)
    }

    fn finish(&mut self, index: u64, raw_evidence: &str) -> Result<Payload, AxError> {
        let evidence = Locator::parse(raw_evidence)?;
        let rows = self.rows()?;
        let row = Self::row_at(&rows, index)?;
        self.require_in_progress(&row)?;
        self.apply(index, RoadmapStatus::Done, Some(&evidence))?;
        if self.held == Some(index) {
            self.held = None;
        }
        self.effects.push(ClaimEffect::Finished {
            index,
            item: row.item.clone(),
            evidence: evidence.clone(),
        });
        let mut result = Map::new();
        result.insert("row".to_owned(), Value::Number(index.into()));
        result.insert("item".to_owned(), Value::String(row.item));
        result.insert("evidence".to_owned(), Value::String(evidence.to_string()));
        Payload::new(result)
    }

    fn release(&mut self, index: u64) -> Result<Payload, AxError> {
        let rows = self.rows()?;
        let row = Self::row_at(&rows, index)?;
        self.require_in_progress(&row)?;
        self.apply(index, RoadmapStatus::NotStarted, None)?;
        if self.held == Some(index) {
            self.held = None;
        }
        self.effects.push(ClaimEffect::Released {
            index,
            item: row.item.clone(),
        });
        let mut result = Map::new();
        result.insert("row".to_owned(), Value::Number(index.into()));
        result.insert("item".to_owned(), Value::String(row.item));
        Payload::new(result)
    }

    fn require_in_progress(&self, row: &RoadmapRow) -> Result<(), AxError> {
        if matches!(row.status, RoadmapStatus::InProgress) {
            return Ok(());
        }
        let hint = match row.status {
            RoadmapStatus::NotStarted => "claim it first",
            RoadmapStatus::Done => "it is already done; its evidence is in the plan",
            RoadmapStatus::Blocked | RoadmapStatus::AwaitingApproval => {
                "it is waiting on somebody; say so rather than closing it"
            }
            RoadmapStatus::InProgress => "",
        };
        Err(AxError::failure(
            AxCode::InvalidArgs,
            "close a plan row",
            format!("row {} is `{}`", row.index, spelling(row.status)),
        )
        .with_recovery(hint.to_owned()))
    }
}

fn spelling(status: RoadmapStatus) -> &'static str {
    kernel::ROADMAP_STATUS_SPELLINGS
        .iter()
        .find(|(known, _)| *known == status)
        .map_or("Not started", |(_, text)| *text)
}

/// The tool itself: a thin router onto the desk.
pub struct ClaimTool {
    meta: ToolMeta,
    desk: Rc<RefCell<ClaimDesk>>,
}

impl ClaimTool {
    /// # Errors
    /// Propagates a malformed tool name or parameter schema, neither of
    /// which can happen with the literals below.
    pub fn new(desk: Rc<RefCell<ClaimDesk>>) -> Result<ClaimTool, AxError> {
        let room = desk.borrow().room.clone();
        let mut properties = Map::new();
        for (field, kind, description) in [
            (
                "action",
                "string",
                "`list` to see what is unclaimed, `claim` to take a row, `finish` to close one \
                 with evidence, `release` to hand one back",
            ),
            (
                "row",
                "integer",
                "claim, finish and release: the row number from the plan's first column",
            ),
            (
                "evidence",
                "string",
                "finish only: a retrievable locator, `cas:<hash>` or `file:<path>@<oid>`",
            ),
        ] {
            let mut spec = Map::new();
            spec.insert("type".to_owned(), Value::String(kind.to_owned()));
            spec.insert(
                "description".to_owned(),
                Value::String(description.to_owned()),
            );
            properties.insert(field.to_owned(), Value::Object(spec));
        }
        let mut params = Map::new();
        params.insert("type".to_owned(), Value::String("object".to_owned()));
        params.insert("properties".to_owned(), Value::Object(properties));
        params.insert(
            "required".to_owned(),
            Value::Array(vec![Value::String("action".to_owned())]),
        );
        Ok(ClaimTool {
            meta: ToolMeta {
                name: ToolName::parse("plan")?,
                disclosure: "Read this building's plan and take a row to work on; call it before \
                             starting work nobody assigned you."
                    .to_owned(),
                params: Payload::new(params)?,
                effect: Effect::Write { domain: room },
                cost_tier: CostTier::Free,
                timeout: None,
                render: RenderIntent::Generic,
                temporal: Temporal::Timeless,
            },
            desk,
        })
    }
}

impl Tool for ClaimTool {
    fn meta(&self) -> &ToolMeta {
        &self.meta
    }

    fn invoke(&mut self, call: &ToolCall) -> Result<ToolOutcome, AxError> {
        if call.name != self.meta.name {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "read the plan",
                format!("call routed to the wrong tool: {}", call.name.as_str()),
            ));
        }
        let args = call.args.as_map();
        let mut desk = self.desk.try_borrow_mut().map_err(|_| {
            AxError::failure(
                AxCode::InvalidArgs,
                "reach the plan desk",
                "the desk is already in use",
            )
            .with_recovery("call the tool once at a time")
        })?;
        let action = args.get("action").and_then(Value::as_str).ok_or_else(|| {
            AxError::failure(
                AxCode::InvalidArgs,
                "read a plan action",
                "missing string argument `action`",
            )
            .with_recovery("use `list`, `claim`, `finish` or `release`")
        })?;
        let result = match action {
            "list" => desk.list()?,
            "claim" => desk.claim(row_of(args)?)?,
            "finish" => {
                let evidence = args
                    .get("evidence")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        AxError::failure(
                            AxCode::EvidenceMissing,
                            "finish a plan row",
                            "missing string argument `evidence`",
                        )
                        .with_recovery(
                            "pass a retrievable locator: `cas:<hash>` or `file:<path>@<oid>`",
                        )
                    })?;
                desk.finish(row_of(args)?, evidence)?
            }
            "release" => desk.release(row_of(args)?)?,
            other => {
                return Err(AxError::failure(
                    AxCode::InvalidArgs,
                    "read a plan action",
                    other.to_owned(),
                )
                .with_recovery("use `list`, `claim`, `finish` or `release`"));
            }
        };
        Ok(ToolOutcome { result })
    }
}

fn row_of(args: &Map<String, Value>) -> Result<u64, AxError> {
    args.get("row").and_then(Value::as_u64).ok_or_else(|| {
        AxError::failure(
            AxCode::InvalidArgs,
            "read a plan row number",
            "missing integer argument `row`",
        )
        .with_recovery("pass `row` as the number in the plan's first column")
    })
}

/// Whether the row on disk still holds what the effect assumed. The
/// worker asks this before writing, so a concurrent run degrades to "the
/// second claim did not take and said so" rather than "two runs each
/// believe they own the row".
#[must_use]
pub fn still_true(text: &str, effect: &ClaimEffect) -> bool {
    let RoadmapShape::WellFormed { rows } = check_roadmap_shape(text) else {
        return false;
    };
    rows.iter()
        .find(|row| row.index == effect.index())
        .is_some_and(|row| row.status == effect.expected_before())
}

/// Whether the plan's evidence column can be retrieved for this row —
/// the reader half of the same contract `set_roadmap_status` enforces on
/// the writing side.
#[must_use]
pub fn evidence_of(text: &str, index: u64) -> Option<Locator> {
    let RoadmapShape::WellFormed { rows } = check_roadmap_shape(text) else {
        return None;
    };
    rows.iter()
        .find(|row| row.index == index)
        .and_then(|row| match &row.evidence {
            EvidenceCell::Present(locator) => Some(locator.clone()),
            EvidenceCell::Empty | EvidenceCell::Invalid { .. } => None,
        })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code"
)]
mod tests {
    use super::*;

    const PLAN: &str = "\
# Roadmap

| # | Item | Status | Evidence |
|---|------|--------|----------|
| 1 | wire the kiln | Not started | |
| 2 | glaze tests | In progress | |
| 3 | fire the batch | Not started | |
";

    fn desk() -> Rc<RefCell<ClaimDesk>> {
        Rc::new(RefCell::new(ClaimDesk::new(
            "potter@lab.1".to_owned(),
            Address::parse("lab/room1").unwrap(),
            PLAN.to_owned(),
        )))
    }

    fn call(args: Value) -> ToolCall {
        ToolCall {
            id: "tu_1".to_owned(),
            name: ToolName::parse("plan").unwrap(),
            args: Payload::new(args.as_object().unwrap().clone()).unwrap(),
        }
    }

    fn locator() -> String {
        format!("cas:b3-{}", "ab".repeat(32))
    }

    #[test]
    fn claiming_a_free_row_marks_it_and_queues_one_effect() {
        let shared = desk();
        let mut tool = ClaimTool::new(Rc::clone(&shared)).unwrap();
        let outcome = tool
            .invoke(&call(serde_json::json!({ "action": "claim", "row": 1 })))
            .unwrap();
        assert_eq!(
            outcome.result.as_map().get("item").and_then(Value::as_str),
            Some("wire the kiln")
        );
        let mut borrowed = shared.borrow_mut();
        let text = borrowed.roadmap().expect("the plan changed").to_owned();
        assert!(text.contains("| 1 | wire the kiln | In progress |  |"));
        let effects = borrowed.take_effects();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].index(), 1);
        assert!(
            borrowed.take_effects().is_empty(),
            "an effect read twice would be a claim recorded twice"
        );
    }

    #[test]
    fn a_row_somebody_else_is_working_on_is_refused_with_a_free_one() {
        let shared = desk();
        let mut tool = ClaimTool::new(Rc::clone(&shared)).unwrap();
        let refusal = tool
            .invoke(&call(serde_json::json!({ "action": "claim", "row": 2 })))
            .unwrap_err();
        assert_eq!(refusal.code(), &AxCode::GoalConflict);
        assert!(refusal.subject().contains("In progress"));
        assert!(
            refusal.recovery().contains("row 1"),
            "the third part names a row the caller may actually take: {}",
            refusal.recovery()
        );
        assert!(
            shared.borrow().roadmap().is_none(),
            "a refusal writes nothing"
        );
    }

    #[test]
    fn a_run_holds_one_row_at_a_time() {
        let shared = desk();
        let mut tool = ClaimTool::new(Rc::clone(&shared)).unwrap();
        tool.invoke(&call(serde_json::json!({ "action": "claim", "row": 1 })))
            .unwrap();
        let refusal = tool
            .invoke(&call(serde_json::json!({ "action": "claim", "row": 3 })))
            .unwrap_err();
        assert!(refusal.subject().contains("row 1"));
        assert!(refusal.recovery().contains("one row"));
    }

    #[test]
    fn finishing_requires_evidence_that_parses() {
        let shared = desk();
        let mut tool = ClaimTool::new(Rc::clone(&shared)).unwrap();
        let missing = tool
            .invoke(&call(serde_json::json!({ "action": "finish", "row": 2 })))
            .unwrap_err();
        assert_eq!(missing.code(), &AxCode::EvidenceMissing);
        let unparsable = tool
            .invoke(&call(serde_json::json!({
                "action": "finish", "row": 2, "evidence": "trust me"
            })))
            .unwrap_err();
        assert_eq!(unparsable.code(), &AxCode::LocatorInvalid);
        let done = tool
            .invoke(&call(serde_json::json!({
                "action": "finish", "row": 2, "evidence": locator()
            })))
            .unwrap();
        assert!(done.result.as_map().contains_key("evidence"));
        let text = shared.borrow().roadmap().unwrap().to_owned();
        assert_eq!(
            evidence_of(&text, 2).map(|l| l.to_string()),
            Some(locator()),
            "what was written is what a reader retrieves"
        );
    }

    #[test]
    fn finishing_a_row_nobody_started_is_refused_with_the_step_that_was_skipped() {
        let shared = desk();
        let mut tool = ClaimTool::new(Rc::clone(&shared)).unwrap();
        let refusal = tool
            .invoke(&call(serde_json::json!({
                "action": "finish", "row": 3, "evidence": locator()
            })))
            .unwrap_err();
        assert!(refusal.subject().contains("Not started"));
        assert!(refusal.recovery().contains("claim it first"));
    }

    #[test]
    fn releasing_puts_the_row_back_where_another_run_can_take_it() {
        let shared = desk();
        let mut tool = ClaimTool::new(Rc::clone(&shared)).unwrap();
        tool.invoke(&call(serde_json::json!({ "action": "release", "row": 2 })))
            .unwrap();
        let text = shared.borrow().roadmap().unwrap().to_owned();
        assert!(text.contains("| 2 | glaze tests | Not started |  |"));
        let effects = shared.borrow_mut().take_effects();
        assert!(matches!(effects[0], ClaimEffect::Released { index: 2, .. }));
    }

    #[test]
    fn listing_separates_free_rows_from_taken_ones() {
        let shared = desk();
        let mut tool = ClaimTool::new(Rc::clone(&shared)).unwrap();
        let outcome = tool
            .invoke(&call(serde_json::json!({ "action": "list" })))
            .unwrap();
        let map = outcome.result.as_map();
        assert_eq!(
            map.get("unclaimed")
                .and_then(Value::as_array)
                .unwrap()
                .len(),
            2
        );
        assert_eq!(
            map.get("in_progress")
                .and_then(Value::as_array)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(map.get("rows_total").and_then(Value::as_u64), Some(3));
        assert!(
            shared.borrow().roadmap().is_none(),
            "reading the plan does not modify it"
        );
    }

    #[test]
    fn an_effect_whose_row_moved_underneath_it_is_no_longer_true() {
        let shared = desk();
        let mut tool = ClaimTool::new(Rc::clone(&shared)).unwrap();
        tool.invoke(&call(serde_json::json!({ "action": "claim", "row": 1 })))
            .unwrap();
        let effects = shared.borrow_mut().take_effects();
        assert!(
            still_true(PLAN, &effects[0]),
            "against the file it was decided on, the effect holds"
        );
        let moved = set_roadmap_status(PLAN, 1, RoadmapStatus::InProgress, None).unwrap();
        assert!(
            !still_true(&moved, &effects[0]),
            "somebody else took the row first; the claim does not take"
        );
    }

    #[test]
    fn a_plan_that_does_not_parse_refuses_with_the_repair() {
        let shared = Rc::new(RefCell::new(ClaimDesk::new(
            "potter@lab.1".to_owned(),
            Address::parse("lab/room1").unwrap(),
            "no table here".to_owned(),
        )));
        let mut tool = ClaimTool::new(Rc::clone(&shared)).unwrap();
        let refusal = tool
            .invoke(&call(serde_json::json!({ "action": "list" })))
            .unwrap_err();
        assert!(refusal.recovery().contains("four-column"));
    }

    #[test]
    fn the_tool_refuses_a_call_bearing_another_tools_name() {
        let mut tool = ClaimTool::new(desk()).unwrap();
        let refusal = tool
            .invoke(&ToolCall {
                id: "tu_x".to_owned(),
                name: ToolName::parse("signal").unwrap(),
                args: Payload::empty(),
            })
            .unwrap_err();
        assert_eq!(refusal.code(), &AxCode::InvalidArgs);
        assert_eq!(
            tool.meta().name.as_str(),
            "plan",
            "a refusal must not poison the tool"
        );
    }
}
