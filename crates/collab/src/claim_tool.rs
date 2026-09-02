// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The plan tool: the face `Roadmap.md` shows a model.
//!
//! The plan file is the only register. A second table of who-holds-what
//! would be a second answer to the same question, and the one that
//! drifts is always the one nobody reads — while this file is read by
//! the person, counted by `kernel::PlanTree`, and edited here.
//!
//! **Six actions on one catalog line.** `list` and `claim` are how a run
//! finds work without being told what to do; `finish`, `block` and
//! `release` are the ways a held node is put down; `split` is how a run
//! that has found more work says so. They are actions of the entry that
//! already existed rather than a second tool, because the number of
//! lines a model reads every turn is a cost and the number of verbs
//! behind one line is not.
//!
//! Which transitions are legal is decided by `kernel::PlanTree` from the
//! plan's own state, not from what the caller says it is doing — and the
//! refusal names both the state the node is actually in and a node that
//! is ready, because "no" teaches a model to rephrase and try again
//! while "2.3 is being worked on, 2.4 is ready" does not.

use std::cell::RefCell;
use std::rc::Rc;

use kernel::{
    Address, AxCode, AxError, CostTier, Effect, EvidenceCell, Held, Locator, NewChild, NodeId,
    Payload, PlanExit, PlanTree, RenderIntent, RoadmapShape, RoadmapStatus, StopCause, Temporal,
    Tool, ToolCall, ToolMeta, ToolName, ToolOutcome, check_roadmap_shape, insert_children,
    set_roadmap_status,
};
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

/// The run's side of the plan: the file as it stood when the run was
/// dispatched, plus what this run has changed that the ledger does not
/// know yet.
#[derive(Debug)]
pub struct ClaimDesk {
    who: String,
    room: Address,
    text: String,
    changed: bool,
    /// The node this run holds. One at a time: a run holding two nodes
    /// makes both nodes' progress unreadable, because the node is the
    /// unit of "what is being worked on now".
    ///
    /// The value itself is the plan gate: it is minted only by
    /// `PlanTree::claim` and spent only on an exit, so a run cannot put
    /// work down without saying how it went.
    held: Option<Held>,
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

    /// The node this run is holding, if any.
    #[must_use]
    pub fn holding(&self) -> Option<&NodeId> {
        self.held.as_ref().map(Held::id)
    }

    /// Spends a still-held node on the one exit a run that simply ended
    /// has earned.
    ///
    /// **This is where red comes from.** A run that froze while holding
    /// work did not release it and did not finish it; leaving the node
    /// `In progress` for ever would strand the branch behind it, and
    /// calling it done would be a lie. The freeze path calls this, and
    /// it is a no-op for a run that put its node down properly.
    ///
    /// # Errors
    /// Propagates the plan's refusal to record the exit.
    pub fn abandon(&mut self) -> Result<(), AxError> {
        let Some(held) = self.held.take() else {
            return Ok(());
        };
        self.record(held.stop(StopCause::FrozeWithoutEvidence))
    }

    fn tree(&self) -> Result<PlanTree, AxError> {
        match check_roadmap_shape(&self.text) {
            RoadmapShape::WellFormed { rows } => PlanTree::build(rows),
            RoadmapShape::Malformed { problems } => Err(AxError::failure(
                AxCode::InvalidArgs,
                "read the plan",
                problems.join("; "),
            )
            .with_recovery(
                "repair the six-column table in Roadmap.md, then claim a node; a plan that does \
                 not parse has no denominator",
            )),
        }
    }

    /// Writes an exit into the file and queues the record.
    fn record(&mut self, exit: PlanExit) -> Result<(), AxError> {
        let tree = self.tree()?;
        let item = tree
            .get(exit.id())
            .map_or_else(String::new, |node| node.row.item.clone());
        self.text = set_roadmap_status(&self.text, exit.id(), exit.status(), exit.evidence())?;
        self.changed = true;
        self.effects.push(ClaimEffect::PutDown {
            id: exit.id().clone(),
            item,
            exit,
        });
        Ok(())
    }

