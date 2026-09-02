// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Red, and how far it reaches.
//!
//! **Red is not a new mechanism.** Every source of it is a fact the city
//! already records — a run that froze without citing anything, a door
//! that has waited too long for a person, `crate::stall` finding a run
//! going nowhere, or a resident saying in as many words that it is
//! stuck. This module answers the one question those facts do not: given
//! that `2.3.1` is red, what else in the plan cannot move?
//!
//! **The answer names the source, not the symptom.** A plan with one
//! real problem produces one blockage that says which node it is and
//! which nodes are waiting behind it, rather than seventeen red dots a
//! person has to trace back by hand. That is the whole reason to
//! propagate at all: the first screen has room for a cause, not for a
//! list of consequences.
//!
//! Red travels two ways, and both are the same relation seen from two
//! sides: **up the tree**, because a branch whose child is stuck is
//! stuck, and **along the dependency edges**, because a node that waits
//! for a red node waits for something that is not coming. Nothing here
//! reads a clock or a disk; the caller supplies the facts and gets a
//! verdict.

use std::collections::{BTreeMap, BTreeSet};

use crate::plan::{NodeId, PlanTree, StopCause};

/// One node the city has a reason to call red.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RedNode {
    pub at: NodeId,
    pub why: StopCause,
}

/// One cause and everything waiting behind it.
///
/// `reaches` never contains `source`: a reader draws the source once,
/// and a list that repeated it would make a single problem look like
/// two.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Blockage {
    pub source: NodeId,
    pub why: StopCause,
    /// Every node stuck because of this one, in reading order.
    pub reaches: Vec<NodeId>,
}

impl Blockage {
    /// The sentence the first screen shows: what is stuck, where, and
    /// why. One clause each, in the order a reader needs them.
    #[must_use]
    pub fn line(&self) -> String {
        let branch = self
            .source
            .ancestors()
            .last()
            .map_or_else(|| self.source.to_string(), |top| top.to_string());
        format!(
            "branch {branch} is stuck at {}: {}",
            self.source,
            self.why.line()
        )
    }
}

/// Somebody to tell, and what about.
///
/// The city already has a way for one resident to reach another; what it
/// lacked was a fact that triggers one without a person asking. A node
/// going red is that fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Notice {
    /// Who holds a node that is now waiting for a red one.
    pub to: String,
    pub about: NodeId,
    pub line: String,
}

/// Which nodes each red one reaches.
///
/// Reported per cause, sorted by the cause's own index, and a node
/// already red on its own account is never listed as somebody else's
/// consequence — it has its own entry, and a reader following the list
/// downwards meets each problem once.
#[must_use]
pub fn spread(tree: &PlanTree, red: &[RedNode]) -> Vec<Blockage> {
    let sources: BTreeSet<NodeId> = red.iter().map(|held| held.at.clone()).collect();
    let dependents = reverse_edges(tree);
    let mut found: BTreeMap<NodeId, Blockage> = BTreeMap::new();
    for held in red {
        if tree.get(&held.at).is_none() {
            continue;
        }
        let mut reaches: BTreeSet<NodeId> = BTreeSet::new();
        walk(&held.at, &dependents, &mut reaches);
        reaches.remove(&held.at);
        for other in &sources {
            reaches.remove(other);
        }
        found.insert(
            held.at.clone(),
            Blockage {
                source: held.at.clone(),
                why: held.why.clone(),
                reaches: reaches.into_iter().collect(),
            },
        );
    }
    found.into_values().collect()
}

/// One line for each resident holding work behind a red node.
///
/// A resident is told once per blockage rather than once per node it
/// holds: a mailbox with four copies of one problem is a mailbox nobody
/// reads. The resident that caused the blockage is not told about its
/// own — it is the one that reported it.
#[must_use]
pub fn notices(blocked: &[Blockage], holders: &BTreeMap<NodeId, String>) -> Vec<Notice> {
    let mut out = Vec::new();
    for blockage in blocked {
        let source_holder = holders.get(&blockage.source);
        let mut told: BTreeSet<&String> = BTreeSet::new();
        for node in &blockage.reaches {
            let Some(who) = holders.get(node) else {
                continue;
            };
            if Some(who) == source_holder || !told.insert(who) {
                continue;
            }
            out.push(Notice {
                to: who.clone(),
                about: blockage.source.clone(),
                line: blockage.line(),
            });
        }
    }
    out
}

/// Everything downstream of one node, through both relations.
///
/// Bounded by the size of the plan: `seen` grows by one every time the
/// walk goes further, and stops when it does not.
fn walk(from: &NodeId, dependents: &BTreeMap<NodeId, Vec<NodeId>>, seen: &mut BTreeSet<NodeId>) {
    if !seen.insert(from.clone()) {
        return;
    }
    for parent in from.ancestors() {
        walk(&parent, dependents, seen);
    }
    for waiting in dependents.get(from).into_iter().flatten() {
        walk(waiting, dependents, seen);
    }
}

