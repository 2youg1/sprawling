// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Completion and progress. Claiming done makes
//! nothing done: `Completion::Done` cannot be built without in-window
//! evidence (A6's type half; the runtime depth check is S3's). Progress
//! is honest in the type: only a planned run owns a ratio method —
//! an unplanned run has nothing to ask a percentage from.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::budget::BudgetUse;
use crate::error::{AxCode, AxError};
use crate::event::{EventKind, EventRef};

/// Non-empty, and every ref is a `tool_result` or `model_returned`.
/// There is no other constructor and no Deserialize anywhere in the
/// chain — history cannot be claimed, only cited.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Evidence(Vec<EventRef>);

impl Evidence {
    pub fn new(refs: Vec<EventRef>) -> Result<Evidence, AxError> {
        if refs.is_empty() {
            return Err(AxError::failure(
                AxCode::EvidenceMissing,
                "construct evidence",
                "empty ref list",
            )
            .with_recovery("cite at least one tool_result or model_returned event"));
        }
        if let Some(bad) = refs
            .iter()
            .find(|r| !matches!(r.kind(), EventKind::ToolResult | EventKind::ModelReturned))
        {
            return Err(AxError::failure(
                AxCode::EvidenceMissing,
                "construct evidence",
                format!("ref kind {:?}", bad.kind()),
            )
            .with_recovery("evidence kinds are tool_result and model_returned only"));
        }
        Ok(Evidence(refs))
    }

    pub fn refs(&self) -> &[EventRef] {
        &self.0
    }
}

/// The three endings; a fourth cannot be represented (frozen surface,
/// 14.1). `Limit` is not a kind of done — record limit, not completion.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Completion {
    Done(Evidence),
    Limit,
    Cancelled,
}

impl Completion {
    pub fn name(&self) -> &'static str {
        match self {
            Completion::Done(_) => "done",
            Completion::Limit => "limit",
            Completion::Cancelled => "cancelled",
        }
    }

    /// One-way projection into an event payload: `completion` plus, for
    /// Done, the evidence as `{seq, kind}` pairs. Deliberately no serde —
    /// deserializing a Completion would mint EventRefs out of thin air.
    pub fn extend_payload(&self, map: &mut Map<String, Value>) -> Result<(), AxError> {
        map.insert(
            "completion".to_owned(),
            Value::String(self.name().to_owned()),
        );
        if let Completion::Done(evidence) = self {
            let mut cited = Vec::new();
            for r in evidence.refs() {
                let mut entry = Map::new();
                entry.insert("seq".to_owned(), Value::Number(r.seq().value().into()));
                entry.insert(
                    "kind".to_owned(),
                    serde_json::to_value(r.kind()).map_err(|err| {
                        AxError::failure(AxCode::InvalidArgs, "encode evidence", err.to_string())
                    })?,
                );
                cited.push(Value::Object(entry));
            }
            map.insert("evidence".to_owned(), Value::Array(cited));
        }
        Ok(())
    }
}

/// Progress with a denominator: the Roadmap table rows.
///
/// Unlike [`Completion`], progress carries serde: it is a reading of a
/// file that the interface renders, not a claim that work finished. What
/// must not be deserialisable is a `Done` — and that still is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlannedProgress {
    pub done: u32,
    pub blocked: u32,
    pub total: u32,
}

impl PlannedProgress {
    /// `(done, total)` — the renderer computes its own percentage from a
    ///真 fraction, never from a claim.
    pub fn ratio(&self) -> (u32, u32) {
        (self.done, self.total)
    }
}

/// Progress without a denominator: steps walked and budget burned. No
/// ratio method exists — the interface cannot paint what it cannot know
/// (A17's type half).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct UnplannedProgress {
    pub steps: u32,
    pub budget: BudgetUse,
}

/// Deliberately exhaustive: both faces must be handled by every renderer.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Progress {
    Planned(PlannedProgress),
    Unplanned(UnplannedProgress),
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
    use crate::event::{EventDraft, EventRecord, Payload, RunId, Seq, TimeMs};
    use crate::ledger::GENESIS_PREV;

    fn evidence_ref(kind: EventKind) -> EventRef {
        let draft = EventDraft {
            run: RunId::CITY,
            t: TimeMs::new(0),
            who: "city".into(),
            addr: None,
            kind,
            data: Payload::empty(),
            ig: false,
        };
        EventRecord::from_draft(draft, Seq::FIRST, GENESIS_PREV).to_ref()
    }

    #[test]
    fn empty_or_wrong_kind_evidence_cannot_be_built() {
        assert_eq!(
            Evidence::new(vec![]).unwrap_err().code(),
            &AxCode::EvidenceMissing
        );
        let err = Evidence::new(vec![evidence_ref(EventKind::RunStarted)]).unwrap_err();
        assert_eq!(err.code(), &AxCode::EvidenceMissing);
    }

    #[test]
    fn done_payload_cites_its_evidence() {
        let done =
            Completion::Done(Evidence::new(vec![evidence_ref(EventKind::ModelReturned)]).unwrap());
        let mut map = Map::new();
        done.extend_payload(&mut map).unwrap();
        assert_eq!(map["completion"], "done");
        assert_eq!(map["evidence"][0]["kind"], "model_returned");
        let mut map = Map::new();
        Completion::Limit.extend_payload(&mut map).unwrap();
        assert_eq!(map["completion"], "limit");
        assert!(!map.contains_key("evidence"));
    }

    #[test]
    fn only_planned_progress_owns_a_ratio() {
        let planned = PlannedProgress {
            done: 3,
            blocked: 1,
            total: 9,
        };
        assert_eq!(planned.ratio(), (3, 9));
        // UnplannedProgress has no ratio method — nothing to assert at
        // runtime; the S2.11 trybuild case pins the absence.
        let unplanned = UnplannedProgress {
            steps: 12,
            budget: BudgetUse::default(),
        };
        assert_eq!(unplanned.steps, 12);
    }
}
