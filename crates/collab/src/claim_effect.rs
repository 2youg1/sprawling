// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! What a claim on a plan node left behind, and whether the file still
//! agrees with it.
//!
//! **A value, not a decision.** `ClaimEffect` records what happened so
//! the desk that produced it does not have to be consulted again, and
//! `still_true` asks the one question that cannot be answered from the
//! record alone: does the document on disk still say what this effect
//! says it made it say. `ClaimDesk` decides; this describes and checks.
//! Two shapes, so two files (ARCHITECTURE.md section 9).

use kernel::{AxCode, AxError, EvidenceCell, Locator, NodeId, Payload, PlanExit};
use kernel::{RoadmapShape, RoadmapStatus, check_roadmap_shape};
use serde_json::{Map, Value};

/// What the run did to the plan. Exhaustive on purpose, like the other
/// desks': every variant is a line the worker has to write, so a new one
/// must be a compile error where the writing happens rather than a
/// runtime arm nobody reaches until a claim quietly goes unrecorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClaimEffect {
    Claimed {
        id: NodeId,
        item: String,
    },
    /// A held node put down. Carries the exit rather than a verb,
    /// because the exit is the thing the plan gate produced and copying
    /// its two arms into a second enum would be a second opinion about
    /// how a node may leave.
    PutDown {
        id: NodeId,
        item: String,
        exit: PlanExit,
    },
    Split {
        parent: NodeId,
        children: Vec<String>,
    },
}

impl ClaimEffect {
    /// The node the effect concerns, which is what the worker re-reads
    /// the file against before writing.
    #[must_use]
    pub fn id(&self) -> &NodeId {
        match self {
            ClaimEffect::Claimed { id, .. } | ClaimEffect::PutDown { id, .. } => id,
            ClaimEffect::Split { parent, .. } => parent,
        }
    }

    /// The status the node must already be in for this effect to still be
    /// the truth when the worker applies it.
    #[must_use]
    pub fn expected_before(&self) -> RoadmapStatus {
        match self {
            ClaimEffect::Claimed { .. } => RoadmapStatus::NotStarted,
            ClaimEffect::PutDown { .. } => RoadmapStatus::InProgress,
            // A split does not move the parent, so what must still hold
            // is only that the parent is where it was.
            ClaimEffect::Split { .. } => RoadmapStatus::InProgress,
        }
    }

    /// Which record this becomes.
    #[must_use]
    pub fn kind(&self) -> kernel::EventKind {
        match self {
            ClaimEffect::Claimed { .. } => kernel::EventKind::RoadmapClaimed,
            ClaimEffect::Split { .. } => kernel::EventKind::RoadmapSplit,
            ClaimEffect::PutDown { exit, .. } => match exit {
                PlanExit::Finished { .. } => kernel::EventKind::RoadmapFinished,
                PlanExit::Stopped { why, .. } if why.is_red() => kernel::EventKind::RoadmapBlocked,
                PlanExit::Stopped { .. } => kernel::EventKind::RoadmapReleased,
            },
        }
    }

    /// The `roadmap_*` payload: what a rebuild reads back.
    ///
    /// # Errors
    /// Propagates the payload's refusal to hold what it was given.
    pub fn payload(&self, who: &str) -> Result<Payload, AxError> {
        let mut map = Map::new();
        map.insert("by".to_owned(), Value::String(who.to_owned()));
        map.insert("node".to_owned(), Value::String(self.id().to_string()));
        match self {
            ClaimEffect::Claimed { item, .. } => {
                map.insert("verb".to_owned(), Value::String("claimed".to_owned()));
                map.insert("item".to_owned(), Value::String(item.clone()));
            }
            ClaimEffect::Split { children, .. } => {
                map.insert("verb".to_owned(), Value::String("split".to_owned()));
                map.insert(
                    "children".to_owned(),
                    Value::Array(
                        children
                            .iter()
                            .map(|text| Value::String(text.clone()))
                            .collect(),
                    ),
                );
            }
            ClaimEffect::PutDown { item, exit, .. } => {
                map.insert("item".to_owned(), Value::String(item.clone()));
                match exit {
                    PlanExit::Finished { evidence, .. } => {
                        map.insert("verb".to_owned(), Value::String("finished".to_owned()));
                        map.insert("evidence".to_owned(), Value::String(evidence.to_string()));
                    }
                    PlanExit::Stopped { why, .. } => {
                        map.insert(
                            "verb".to_owned(),
                            Value::String(
                                if why.is_red() { "blocked" } else { "released" }.to_owned(),
                            ),
                        );
                        map.insert(
                            "why".to_owned(),
                            serde_json::to_value(why).map_err(|err| {
                                AxError::failure(
                                    AxCode::InvalidArgs,
                                    "record why a plan node stopped",
                                    err.to_string(),
                                )
                            })?,
                        );
                        map.insert("line".to_owned(), Value::String(why.line()));
                    }
                }
            }
        }
        Payload::new(map)
    }
}

/// Whether the plan's evidence column can be retrieved for this node —
/// the reader half of the same contract `set_roadmap_status` enforces on
/// the writing side.
#[must_use]
pub fn evidence_of(text: &str, id: &NodeId) -> Option<Locator> {
    let RoadmapShape::WellFormed { rows } = check_roadmap_shape(text) else {
        return None;
    };
    rows.iter()
        .find(|row| &row.id == id)
        .and_then(|row| match &row.evidence {
            EvidenceCell::Present(locator) => Some(locator.clone()),
            EvidenceCell::Empty | EvidenceCell::Invalid { .. } => None,
        })
}

/// Whether the node on disk still holds what the effect assumed. The
/// worker asks this before writing, so a concurrent run degrades to "the
/// second claim did not take and said so" rather than "two runs each
/// believe they own the node".
#[must_use]
pub fn still_true(text: &str, effect: &ClaimEffect) -> bool {
    let RoadmapShape::WellFormed { rows } = check_roadmap_shape(text) else {
        return false;
    };
    rows.iter()
        .find(|row| &row.id == effect.id())
        .is_some_and(|row| row.status == effect.expected_before())
}
