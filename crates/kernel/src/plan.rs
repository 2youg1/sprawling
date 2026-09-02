// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The plan as a tree: what the nodes are, what each one is worth, what
//! may be started now, and the two ways a held node is put down.
//!
//! `crate::spine` owns the document — how a row is written and read.
//! This module owns the structure that document describes, and the two
//! are separate because the grammar of a line and the arithmetic of a
//! plan fail in different ways: a mistyped row is repaired by editing
//! it, while a cycle in the dependencies is repaired by rethinking the
//! work.
//!
//! **Weight is conserved because [`crate::share`] is the only way to
//! hold any.** A node's share is divided out of its parent's when the
//! tree is built, so the total is 1 whatever the plan turns into and no
//! version number is needed for the denominator. What grows when a
//! branch is split is the number of leaves, not the total; that is why
//! the interface shows both — a percentage says how much is done and a
//! leaf count says how much was found.
//!
//! **Progress is counted on leaves only.** A branch has no work of its
//! own: its children are its work, and counting both would count the
//! same effort twice. A branch that says `Done` while a child does not
//! is refused where the tree is built, so the status column of a branch
//! is a summary a reader can trust rather than a second opinion.

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::completion::{PlannedProgress, Progress};
use crate::error::{AxCode, AxError};
use crate::locator::Locator;
use crate::share::{self, Share, WHOLE_PPB};
use crate::spine::{EvidenceCell, RoadmapRow, RoadmapStatus};

/// The longest branch a plan may grow. Ten levels of `1.1.1.…` is a
/// depth no plan has ever needed and a length past which the index
/// column stops being readable; refusing it also bounds every walk in
/// this module.
pub const NODE_DEPTH_MAX: usize = 10;

/// A node's place in the plan, as the first column spells it: `2`,
/// `2.3`, `2.3.1`. The parent is the prefix, so the tree needs no second
/// field to say where a node hangs.
///
/// Ordering is the reading order of the table — `1`, `1.1`, `1.2`, `2` —
/// because segment-wise comparison is exactly that order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct NodeId(String);

impl NodeId {
    /// Reads an index cell.
    ///
    /// # Errors
    /// Refuses an empty cell, a segment that is not a number, a zero
    /// segment (the table is one-based, and `0` next to `1` reads as a
    /// different node than it is), a leading zero (`01` and `1` would be
    /// two spellings of one node), and a branch deeper than
    /// [`NODE_DEPTH_MAX`].
    pub fn parse(raw: &str) -> Result<NodeId, AxError> {
        let text = raw.trim();
        let refuse = |why: String| {
            AxError::failure(AxCode::InvalidArgs, "read a plan index", why)
                .with_recovery("write the index as dotted numbers from one, such as `2.3.1`")
        };
        if text.is_empty() {
            return Err(refuse("the cell is empty".to_owned()));
        }
        let segments: Vec<&str> = text.split('.').collect();
        if segments.len() > NODE_DEPTH_MAX {
            return Err(refuse(format!(
                "{} levels deep, the plan holds {NODE_DEPTH_MAX}",
                segments.len()
            )));
        }
        for segment in &segments {
            if segment.is_empty() || !segment.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(refuse(format!("`{segment}` is not a number")));
            }
            if segment.starts_with('0') {
                return Err(refuse(format!("`{segment}` has a leading zero")));
            }
            if segment.parse::<u32>().is_err() {
                return Err(refuse(format!("`{segment}` does not fit a plan index")));
            }
        }
        Ok(NodeId(segments.join(".")))
    }

    /// The node this one hangs under, or `None` for a top-level node.
    #[must_use]
    pub fn parent(&self) -> Option<NodeId> {
        self.0
            .rsplit_once('.')
            .map(|(head, _)| NodeId(head.to_owned()))
    }

    /// The last segment: which child of its parent this is.
    #[must_use]
    pub fn ordinal(&self) -> u32 {
        self.0
            .rsplit_once('.')
            .map_or(self.0.as_str(), |(_, tail)| tail)
            .parse()
            .unwrap_or(0)
    }

    /// How many levels down this node sits; a top-level node is 1.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.0.split('.').count()
    }

    /// Whether `other` hangs below this node, at any depth. A node is
    /// not its own ancestor.
    #[must_use]
    pub fn is_ancestor_of(&self, other: &NodeId) -> bool {
        other.0.len() > self.0.len()
            && other.0.starts_with(&self.0)
            && other.0.as_bytes().get(self.0.len()) == Some(&b'.')
    }

    /// Every node between the root and this one, closest last.
    #[must_use]
    pub fn ancestors(&self) -> Vec<NodeId> {
        let mut found = Vec::new();
        let mut walking = self.parent();
        while let Some(held) = walking {
            walking = held.parent();
            found.push(held);
        }
        found.reverse();
        found
    }

    /// This node's `n`-th child.
    ///
    /// # Errors
    /// Refuses an ordinal of zero and a child past [`NODE_DEPTH_MAX`].
    pub fn child(&self, ordinal: u32) -> Result<NodeId, AxError> {
        NodeId::parse(&format!("{}.{ordinal}", self.0))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for NodeId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        NodeId::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// Why a held node was put down without evidence.
///
/// Exhaustive, and the whole point of the enum is the last field of
/// [`Self::returns_to`]: a resident that hands work back leaves it ready
/// for somebody else, while every other cause leaves it red. Collapsing
/// the two would either strand work nobody refused or paint the plan red
/// every time a resident ran out of budget.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopCause {
    /// A resident says this node cannot proceed, and why.
    Blocked { note: String },
    /// A resident is not the one to do this after all. Not red: the node
    /// goes back where another run can take it.
    HandedBack { note: String },
    /// The run holding it ended without citing anything.
    FrozeWithoutEvidence,
    /// `crate::stall` found the run going nowhere.
    Stalled { repeats: u32 },
    /// A door was escalated to a person and nobody has answered.
    GateOverdue { waited_ms: u64 },
}

