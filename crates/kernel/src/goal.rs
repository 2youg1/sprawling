// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Same-resource mutual exclusion. Detection is
//! kernel's; arbitration is not — reading statements and judging intent
//! belongs to models (collab::arbiter, P2) and humans.

use serde::{Deserialize, Serialize};

use crate::address::Address;

/// Non-empty goal identity; uniqueness bookkeeping is the caller's.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct GoalId(String);

impl GoalId {
    pub fn new(raw: impl Into<String>) -> Option<GoalId> {
        let raw = raw.into();
        if raw.is_empty() {
            None
        } else {
            Some(GoalId(raw))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// What a goal claims. Paths conflict on prefix overlap either way;
/// external names conflict on equality; the two kinds never conflict
/// with each other.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GoalResource {
    Path(Address),
    External(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GoalEntry {
    pub id: GoalId,
    pub owner: String,
    pub resources: Vec<GoalResource>,
    pub statement: String,
    pub standing: bool,
}

/// Deliberately exhaustive verdict.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalVerdict {
    Clear,
    Conflict { with: GoalId },
}

fn resources_clash(a: &GoalResource, b: &GoalResource) -> bool {
    match (a, b) {
        (GoalResource::Path(pa), GoalResource::Path(pb)) => pa.is_within(pb) || pb.is_within(pa),
        (GoalResource::External(ea), GoalResource::External(eb)) => ea == eb,
        _ => false,
    }
}

/// First conflict wins, in the registered slice's order (deterministic:
/// the caller's table order is the clock). A candidate re-submitting its
/// own id is idempotent, not a conflict.
pub fn detect_conflict(registered: &[GoalEntry], candidate: &GoalEntry) -> GoalVerdict {
    for entry in registered {
        if entry.id == candidate.id {
            continue;
        }
        for held in &entry.resources {
            for wanted in &candidate.resources {
                if resources_clash(held, wanted) {
                    return GoalVerdict::Conflict {
                        with: entry.id.clone(),
                    };
                }
            }
        }
    }
    GoalVerdict::Clear
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

    fn entry(id: &str, resources: Vec<GoalResource>) -> GoalEntry {
        GoalEntry {
            id: GoalId::new(id).unwrap(),
            owner: "worker@sim.1".into(),
            resources,
            statement: "hold".into(),
            standing: true,
        }
    }

    fn path(raw: &str) -> GoalResource {
        GoalResource::Path(Address::parse(raw).unwrap())
    }

    #[test]
    fn prefix_overlap_conflicts_both_ways() {
        let held = entry("g1", vec![path("b/docs")]);
        let narrower = entry("g2", vec![path("b/docs/ch1")]);
        let wider = entry("g3", vec![path("b")]);
        assert_eq!(
            detect_conflict(std::slice::from_ref(&held), &narrower),
            GoalVerdict::Conflict {
                with: GoalId::new("g1").unwrap()
            }
        );
        assert_eq!(
            detect_conflict(&[held], &wider),
            GoalVerdict::Conflict {
                with: GoalId::new("g1").unwrap()
            }
        );
    }

    #[test]
    fn externals_conflict_on_equality_only() {
        let held = entry("g1", vec![GoalResource::External("crates.io/foo".into())]);
        let same = entry("g2", vec![GoalResource::External("crates.io/foo".into())]);
        let other = entry("g3", vec![GoalResource::External("crates.io/bar".into())]);
        assert!(matches!(
            detect_conflict(std::slice::from_ref(&held), &same),
            GoalVerdict::Conflict { .. }
        ));
        assert_eq!(detect_conflict(&[held], &other), GoalVerdict::Clear);
    }

    #[test]
    fn path_and_external_never_clash_and_resubmission_is_idempotent() {
        let held = entry("g1", vec![path("b/docs")]);
        let external = entry("g2", vec![GoalResource::External("b/docs".into())]);
        assert_eq!(
            detect_conflict(std::slice::from_ref(&held), &external),
            GoalVerdict::Clear
        );
        let resubmitted = entry("g1", vec![path("b/docs")]);
        assert_eq!(detect_conflict(&[held], &resubmitted), GoalVerdict::Clear);
    }
}