    fn list(&self) -> Result<Payload, AxError> {
        let tree = self.tree()?;
        let mut ready = Vec::new();
        let mut working = Vec::new();
        let mut red = Vec::new();
        for node in tree.nodes() {
            let mut entry = Map::new();
            entry.insert("node".to_owned(), Value::String(node.row.id.to_string()));
            entry.insert("item".to_owned(), Value::String(node.row.item.clone()));
            match node.row.status {
                RoadmapStatus::InProgress if node.is_leaf() => working.push(Value::Object(entry)),
                RoadmapStatus::Blocked | RoadmapStatus::AwaitingApproval => {
                    red.push(Value::Object(entry));
                }
                _ => {}
            }
        }
        for id in tree.ready() {
            let mut entry = Map::new();
            entry.insert("node".to_owned(), Value::String(id.to_string()));
            if let Some(node) = tree.get(&id) {
                entry.insert("item".to_owned(), Value::String(node.row.item.clone()));
            }
            ready.push(Value::Object(entry));
        }
        let mut result = Map::new();
        // Ready rather than unclaimed: a node whose dependencies are not
        // done is not work anybody can take, and offering it would send
        // a run to a door that is locked.
        result.insert("ready".to_owned(), Value::Array(ready));
        result.insert("in_progress".to_owned(), Value::Array(working));
        result.insert("blocked".to_owned(), Value::Array(red));
        // The denominator is on screen for the person; it is here so the
        // model does not have to count the array to know where it stands.
        result.insert(
            "nodes_total".to_owned(),
            Value::Number(u64::try_from(tree.len()).unwrap_or(u64::MAX).into()),
        );
        Payload::new(result)
    }

    fn claim(&mut self, id: &NodeId) -> Result<Payload, AxError> {
        if let Some(already) = self.holding() {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "claim a plan node",
                format!("this run already holds {already}"),
            )
            .with_recovery(format!(
                "finish, block or release {already} first; a run works on one node"
            )));
        }
        let tree = self.tree()?;
        let held = tree.claim(id)?;
        let item = tree
            .get(id)
            .map_or_else(String::new, |node| node.row.item.clone());
        self.text = set_roadmap_status(&self.text, id, RoadmapStatus::InProgress, None)?;
        self.changed = true;
        self.effects.push(ClaimEffect::Claimed {
            id: id.clone(),
            item: item.clone(),
        });
        self.held = Some(held);
        let mut result = Map::new();
        result.insert("node".to_owned(), Value::String(id.to_string()));
        result.insert("item".to_owned(), Value::String(item));
        result.insert("held_by".to_owned(), Value::String(self.who.clone()));
        Payload::new(result)
    }

    /// The one door out of a held node that needs a locator; the other
    /// two go through [`Self::put_down`].
    fn finish(&mut self, id: &NodeId, raw_evidence: &str) -> Result<Payload, AxError> {
        let evidence = Locator::parse(raw_evidence)?;
        let held = self.take_held(id)?;
        self.record(held.finish(evidence.clone()))?;
        let mut result = Map::new();
        result.insert("node".to_owned(), Value::String(id.to_string()));
        result.insert("evidence".to_owned(), Value::String(evidence.to_string()));
        Payload::new(result)
    }

    fn put_down(&mut self, id: &NodeId, why: StopCause) -> Result<Payload, AxError> {
        let held = self.take_held(id)?;
        let red = why.is_red();
        let line = why.line();
        self.record(held.stop(why))?;
        let mut result = Map::new();
        result.insert("node".to_owned(), Value::String(id.to_string()));
        result.insert("red".to_owned(), Value::Bool(red));
        result.insert("why".to_owned(), Value::String(line));
        Payload::new(result)
    }

    /// Divides a node into children and leaves the run holding nothing:
    /// the work it took has become several pieces, and one of them is
    /// what it should take next.
    fn split(&mut self, id: &NodeId, children: &[NewChild]) -> Result<Payload, AxError> {
        let tree = self.tree()?;
        if tree.get(id).is_none() {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "split a plan node",
                format!("no node numbered {id}"),
            )
            .with_recovery("list the plan first and use an index it carries"));
        }
        let grown = insert_children(&self.text, id, children)?;
        // Built before it is written: a split that would not parse, or
        // that would push the plan past its depth, is refused with the
        // file untouched rather than repaired afterwards.
        let RoadmapShape::WellFormed { rows } = check_roadmap_shape(&grown) else {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "split a plan node",
                "the split would leave a table that does not parse",
            )
            .with_recovery("shorten the child items; they must fit one table row each"));
        };
        PlanTree::build(rows)?;
        self.text = grown;
        self.changed = true;
        if self.holding() == Some(id) {
            self.held = None;
        }
        let names: Vec<String> = children
            .iter()
            .map(|child| child.item.trim().to_owned())
            .collect();
        self.effects.push(ClaimEffect::Split {
            parent: id.clone(),
            children: names.clone(),
        });
        let mut result = Map::new();
        result.insert("node".to_owned(), Value::String(id.to_string()));
        result.insert(
            "children".to_owned(),
            Value::Array(names.into_iter().map(Value::String).collect()),
        );
        Payload::new(result)
    }

    /// Takes the held node, refusing when it is not the one named.
    ///
    /// The run may only put down what it took: a run that could close
    /// somebody else's node could hand out evidence for work it never
    /// saw.
    fn take_held(&mut self, id: &NodeId) -> Result<Held, AxError> {
        match self.held.take() {
            Some(held) if held.id() == id => Ok(held),
            Some(held) => {
                let holding = held.id().clone();
                self.held = Some(held);
                Err(AxError::failure(
                    AxCode::InvalidArgs,
                    "put down a plan node",
                    format!("this run holds {holding}, not {id}"),
                )
                .with_recovery(format!("put down {holding}, or claim {id} first")))
            }
            None => Err(AxError::failure(
                AxCode::InvalidArgs,
                "put down a plan node",
                format!("this run holds nothing, so it cannot put down {id}"),
            )
            .with_recovery("claim it first; a node is closed by the run that took it")),
        }
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
                "`list` | `claim` | `finish` (evidence) | `block` (reason) | `release` (reason) \
                 | `split` (parts)",
            ),
            ("node", "string", "dotted index, such as `2.3`"),
            ("evidence", "string", "a retrievable locator"),
            ("reason", "string", "one line: why"),
            ("parts", "array", "the children, each `{item, weight}`"),
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
                disclosure: "Read this building's plan and take a node before starting work \
                             nobody assigned you."
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
            .with_recovery(ACTIONS)
        })?;
        let result = match action {
            "list" => desk.list()?,
            "claim" => desk.claim(&node_of(args)?)?,
            "finish" => {
                let evidence = args
                    .get("evidence")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        AxError::failure(
                            AxCode::EvidenceMissing,
                            "finish a plan node",
                            "missing string argument `evidence`",
                        )
                        .with_recovery(
                            "pass a retrievable locator: `cas:<hash>` or `file:<path>@<oid>`",
                        )
                    })?;
                desk.finish(&node_of(args)?, evidence)?
            }
            "block" => {
                let note = reason_of(args, "block")?;
                desk.put_down(&node_of(args)?, StopCause::Blocked { note })?
            }
            "release" => {
                let note = reason_of(args, "release")?;
                desk.put_down(&node_of(args)?, StopCause::HandedBack { note })?
            }
            "split" => desk.split(&node_of(args)?, &parts_of(args)?)?,
            other => {
                return Err(AxError::failure(
                    AxCode::InvalidArgs,
                    "read a plan action",
                    other.to_owned(),
                )
                .with_recovery(ACTIONS));
            }
        };
        Ok(ToolOutcome { result })
    }
}