impl StopCause {
    /// What the status column says afterwards.
    #[must_use]
    pub fn status(&self) -> RoadmapStatus {
        match self {
            StopCause::HandedBack { .. } => RoadmapStatus::NotStarted,
            StopCause::Blocked { .. }
            | StopCause::FrozeWithoutEvidence
            | StopCause::Stalled { .. }
            | StopCause::GateOverdue { .. } => RoadmapStatus::Blocked,
        }
    }

    /// Whether this stop is one the plan reports as red.
    #[must_use]
    pub fn is_red(&self) -> bool {
        !matches!(self, StopCause::HandedBack { .. })
    }

    /// One line, for a person reading the first screen.
    #[must_use]
    pub fn line(&self) -> String {
        match self {
            StopCause::Blocked { note } | StopCause::HandedBack { note } => note.clone(),
            StopCause::FrozeWithoutEvidence => {
                "the run holding it ended without citing anything".to_owned()
            }
            StopCause::Stalled { repeats } => {
                format!("the run repeated one action {repeats} times")
            }
            StopCause::GateOverdue { waited_ms } => {
                format!("a door has waited {waited_ms} ms for a person")
            }
        }
    }
}

/// A node this run holds.
///
/// **There is no third way to put it down.** The type has one private
/// field, so it is minted only by [`PlanTree::claim`], and it is
/// consumed only by [`Held::finish`] and [`Held::stop`] — one takes
/// evidence, the other takes a cause. A run that simply stops working
/// leaves the value with its owner, and the owner's freeze path spends
/// it on [`StopCause::FrozeWithoutEvidence`], which is where the red in
/// `crate::blockage` comes from.
#[derive(Debug)]
#[must_use = "a held node has to be finished with evidence or stopped with a cause"]
pub struct Held(NodeId);

impl Held {
    #[must_use]
    pub fn id(&self) -> &NodeId {
        &self.0
    }

    /// Green: the work is done and here is where to look.
    pub fn finish(self, evidence: Locator) -> PlanExit {
        PlanExit::Finished {
            id: self.0,
            evidence,
        }
    }

    /// Red, or back in the pool: either way it says why.
    pub fn stop(self, why: StopCause) -> PlanExit {
        PlanExit::Stopped { id: self.0, why }
    }
}

/// How a held node left. Exhaustive, and every arm carries its reason —
/// which is the whole of the plan gate.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PlanExit {
    Finished { id: NodeId, evidence: Locator },
    Stopped { id: NodeId, why: StopCause },
}

impl PlanExit {
    #[must_use]
    pub fn id(&self) -> &NodeId {
        match self {
            PlanExit::Finished { id, .. } | PlanExit::Stopped { id, .. } => id,
        }
    }