/// Who waits for whom, the other way round.
///
/// A node's own `needs` plus the `needs` of every branch above it: a
/// child waits for what its branch waits for, which is the same rule
/// `PlanTree::ready` applies, read backwards.
fn reverse_edges(tree: &PlanTree) -> BTreeMap<NodeId, Vec<NodeId>> {
    let mut edges: BTreeMap<NodeId, Vec<NodeId>> = BTreeMap::new();
    for node in tree.nodes() {
        for need in tree.needs_of(&node.row.id) {
            edges.entry(need).or_default().push(node.row.id.clone());
        }
    }
    edges
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
    use crate::spine::{RoadmapShape, check_roadmap_shape};

    fn tree(text: &str) -> PlanTree {
        let RoadmapShape::WellFormed { rows } = check_roadmap_shape(text) else {
            panic!("the fixture parses");
        };
        PlanTree::build(rows).expect("the fixture is a tree")
    }

    fn node(raw: &str) -> NodeId {
        NodeId::parse(raw).unwrap()
    }

    fn stuck(at: &str) -> RedNode {
        RedNode {
            at: node(at),
            why: StopCause::Blocked {
                note: "the kiln is cold".to_owned(),
            },
        }
    }

    const PLAN: &str = "\
| # | Item | Weight | Needs | Status | Evidence |
|---|------|--------|-------|--------|----------|
| 1 | groundwork | 1 |  | Not started |  |
| 2 | build | 1 |  | Not started |  |
| 2.1 | frame | 1 | 1 | Not started |  |
| 2.2 | roof | 1 | 2.1 | Not started |  |
| 2.3 | door | 1 | 2.2 | Not started |  |
| 3 | ship | 1 | 2 | Not started |  |
";

    #[test]
    fn red_climbs_the_branch_and_runs_down_the_dependencies() {
        let blocked = spread(&tree(PLAN), &[stuck("2.1")]);
        assert_eq!(blocked.len(), 1);
        let reached: Vec<&str> = blocked[0].reaches.iter().map(NodeId::as_str).collect();
        assert_eq!(
            reached,
            ["2", "2.2", "2.3", "3"],
            "the branch above, what waits for it, and what waits for those"
        );
        assert!(!blocked[0].reaches.contains(&node("1")), "1 is upstream");
        assert!(!blocked[0].reaches.contains(&node("2.1")), "not itself");
    }

    /// The reason this module exists: one problem is one line naming the
    /// source, not four red dots a person has to trace.
    #[test]
    fn the_line_names_the_branch_and_the_node_it_is_stuck_at() {
        let blocked = spread(&tree(PLAN), &[stuck("2.1")]);
        assert_eq!(
            blocked[0].line(),
            "branch 2 is stuck at 2.1: the kiln is cold"
        );
    }

    #[test]
    fn a_node_red_on_its_own_account_is_not_listed_as_somebody_elses_consequence() {
        let blocked = spread(&tree(PLAN), &[stuck("2.1"), stuck("2.3")]);
        assert_eq!(blocked.len(), 2);
        assert!(
            !blocked[0].reaches.contains(&node("2.3")),
            "2.3 has its own entry"
        );
        assert_eq!(blocked[0].source, node("2.1"));
        assert_eq!(blocked[1].source, node("2.3"));
    }

    #[test]
    fn a_red_node_the_plan_does_not_carry_is_dropped_rather_than_invented() {
        assert!(spread(&tree(PLAN), &[stuck("9.9")]).is_empty());
    }

    #[test]
    fn everyone_waiting_behind_it_is_told_once_and_the_reporter_is_not() {
        let blocked = spread(&tree(PLAN), &[stuck("2.1")]);
        let mut holders = BTreeMap::new();
        holders.insert(node("2.1"), "mason@yard.1".to_owned());
        holders.insert(node("2.2"), "roofer@yard.1".to_owned());
        holders.insert(node("2.3"), "roofer@yard.1".to_owned());
        holders.insert(node("3"), "carter@yard.1".to_owned());
        let told = notices(&blocked, &holders);
        let who: Vec<&str> = told.iter().map(|notice| notice.to.as_str()).collect();
        assert_eq!(
            who,
            ["roofer@yard.1", "carter@yard.1"],
            "the roofer holds two of them and hears once; the mason reported it"
        );
        assert_eq!(told[0].about, node("2.1"));
    }

    #[test]
    fn nothing_red_tells_nobody() {
        let blocked = spread(&tree(PLAN), &[]);
        assert!(blocked.is_empty());
        assert!(notices(&blocked, &BTreeMap::new()).is_empty());
    }
}