/// The one place the six actions are spelled for a caller that got it
/// wrong. A second list would drift from the schema above.
const ACTIONS: &str = "use `list`, `claim`, `finish`, `block`, `release` or `split`";

fn node_of(args: &Map<String, Value>) -> Result<NodeId, AxError> {
    let raw = args.get("node").and_then(Value::as_str).ok_or_else(|| {
        AxError::failure(
            AxCode::InvalidArgs,
            "read a plan node index",
            "missing string argument `node`",
        )
        .with_recovery("pass `node` as the dotted index in the plan's first column")
    })?;
    NodeId::parse(raw)
}

fn reason_of(args: &Map<String, Value>, action: &str) -> Result<String, AxError> {
    let raw = args
        .get("reason")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|text| !text.is_empty())
        .ok_or_else(|| {
            AxError::failure(
                AxCode::InvalidArgs,
                "read why a plan node stopped",
                format!("`{action}` without a reason"),
            )
            .with_recovery(
                "say in one line why, so the next run does not repeat what you already tried",
            )
        })?;
    Ok(raw.to_owned())
}

/// The children of a split, as the model wrote them.
///
/// A bare string is accepted as well as an object: a weight nobody
/// stated is 1, and refusing the shorter form would make the common case
/// — divide this evenly — the one that needs the most typing.
fn parts_of(args: &Map<String, Value>) -> Result<Vec<NewChild>, AxError> {
    let refuse = |why: &str| {
        AxError::failure(
            AxCode::InvalidArgs,
            "read the parts of a split",
            why.to_owned(),
        )
        .with_recovery("pass `parts` as a list of `{item, weight}`, or of plain strings")
    };
    let listed = args
        .get("parts")
        .and_then(Value::as_array)
        .ok_or_else(|| refuse("missing array argument `parts`"))?;
    let mut children = Vec::with_capacity(listed.len());
    for entry in listed {
        let child = match entry {
            Value::String(item) => NewChild {
                item: item.clone(),
                weight: 1,
            },
            Value::Object(map) => {
                let item = map
                    .get("item")
                    .and_then(Value::as_str)
                    .ok_or_else(|| refuse("a part with no `item`"))?;
                let weight = map.get("weight").and_then(Value::as_u64).unwrap_or(1);
                NewChild {
                    item: item.to_owned(),
                    weight: u32::try_from(weight).unwrap_or(u32::MAX),
                }
            }
            _ => return Err(refuse("a part that is neither text nor an object")),
        };
        children.push(child);
    }
    Ok(children)
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

| # | Item | Weight | Needs | Status | Evidence |
|---|------|--------|-------|--------|----------|
| 1 | wire the kiln | 1 |  | Not started |  |
| 2 | glaze tests | 1 |  | In progress |  |
| 3 | fire the batch | 1 | 2 | Not started |  |
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

    fn node(raw: &str) -> NodeId {
        NodeId::parse(raw).unwrap()
    }

    #[test]
    fn claiming_a_ready_node_marks_it_and_queues_one_effect() {
        let shared = desk();
        let mut tool = ClaimTool::new(Rc::clone(&shared)).unwrap();
        let outcome = tool
            .invoke(&call(serde_json::json!({ "action": "claim", "node": "1" })))
            .unwrap();
        assert_eq!(
            outcome.result.as_map().get("item").and_then(Value::as_str),
            Some("wire the kiln")
        );
        let mut borrowed = shared.borrow_mut();
        let text = borrowed.roadmap().expect("the plan changed").to_owned();
        assert!(text.contains("| 1 | wire the kiln | 1 |  | In progress |  |"));
        let effects = borrowed.take_effects();
        assert_eq!(effects.len(), 1);
        assert_eq!(effects[0].id(), &node("1"));
        assert_eq!(effects[0].kind(), kernel::EventKind::RoadmapClaimed);
        assert!(
            borrowed.take_effects().is_empty(),
            "an effect read twice would be a claim recorded twice"
        );
    }

    #[test]
    fn a_node_somebody_else_is_working_on_is_refused_with_a_ready_one() {
        let shared = desk();
        let mut tool = ClaimTool::new(Rc::clone(&shared)).unwrap();
        let refusal = tool
            .invoke(&call(serde_json::json!({ "action": "claim", "node": "2" })))
            .unwrap_err();
        assert_eq!(refusal.code(), &AxCode::GoalConflict);
        assert!(refusal.subject().contains("In progress"));
        assert!(
            refusal.recovery().contains("claim 1"),
            "the third part names a node the caller may actually take: {}",
            refusal.recovery()
        );
        assert!(
            shared.borrow().roadmap().is_none(),
            "a refusal writes nothing"
        );
    }

    /// The ready set is what `list` offers: a node whose dependency is
    /// not done is not work anybody can take.
    #[test]
    fn listing_offers_what_is_ready_rather_than_what_is_merely_unclaimed() {
        let shared = desk();
        let mut tool = ClaimTool::new(Rc::clone(&shared)).unwrap();
        let outcome = tool
            .invoke(&call(serde_json::json!({ "action": "list" })))
            .unwrap();
        let map = outcome.result.as_map();
        let ready = map.get("ready").and_then(Value::as_array).unwrap();
        assert_eq!(ready.len(), 1, "3 waits for 2");
        assert_eq!(ready[0].get("node").and_then(Value::as_str), Some("1"));
        assert_eq!(
            map.get("in_progress")
                .and_then(Value::as_array)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(map.get("nodes_total").and_then(Value::as_u64), Some(3));
        assert!(
            shared.borrow().roadmap().is_none(),
            "reading the plan does not modify it"
        );
    }

    #[test]
    fn a_run_holds_one_node_at_a_time() {
        let shared = desk();
        let mut tool = ClaimTool::new(Rc::clone(&shared)).unwrap();
        tool.invoke(&call(serde_json::json!({ "action": "claim", "node": "1" })))
            .unwrap();
        let refusal = tool
            .invoke(&call(serde_json::json!({ "action": "claim", "node": "3" })))
            .unwrap_err();
        assert!(refusal.subject().contains("holds 1"));
        assert!(refusal.recovery().contains("one node"));
    }

    #[test]
    fn finishing_requires_evidence_that_parses() {
        let shared = desk();
        let mut tool = ClaimTool::new(Rc::clone(&shared)).unwrap();
        tool.invoke(&call(serde_json::json!({ "action": "claim", "node": "1" })))
            .unwrap();
        let missing = tool
            .invoke(&call(
                serde_json::json!({ "action": "finish", "node": "1" }),
            ))
            .unwrap_err();
        assert_eq!(missing.code(), &AxCode::EvidenceMissing);
        let unparsable = tool
            .invoke(&call(serde_json::json!({
                "action": "finish", "node": "1", "evidence": "trust me"
            })))
            .unwrap_err();
        assert_eq!(unparsable.code(), &AxCode::LocatorInvalid);
        let done = tool
            .invoke(&call(serde_json::json!({
                "action": "finish", "node": "1", "evidence": locator()
            })))
            .unwrap();
        assert!(done.result.as_map().contains_key("evidence"));
        let text = shared.borrow().roadmap().unwrap().to_owned();
        assert_eq!(
            evidence_of(&text, &node("1")).map(|l| l.to_string()),
            Some(locator()),
            "what was written is what a reader retrieves"
        );
    }

    /// The plan gate, from the tool's side: a run may only put down the
    /// node it took, and it may not close one it never claimed.
    #[test]
    fn a_run_can_only_put_down_what_it_took() {
        let shared = desk();
        let mut tool = ClaimTool::new(Rc::clone(&shared)).unwrap();
        let never = tool
            .invoke(&call(serde_json::json!({
                "action": "finish", "node": "2", "evidence": locator()
            })))
            .unwrap_err();
        assert!(never.subject().contains("holds nothing"));
        tool.invoke(&call(serde_json::json!({ "action": "claim", "node": "1" })))
            .unwrap();
        let other = tool
            .invoke(&call(serde_json::json!({
                "action": "finish", "node": "2", "evidence": locator()
            })))
            .unwrap_err();
        assert!(other.subject().contains("holds 1, not 2"));
    }

    #[test]
    fn blocking_paints_the_node_red_and_says_why() {
        let shared = desk();
        let mut tool = ClaimTool::new(Rc::clone(&shared)).unwrap();
        tool.invoke(&call(serde_json::json!({ "action": "claim", "node": "1" })))
            .unwrap();
        let outcome = tool
            .invoke(&call(serde_json::json!({
                "action": "block", "node": "1", "reason": "the kiln has no power"
            })))
            .unwrap();
        assert_eq!(
            outcome.result.as_map().get("red").and_then(Value::as_bool),
            Some(true)
        );
        let text = shared.borrow().roadmap().unwrap().to_owned();
        assert!(text.contains("| 1 | wire the kiln | 1 |  | Blocked |  |"));
        let effects = shared.borrow_mut().take_effects();
        assert_eq!(effects[1].kind(), kernel::EventKind::RoadmapBlocked);
        let payload = effects[1].payload("potter@lab.1").unwrap();
        assert_eq!(
            payload.as_map().get("verb").and_then(Value::as_str),
            Some("blocked")
        );
        assert!(
            payload
                .as_map()
                .get("line")
                .and_then(Value::as_str)
                .is_some_and(|line| line.contains("no power")),
            "the reason travels with the record, not only with the row"
        );
    }

    #[test]
    fn releasing_puts_the_node_back_where_another_run_can_take_it() {
        let shared = desk();
        let mut tool = ClaimTool::new(Rc::clone(&shared)).unwrap();
        tool.invoke(&call(serde_json::json!({ "action": "claim", "node": "1" })))
            .unwrap();
        let outcome = tool
            .invoke(&call(serde_json::json!({
                "action": "release", "node": "1", "reason": "not my trade"
            })))
            .unwrap();
        assert_eq!(
            outcome.result.as_map().get("red").and_then(Value::as_bool),
            Some(false),
            "handing back is not red"
        );
        let text = shared.borrow().roadmap().unwrap().to_owned();
        assert!(text.contains("| 1 | wire the kiln | 1 |  | Not started |  |"));
        let effects = shared.borrow_mut().take_effects();
        assert_eq!(effects[1].kind(), kernel::EventKind::RoadmapReleased);
    }

    #[test]
    fn putting_a_node_down_without_a_reason_is_refused() {
        let shared = desk();
        let mut tool = ClaimTool::new(Rc::clone(&shared)).unwrap();
        tool.invoke(&call(serde_json::json!({ "action": "claim", "node": "1" })))
            .unwrap();
        let refusal = tool
            .invoke(&call(serde_json::json!({ "action": "block", "node": "1" })))
            .unwrap_err();
        assert!(refusal.recovery().contains("one line"));
    }

    #[test]
    fn splitting_grows_the_plan_and_the_run_stops_holding_the_branch() {
        let shared = desk();
        let mut tool = ClaimTool::new(Rc::clone(&shared)).unwrap();
        tool.invoke(&call(serde_json::json!({ "action": "claim", "node": "1" })))
            .unwrap();
        let outcome = tool
            .invoke(&call(serde_json::json!({
                "action": "split",
                "node": "1",
                "parts": [{"item": "run the cable", "weight": 3}, "test the element"]
            })))
            .unwrap();
        assert_eq!(
            outcome
                .result
                .as_map()
                .get("children")
                .and_then(Value::as_array)
                .map(Vec::len),
            Some(2)
        );
        let text = shared.borrow().roadmap().unwrap().to_owned();
        assert!(text.contains("| 1.1 | run the cable | 3 |  | Not started |  |"));
        assert!(
            text.contains("| 1.2 | test the element | 1 |  | Not started |  |"),
            "a bare string is a child of weight one"
        );
        assert!(
            shared.borrow().holding().is_none(),
            "the work it took is now several pieces; it takes one of them next"
        );
        let effects = shared.borrow_mut().take_effects();
        assert_eq!(effects[1].kind(), kernel::EventKind::RoadmapSplit);
    }

    /// A run that ends holding a node leaves red behind, and the reason
    /// says what happened rather than inventing one.
    #[test]
    fn a_run_that_freezes_still_holding_a_node_leaves_it_red() {
        let shared = desk();
        let mut tool = ClaimTool::new(Rc::clone(&shared)).unwrap();
        tool.invoke(&call(serde_json::json!({ "action": "claim", "node": "1" })))
            .unwrap();
        shared.borrow_mut().abandon().unwrap();
        let text = shared.borrow().roadmap().unwrap().to_owned();
        assert!(text.contains("| 1 | wire the kiln | 1 |  | Blocked |  |"));
        let effects = shared.borrow_mut().take_effects();
        assert_eq!(effects[1].kind(), kernel::EventKind::RoadmapBlocked);
        assert!(
            shared.borrow_mut().abandon().is_ok(),
            "a run that put its node down properly abandons nothing"
        );
    }

    #[test]
    fn an_effect_whose_node_moved_underneath_it_is_no_longer_true() {
        let shared = desk();
        let mut tool = ClaimTool::new(Rc::clone(&shared)).unwrap();
        tool.invoke(&call(serde_json::json!({ "action": "claim", "node": "1" })))
            .unwrap();
        let effects = shared.borrow_mut().take_effects();
        assert!(
            still_true(PLAN, &effects[0]),
            "against the file it was decided on, the effect holds"
        );
        let moved = set_roadmap_status(PLAN, &node("1"), RoadmapStatus::InProgress, None).unwrap();
        assert!(
            !still_true(&moved, &effects[0]),
            "somebody else took the node first; the claim does not take"
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
        assert!(refusal.recovery().contains("six-column"));
    }

    #[test]
    fn an_unknown_action_is_answered_with_the_six_that_exist() {
        let mut tool = ClaimTool::new(desk()).unwrap();
        let refusal = tool
            .invoke(&call(serde_json::json!({ "action": "delete" })))
            .unwrap_err();
        assert_eq!(refusal.recovery(), ACTIONS);
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

    /// V3.19's closing condition: six actions cost no more catalog bytes
    /// than four did. The catalog is what every turn pays for, so a
    /// verb that grows it is a verb charged to every run in the city
    /// whether or not it is ever called.
    #[test]
    fn six_actions_cost_no_more_catalog_bytes_than_four_did() {
        let tool = ClaimTool::new(desk()).unwrap();
        let meta = tool.meta();
        let bytes = meta.disclosure.len()
            + serde_json::to_string(meta.params.as_map())
                .expect("the schema serialises")
                .len();
        // The four-action tool measured 548 B on this same reading. Two
        // more verbs and two more arguments fit under it because the
        // locator grammar left the schema for the refusal that needs it:
        // a description repeating what a refusal already says is paid
        // for every turn and read once.
        assert!(
            bytes <= 548,
            "the plan entry costs {bytes} B, and four actions cost 548"
        );
    }
}