    /// The status the row carries after this exit.
    #[must_use]
    pub fn status(&self) -> RoadmapStatus {
        match self {
            PlanExit::Finished { .. } => RoadmapStatus::Done,
            PlanExit::Stopped { why, .. } => why.status(),
        }
    }

    /// The evidence cell after this exit: an exit without evidence
    /// clears it rather than leaving the last run's citation on a row
    /// that is no longer done.
    #[must_use]
    pub fn evidence(&self) -> Option<&Locator> {
        match self {
            PlanExit::Finished { evidence, .. } => Some(evidence),
            PlanExit::Stopped { .. } => None,
        }
    }
}

/// One node, placed: the row as written plus what the tree worked out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanNode {
    pub row: RoadmapRow,
    /// This node's part of the whole plan.
    pub share: Share,
    /// Its children, in table order. Empty means a leaf, and only
    /// leaves carry progress.
    pub children: Vec<NodeId>,
}

impl PlanNode {
    #[must_use]
    pub fn is_leaf(&self) -> bool {
        self.children.is_empty()
    }
}

/// A plan whose shape has been checked: every parent exists, every
/// dependency names a node, and no dependency runs in a circle.
///
/// Built once and read many times. The refusals are all at the
/// construction point, so a caller holding one of these never has to ask
/// whether the plan makes sense.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlanTree {
    nodes: BTreeMap<NodeId, PlanNode>,
}

impl PlanTree {
    /// Places every row and works out what each one is worth.
    ///
    /// # Errors
    /// Fail-closed, in the same shape as `crate::locator`: a plan that
    /// cannot be read is refused rather than guessed at, because every
    /// progress figure in the city is divided by it. Five refusals — a
    /// repeated index, a node whose parent is missing, a dependency on a
    /// node that is not there, a dependency in a circle, and a branch
    /// marked done over a child that is not.
    pub fn build(rows: Vec<RoadmapRow>) -> Result<PlanTree, AxError> {
        let mut nodes: BTreeMap<NodeId, PlanNode> = BTreeMap::new();
        for row in rows {
            let id = row.id.clone();
            if nodes.contains_key(&id) {
                return Err(refusal(
                    "place a plan node",
                    format!("index {id} appears twice"),
                    "give every row its own index; two rows with one number are two plans",
                ));
            }
            nodes.insert(
                id,
                PlanNode {
                    row,
                    share: Share::NONE,
                    children: Vec::new(),
                },
            );
        }
        let placed: Vec<NodeId> = nodes.keys().cloned().collect();
        for id in &placed {
            if let Some(parent) = id.parent() {
                let Some(held) = nodes.get_mut(&parent) else {
                    return Err(refusal(
                        "place a plan node",
                        format!("{id} hangs under {parent}, which the table does not carry"),
                        "write the parent row first; a plan cannot branch from nothing",
                    ));
                };
                held.children.push(id.clone());
            }
        }
        Self::check_dependencies(&nodes)?;
        Self::check_branch_claims(&nodes)?;
        let mut tree = PlanTree { nodes };
        tree.divide()?;
        Ok(tree)
    }

    /// Every dependency names a node that exists, is not the node
    /// itself, and does not close a circle.
    fn check_dependencies(nodes: &BTreeMap<NodeId, PlanNode>) -> Result<(), AxError> {
        for (id, node) in nodes {
            for need in &node.row.needs {
                if !nodes.contains_key(need) {
                    return Err(refusal(
                        "read a plan dependency",
                        format!("{id} needs {need}, which the table does not carry"),
                        "name a row the plan holds, or drop the dependency",
                    ));
                }
                if need == id {
                    return Err(refusal(
                        "read a plan dependency",
                        format!("{id} needs itself"),
                        "a node cannot wait for its own result",
                    ));
                }
            }
        }
        if let Some(circle) = first_cycle(nodes) {
            let drawn: Vec<String> = circle.iter().map(NodeId::to_string).collect();
            return Err(refusal(
                "read a plan dependency",
                format!("{} runs in a circle", drawn.join(" → ")),
                "drop one of those dependencies; nothing in a circle can ever start",
            ));
        }
        Ok(())
    }

    /// A branch that says `Done` over a child that is not is refused,
    /// which is what lets the status column of a branch be read as a
    /// summary instead of as a second opinion.
    fn check_branch_claims(nodes: &BTreeMap<NodeId, PlanNode>) -> Result<(), AxError> {
        for (id, node) in nodes {
            if node.children.is_empty() || node.row.status != RoadmapStatus::Done {
                continue;
            }
            for child in &node.children {
                let unfinished = nodes
                    .get(child)
                    .is_some_and(|held| held.row.status != RoadmapStatus::Done);
                if unfinished {
                    return Err(refusal(
                        "read a plan branch",
                        format!("{id} says done while {child} does not"),
                        "a branch is done when its children are; finish the child or reopen the \
                         branch",
                    ));
                }
            }
        }
        Ok(())
    }

    /// Divides the whole plan down the tree, top-level rows first.
    fn divide(&mut self) -> Result<(), AxError> {
        let roots: Vec<NodeId> = self
            .nodes
            .keys()
            .filter(|id| id.parent().is_none())
            .cloned()
            .collect();
        self.hand_out(Share::WHOLE, &roots)?;
        Ok(())
    }

    /// Gives `pot` to `among` by their weights, then recurses.
    fn hand_out(&mut self, pot: Share, among: &[NodeId]) -> Result<(), AxError> {
        if among.is_empty() {
            return Ok(());
        }
        let weights: Vec<u32> = among
            .iter()
            .map(|id| self.nodes.get(id).map_or(1, |node| node.row.weight))
            .collect();
        // Every weight zero is a table that says nothing about how this
        // level divides, and an even cut is the reading that keeps the
        // share where the rows are.
        let parts = if weights.iter().all(|weight| *weight == 0) {
            pot.split(&vec![1; among.len()])?
        } else {
            pot.split(&weights)?
        };
        for (id, part) in among.iter().zip(parts) {
            let children = {
                let Some(node) = self.nodes.get_mut(id) else {
                    continue;
                };
                node.share = part;
                node.children.clone()
            };
            self.hand_out(part, &children)?;
        }
        Ok(())
    }

    #[must_use]
    pub fn get(&self, id: &NodeId) -> Option<&PlanNode> {
        self.nodes.get(id)
    }

    /// Every node, in table order.
    pub fn nodes(&self) -> impl Iterator<Item = &PlanNode> {
        self.nodes.values()
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// What a node waits for, including everything its ancestors wait
    /// for. A child of a branch that is waiting is waiting too;
    /// answering otherwise would let `2.3.1` start while the reason
    /// `2.3` cannot start is still unresolved.
    #[must_use]
    pub fn needs_of(&self, id: &NodeId) -> BTreeSet<NodeId> {
        let mut found = BTreeSet::new();
        for step in id
            .ancestors()
            .into_iter()
            .chain(std::iter::once(id.clone()))
        {
            if let Some(node) = self.nodes.get(&step) {
                found.extend(node.row.needs.iter().cloned());
            }
        }
        found
    }

    /// What may be started right now: leaves nobody has taken, whose
    /// dependencies — their own and their branch's — are all done.
    ///
    /// Pure, and that is the point: a city with no ready nodes and
    /// nothing running has finished, and a stop condition that depended
    /// on I/O could not be trusted to say so.
    #[must_use]
    pub fn ready(&self) -> Vec<NodeId> {
        self.nodes
            .values()
            .filter(|node| node.is_leaf() && node.row.status == RoadmapStatus::NotStarted)
            .filter(|node| {
                self.needs_of(&node.row.id).iter().all(|need| {
                    self.nodes
                        .get(need)
                        .is_some_and(|held| held.row.status == RoadmapStatus::Done)
                })
            })
            .map(|node| node.row.id.clone())
            .collect()
    }

    /// Takes a node to work on.
    ///
    /// # Errors
    /// Refuses everything [`Self::ready`] leaves out, and says which of
    /// the three reasons applies: the index is not in the table, the
    /// node is a branch or already taken, or it is still waiting on
    /// something. The third names what it waits for, because "no" sends
    /// a model round the loop again while "1.4 is not done" does not.
    pub fn claim(&self, id: &NodeId) -> Result<Held, AxError> {
        let node = self.nodes.get(id).ok_or_else(|| {
            refusal(
                "claim a plan node",
                format!("no row numbered {id}"),
                format!("list the plan first; it carries {} rows", self.nodes.len()),
            )
        })?;
        if !node.is_leaf() {
            return Err(refusal(
                "claim a plan node",
                format!("{id} is a branch with {} children", node.children.len()),
                "claim one of its children; a branch is done when its children are",
            ));
        }
        if node.row.status != RoadmapStatus::NotStarted {
            // Two runs wanting one node is a goal conflict, and the
            // caller reads the code rather than the sentence.
            return Err(AxError::failure(
                AxCode::GoalConflict,
                "claim a plan node",
                format!("{id} is `{}`", node.row.status.spelling()),
            )
            .with_recovery(self.somewhere_else()));
        }
        let waiting: Vec<String> = self
            .needs_of(id)
            .into_iter()
            .filter(|need| {
                self.nodes
                    .get(need)
                    .is_none_or(|held| held.row.status != RoadmapStatus::Done)
            })
            .map(|need| need.to_string())
            .collect();
        if !waiting.is_empty() {
            return Err(refusal(
                "claim a plan node",
                format!("{id} waits for {}", waiting.join(", ")),
                self.somewhere_else(),
            ));
        }
        Ok(Held(id.clone()))
    }

    /// The third part of a refusal: a node the caller may actually take.
    fn somewhere_else(&self) -> String {
        self.ready().first().map_or_else(
            || "nothing is ready; report to the person rather than picking a node".to_owned(),
            |id| format!("claim {id}, which is ready"),
        )
    }

    /// The plan's own reading of itself: leaves counted, and the share
    /// of the whole those leaves carry.
    ///
    /// Both figures travel because they answer different questions and
    /// mislead alone. A share says how much of the plan is behind you; a
    /// leaf count says how many pieces the plan turned out to have, and
    /// it is the figure that does not move when somebody divides their
    /// own branch generously.
    #[must_use]
    pub fn progress(&self) -> Progress {
        let mut done: u32 = 0;
        let mut blocked: u32 = 0;
        let mut total: u32 = 0;
        let mut done_parts = Vec::new();
        let mut blocked_parts = Vec::new();
        for node in self.nodes.values() {
            if !node.is_leaf() {
                continue;
            }
            total = total.saturating_add(1);
            match (&node.row.status, &node.row.evidence) {
                (RoadmapStatus::Done, EvidenceCell::Present(_)) => {
                    done = done.saturating_add(1);
                    done_parts.push(node.share);
                }
                (RoadmapStatus::Blocked | RoadmapStatus::AwaitingApproval, _) => {
                    blocked = blocked.saturating_add(1);
                    blocked_parts.push(node.share);
                }
                _ => {}
            }
        }
        Progress::Planned(PlannedProgress {
            done,
            blocked,
            total,
            done_ppb: share::gather(&done_parts).ppb(),
            blocked_ppb: share::gather(&blocked_parts).ppb(),
        })
    }
}

/// The whole plan in billionths, re-exported where progress is read so a
/// renderer does not have to know which module the constant lives in.
pub const PLAN_WHOLE_PPB: u64 = WHOLE_PPB;

fn refusal(action: &'static str, subject: String, recovery: impl Into<String>) -> AxError {
    AxError::failure(AxCode::InvalidArgs, action, subject).with_recovery(recovery)
}

/// The first dependency circle, as the nodes on it.
///
/// Kahn's algorithm: peel off everything that waits for nothing, and
/// whatever will not peel is on a circle. Reported as a walk rather than
/// as a set, because a person repairing it needs to see which edge to
/// cut.
fn first_cycle(nodes: &BTreeMap<NodeId, PlanNode>) -> Option<Vec<NodeId>> {
    let mut waiting: BTreeMap<&NodeId, usize> = nodes
        .iter()
        .map(|(id, node)| (id, node.row.needs.len()))
        .collect();
    let mut queue: Vec<&NodeId> = waiting
        .iter()
        .filter(|(_, count)| **count == 0)
        .map(|(id, _)| *id)
        .collect();
    while let Some(free) = queue.pop() {
        waiting.remove(free);
        for (id, node) in nodes {
            if !node.row.needs.contains(free) {
                continue;
            }
            if let Some(count) = waiting.get_mut(id) {
                *count = count.saturating_sub(1);
                if *count == 0 {
                    queue.push(id);
                }
            }
        }
    }
    let stuck: BTreeSet<&NodeId> = waiting.keys().copied().collect();
    let start = *stuck.iter().next()?;
    let mut walk = vec![start.clone()];
    let mut here = start;
    // Bounded by the number of stuck nodes: a walk that long has
    // already repeated a node, and the repeat is the circle.
    for _ in 0..stuck.len() {
        let next = nodes
            .get(here)?
            .row
            .needs
            .iter()
            .find(|need| stuck.contains(need))?;
        walk.push(next.clone());
        if next == start {
            break;
        }
        here = next;
    }
    Some(walk)
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
    use crate::spine::RoadmapShape;
    use crate::spine::check_roadmap_shape;

    fn tree(text: &str) -> PlanTree {
        let RoadmapShape::WellFormed { rows } = check_roadmap_shape(text) else {
            panic!("the fixture parses");
        };
        PlanTree::build(rows).expect("the fixture is a tree")
    }

    fn refused(text: &str) -> AxError {
        let RoadmapShape::WellFormed { rows } = check_roadmap_shape(text) else {
            panic!("the fixture parses");
        };
        PlanTree::build(rows).expect_err("the fixture is not a tree")
    }

    const HEAD: &str = "\
| # | Item | Weight | Needs | Status | Evidence |
|---|------|--------|-------|--------|----------|
";

    fn locator() -> Locator {
        Locator::parse(&format!("cas:b3-{}", "ab".repeat(32))).unwrap()
    }

    #[test]
    fn an_index_is_read_as_a_path_and_a_bad_one_is_named() {
        let id = NodeId::parse("2.3.1").unwrap();
        assert_eq!(id.depth(), 3);
        assert_eq!(id.parent().unwrap().as_str(), "2.3");
        assert_eq!(id.ordinal(), 1);
        assert!(NodeId::parse("2").unwrap().is_ancestor_of(&id));
        assert!(!NodeId::parse("2.3.1").unwrap().is_ancestor_of(&id));
        // `21` starts with `2` and is not below it: the check is on the
        // separator, not on the prefix.
        assert!(
            !NodeId::parse("2")
                .unwrap()
                .is_ancestor_of(&NodeId::parse("21").unwrap())
        );
        for bad in ["", "0", "1.0", "01", "1..2", "1.a", "-1"] {
            assert!(NodeId::parse(bad).is_err(), "`{bad}` is not an index");
        }
    }

    #[test]
    fn the_reading_order_of_the_table_is_the_order_of_the_ids() {
        let mut ids: Vec<NodeId> = ["2", "1.10", "1.2", "1", "10"]
            .into_iter()
            .map(|raw| NodeId::parse(raw).unwrap())
            .collect();
        ids.sort();
        let drawn: Vec<&str> = ids.iter().map(NodeId::as_str).collect();
        assert_eq!(drawn, ["1", "1.10", "1.2", "10", "2"]);
    }

    #[test]
    fn a_branch_hands_its_whole_share_to_its_children() {
        let plan = tree(&format!(
            "{HEAD}\
| 1 | build | 1 |  | Not started | |
| 1.1 | design | 1 |  | Not started | |
| 1.2 | code | 3 |  | Not started | |
| 2 | ship | 1 |  | Not started | |
"
        ));
        let share_of = |raw: &str| plan.get(&NodeId::parse(raw).unwrap()).unwrap().share;
        assert_eq!(share_of("1").ppb(), 500_000_000);
        assert_eq!(share_of("2").ppb(), 500_000_000);
        assert_eq!(share_of("1.1").ppb(), 125_000_000);
        assert_eq!(share_of("1.2").ppb(), 375_000_000);
        assert_eq!(
            share::gather(&[share_of("1.1"), share_of("1.2")]),
            share_of("1"),
            "the children add up to the branch"
        );
    }

    /// The card's own claim, in numbers: splitting a branch cannot take
    /// anything from its neighbours.
    #[test]
    fn dividing_a_branch_generously_takes_nothing_from_the_others() {
        let before = tree(&format!(
            "{HEAD}\
| 1 | build | 1 |  | Not started | |
| 2 | ship | 1 |  | Not started | |
"
        ));
        let after = tree(&format!(
            "{HEAD}\
| 1 | build | 1 |  | Not started | |
| 1.1 | a | 900 |  | Not started | |
| 1.2 | b | 100 |  | Not started | |
| 2 | ship | 1 |  | Not started | |
"
        ));
        let two = NodeId::parse("2").unwrap();
        assert_eq!(
            before.get(&two).unwrap().share,
            after.get(&two).unwrap().share
        );
    }

    #[test]
    fn progress_counts_leaves_and_reports_both_figures() {
        let plan = tree(&format!(
            "{HEAD}\
| 1 | build | 1 |  | In progress | |
| 1.1 | design | 1 |  | Done | cas:b3-{h} |
| 1.2 | code | 1 |  | Blocked | |
| 2 | ship | 1 |  | Not started | |
",
            h = "ab".repeat(32)
        ));
        let Progress::Planned(planned) = plan.progress() else {
            panic!("a tree reports planned progress");
        };
        assert_eq!(
            (planned.done, planned.blocked, planned.total),
            (1, 1, 3),
            "the branch itself is not a leaf and is not counted"
        );
        assert_eq!(planned.done_ppb, 250_000_000);
        assert_eq!(planned.blocked_ppb, 250_000_000);
    }

    #[test]
    fn a_done_row_without_evidence_reaches_neither_figure() {
        let plan = tree(&format!(
            "{HEAD}\
| 1 | claimed without evidence | 1 |  | Done | |
| 2 | ship | 1 |  | Not started | |
"
        ));
        let Progress::Planned(planned) = plan.progress() else {
            panic!("planned")
        };
        assert_eq!(planned.done, 0);
        assert_eq!(planned.done_ppb, 0);
    }

    #[test]
    fn the_ready_set_is_what_nobody_holds_and_nothing_blocks() {
        let plan = tree(&format!(
            "{HEAD}\
| 1 | design | 1 |  | Done | cas:b3-{h} |
| 2 | code | 1 | 1 | Not started | |
| 3 | ship | 1 | 2 | Not started | |
| 4 | notes | 1 |  | In progress | |
",
            h = "ab".repeat(32)
        ));
        let ready: Vec<String> = plan.ready().iter().map(NodeId::to_string).collect();
        assert_eq!(ready, ["2"], "3 waits for 2, and 4 is taken");
    }

    #[test]
    fn a_child_waits_for_what_its_branch_waits_for() {
        let plan = tree(&format!(
            "{HEAD}\
| 1 | groundwork | 1 |  | Not started | |
| 2 | build | 1 | 1 | Not started | |
| 2.1 | frame | 1 |  | Not started | |
"
        ));
        let ready: Vec<String> = plan.ready().iter().map(NodeId::to_string).collect();
        assert_eq!(
            ready,
            ["1"],
            "2.1 inherits 2's dependency on 1, which is not done"
        );
        let refusal = plan.claim(&NodeId::parse("2.1").unwrap()).unwrap_err();
        assert!(refusal.subject().contains("waits for 1"));
    }

    #[test]
    fn a_circle_is_refused_where_it_is_written_and_the_walk_is_named() {
        let refusal = refused(&format!(
            "{HEAD}\
| 1 | a | 1 | 3 | Not started | |
| 2 | b | 1 | 1 | Not started | |
| 3 | c | 1 | 2 | Not started | |
"
        ));
        assert_eq!(refusal.code(), &AxCode::InvalidArgs);
        assert!(refusal.subject().contains('→'), "{}", refusal.subject());
        assert!(refusal.recovery().contains("circle"));
    }

    #[test]
    fn a_node_that_needs_itself_is_refused_by_name() {
        let refusal = refused(&format!(
            "{HEAD}\
| 1 | a | 1 | 1 | Not started | |
"
        ));
        assert!(refusal.subject().contains("needs itself"));
    }

    #[test]
    fn a_dependency_on_a_row_nobody_wrote_is_refused() {
        let refusal = refused(&format!(
            "{HEAD}\
| 1 | a | 1 | 9 | Not started | |
"
        ));
        assert!(refusal.subject().contains("does not carry"));
    }

    #[test]
    fn a_branch_with_no_parent_row_is_refused() {
        let refusal = refused(&format!(
            "{HEAD}\
| 2.1 | orphan | 1 |  | Not started | |
"
        ));
        assert!(refusal.subject().contains("hangs under 2"));
    }

    #[test]
    fn a_branch_cannot_say_done_over_a_child_that_is_not() {
        let refusal = refused(&format!(
            "{HEAD}\
| 1 | build | 1 |  | Done | cas:b3-{h} |
| 1.1 | frame | 1 |  | Not started | |
",
            h = "ab".repeat(32)
        ));
        assert!(refusal.subject().contains("says done while 1.1"));
    }

    #[test]
    fn a_repeated_index_is_two_plans_and_is_refused() {
        let refusal = refused(&format!(
            "{HEAD}\
| 1 | a | 1 |  | Not started | |
| 1 | b | 1 |  | Not started | |
"
        ));
        assert!(refusal.subject().contains("twice"));
    }

    /// Claiming is the only way to get a `Held`, and a branch, a taken
    /// node and a waiting node are each refused with a different reason.
    #[test]
    fn claiming_names_which_of_the_three_reasons_applies() {
        let plan = tree(&format!(
            "{HEAD}\
| 1 | build | 1 |  | In progress | |
| 1.1 | frame | 1 |  | Not started | |
| 2 | ship | 1 | 1.1 | Not started | |
"
        ));
        assert!(
            plan.claim(&NodeId::parse("1").unwrap())
                .unwrap_err()
                .subject()
                .contains("branch")
        );
        assert!(
            plan.claim(&NodeId::parse("2").unwrap())
                .unwrap_err()
                .subject()
                .contains("waits for 1.1")
        );
        assert_eq!(
            plan.claim(&NodeId::parse("1").unwrap()).unwrap_err().code(),
            &AxCode::InvalidArgs,
            "a branch is a mistake about the plan's shape"
        );
        assert!(
            plan.claim(&NodeId::parse("9").unwrap())
                .unwrap_err()
                .subject()
                .contains("no row")
        );
        let held = plan.claim(&NodeId::parse("1.1").unwrap()).unwrap();
        assert_eq!(held.id().as_str(), "1.1");
    }

    #[test]
    fn a_refusal_points_at_a_node_the_caller_may_take() {
        let plan = tree(&format!(
            "{HEAD}\
| 1 | taken | 1 |  | In progress | |
| 2 | free | 1 |  | Not started | |
"
        ));
        let refusal = plan.claim(&NodeId::parse("1").unwrap()).unwrap_err();
        assert_eq!(
            refusal.code(),
            &AxCode::GoalConflict,
            "two runs wanting one node is a goal conflict"
        );
        assert!(
            refusal.recovery().contains("claim 2"),
            "{}",
            refusal.recovery()
        );
    }

    /// The plan gate: a held node leaves green with evidence or stopped
    /// with a cause, and handing back is the one stop that is not red.
    #[test]
    fn a_held_node_leaves_by_one_of_two_doors() {
        let plan = tree(&format!(
            "{HEAD}\
| 1 | frame | 1 |  | Not started | |
| 2 | roof | 1 |  | Not started | |
| 3 | door | 1 |  | Not started | |
"
        ));
        let green = plan
            .claim(&NodeId::parse("1").unwrap())
            .unwrap()
            .finish(locator());
        assert_eq!(green.status(), RoadmapStatus::Done);
        assert!(green.evidence().is_some());

        let red = plan
            .claim(&NodeId::parse("2").unwrap())
            .unwrap()
            .stop(StopCause::Blocked {
                note: "the kiln is cold".to_owned(),
            });
        assert_eq!(red.status(), RoadmapStatus::Blocked);
        assert!(red.evidence().is_none(), "a stop clears the evidence cell");

        let back = plan
            .claim(&NodeId::parse("3").unwrap())
            .unwrap()
            .stop(StopCause::HandedBack {
                note: "not mine".to_owned(),
            });
        assert_eq!(
            back.status(),
            RoadmapStatus::NotStarted,
            "handing back returns the node to the ready set"
        );
        let PlanExit::Stopped { why, .. } = &back else {
            panic!("stopped")
        };
        assert!(!why.is_red(), "and it is not red");
    }

    #[test]
    fn every_stop_cause_says_something_a_person_can_read() {
        for cause in [
            StopCause::Blocked {
                note: "no key".to_owned(),
            },
            StopCause::HandedBack {
                note: "not mine".to_owned(),
            },
            StopCause::FrozeWithoutEvidence,
            StopCause::Stalled { repeats: 3 },
            StopCause::GateOverdue { waited_ms: 60_000 },
        ] {
            assert!(!cause.line().is_empty(), "{cause:?}");
        }
    }

    #[test]
    fn an_empty_plan_is_a_tree_with_nothing_ready() {
        let plan = PlanTree::build(Vec::new()).unwrap();
        assert!(plan.is_empty());
        assert!(plan.ready().is_empty());
        let Progress::Planned(planned) = plan.progress() else {
            panic!("planned")
        };
        assert_eq!(planned.total, 0);
        assert_eq!(planned.done_ppb, 0);
    }
}
