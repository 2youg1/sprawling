// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The pull request tool: the face `collab::pr` shows a model.
//!
//! The losing line of the whole design sits behind this one interface —
//! the resident who wrote the work does not decide whether it is good.
//! Three things enforce it, and none of them is a rule someone has to
//! remember: `Pr<Open>` has no `merged`, `Artifact` has no public
//! constructor, and this tool refuses a verification whose caller is the
//! implementer.
//!
//! Verifying and merging are one action rather than two. A `Verified`
//! request nobody merged would be a third state for a person to chase,
//! and the merge is not a second decision: it is what verification
//! means. A refusal is the other outcome of the same call.

use std::cell::RefCell;
use std::rc::Rc;

use kernel::{
    Address, AxCode, AxError, B3Hash, CostTier, Effect, Locator, Payload, RenderIntent, Temporal,
    Tool, ToolCall, ToolMeta, ToolName, ToolOutcome,
};
use serde_json::{Map, Value};

use crate::fanin::Claim;
use crate::pr::Pr;
use crate::workshop::NodeId;

/// A request the city knows about: what it is called, who wrote it, and
/// which branch carries it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenRequest {
    pub node: NodeId,
    pub implementer: String,
    pub branch: String,
    /// The commit the branch stood at when the request was opened. It is
    /// the identity of the work being judged: a verifier who checked one
    /// commit has not vouched for a later one.
    pub commit: String,
}

impl OpenRequest {
    /// The `pr_opened` record, and the shape a rebuild reads back.
    ///
    /// # Errors
    /// Propagates the payload's refusal to hold what it was given.
    pub fn payload(&self) -> Result<Payload, AxError> {
        let mut map = Map::new();
        map.insert(
            "node".to_owned(),
            Value::String(self.node.as_str().to_owned()),
        );
        map.insert(
            "implementer".to_owned(),
            Value::String(self.implementer.clone()),
        );
        map.insert("branch".to_owned(), Value::String(self.branch.clone()));
        map.insert("commit".to_owned(), Value::String(self.commit.clone()));
        Payload::new(map)
    }

    /// Reads back what [`payload`](Self::payload) wrote.
    ///
    /// # Errors
    /// Refuses a payload missing any of the four fields.
    pub fn from_payload(data: &Payload) -> Result<OpenRequest, AxError> {
        let map = data.as_map();
        let text = |key: &str| -> Result<String, AxError> {
            map.get(key)
                .and_then(Value::as_str)
                .map(str::to_owned)
                .ok_or_else(|| {
                    AxError::failure(AxCode::InvalidArgs, "read an open request", key.to_owned())
                        .with_recovery(
                            "this shape is written by the same binary that reads it; report it",
                        )
                })
        };
        Ok(OpenRequest {
            node: NodeId::parse(&text("node")?)?,
            implementer: text("implementer")?,
            branch: text("branch")?,
            commit: text("commit")?,
        })
    }
}

/// What the run did to the city's requests. Exhaustive for the same
/// reason as the other desks: every variant is a line the worker has to
/// write, so a new one must be a compile error where the writing
/// happens.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrEffect {
    /// Commit this run's tree onto its branch and record the request.
    Opened { branch: String },
    /// Merge the branch into the city trunk and record who checked it.
    Merged { request: OpenRequest, by: String },
    /// Record a refusal. The branch stays where it is; the work is not
    /// lost, it is not accepted.
    Rejected {
        request: OpenRequest,
        by: String,
        why: String,
    },
}

/// The run's side of the request register.
#[derive(Debug)]
pub struct PrDesk {
    who: String,
    room: Address,
    /// The tree this run works in, if it has one. A run without a tree
    /// has nothing to offer, and says so rather than offering the city's
    /// own files.
    branch: Option<String>,
    node: Option<NodeId>,
    open: Vec<OpenRequest>,
    effects: Vec<PrEffect>,
}

impl PrDesk {
    #[must_use]
    pub fn new(
        who: String,
        room: Address,
        branch: Option<String>,
        node: Option<NodeId>,
        open: Vec<OpenRequest>,
    ) -> PrDesk {
        PrDesk {
            who,
            room,
            branch,
            node,
            open,
            effects: Vec::new(),
        }
    }

    /// What the worker has to carry out, drained so it cannot run twice.
    pub fn take_effects(&mut self) -> Vec<PrEffect> {
        std::mem::take(&mut self.effects)
    }

    fn open(&mut self) -> Result<Payload, AxError> {
        let (Some(branch), Some(node)) = (self.branch.clone(), self.node.clone()) else {
            return Err(AxError::failure(
                AxCode::ToolUnavailable,
                "open a pull request",
                "this run has no tree of its own",
            )
            .with_recovery(
                "work in a building whose rules ask for review; a run that writes straight into \
                 the city has nothing to offer for merging",
            ));
        };
        if self.open.iter().any(|request| request.branch == branch) {
            return Err(
                AxError::failure(AxCode::InvalidArgs, "open a pull request", branch).with_recovery(
                    "this branch already has a request waiting; ask someone to check it",
                ),
            );
        }
        // Held as a request only after the worker commits the tree: the
        // commit is what the record names, and naming a commit that does
        // not exist yet would be a record about nothing.
        self.effects.push(PrEffect::Opened {
            branch: branch.clone(),
        });
        let mut result = Map::new();
        result.insert("node".to_owned(), Value::String(node.as_str().to_owned()));
        result.insert("branch".to_owned(), Value::String(branch));
        result.insert(
            "waiting_for".to_owned(),
            Value::String("another resident to check it".to_owned()),
        );
        Payload::new(result)
    }

    fn list(&self) -> Result<Payload, AxError> {
        let mut rows = Vec::with_capacity(self.open.len());
        for request in &self.open {
            let mut row = Map::new();
            row.insert(
                "node".to_owned(),
                Value::String(request.node.as_str().to_owned()),
            );
            row.insert("branch".to_owned(), Value::String(request.branch.clone()));
            row.insert(
                "implementer".to_owned(),
                Value::String(request.implementer.clone()),
            );
            // Stated rather than filtered out: a resident who cannot see
            // its own request would think it had vanished.
            row.insert(
                "yours".to_owned(),
                Value::Bool(request.implementer == self.who),
            );
            rows.push(Value::Object(row));
        }
        let mut result = Map::new();
        result.insert("requests".to_owned(), Value::Array(rows));
        Payload::new(result)
    }

    fn check(&mut self, args: &Map<String, Value>) -> Result<Payload, AxError> {
        let branch = text(args, "branch", "check a pull request")?;
        let Some(request) = self
            .open
            .iter()
            .find(|request| request.branch == branch)
            .cloned()
        else {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "check a pull request",
                branch.to_owned(),
            )
            .with_recovery("list the requests waiting, then name one of those branches"));
        };
        if request.implementer == self.who {
            return Err(AxError::failure(
                AxCode::GateDenied,
                "check a pull request",
                branch.to_owned(),
            )
            .with_recovery("you wrote this; ask another resident to run the done check on it"));
        }
        let passed = args.get("passed").and_then(Value::as_bool).ok_or_else(|| {
            AxError::failure(
                AxCode::InvalidArgs,
                "check a pull request",
                "missing boolean argument `passed`",
            )
            .with_recovery("say whether the node's own done check passed, as true or false")
        })?;
        if !passed {
            let why = args
                .get("why")
                .and_then(Value::as_str)
                .unwrap_or("the done check did not pass")
                .to_owned();
            let mut result = Map::new();
            result.insert("branch".to_owned(), Value::String(branch.to_owned()));
            result.insert("merged".to_owned(), Value::Bool(false));
            result.insert("why".to_owned(), Value::String(why.clone()));
            self.effects.push(PrEffect::Rejected {
                request,
                by: self.who.clone(),
                why,
            });
            return Payload::new(result);
        }
        // The typestate is walked here, not trusted: `verified` is the
        // only door from a claim to an artifact, and it refuses a
        // verifier who is the producer. Walking it means the refusal
        // cannot be skipped by a caller who forgot.
        let digest = B3Hash::digest(request.commit.as_bytes());
        let claim = Claim::new(
            request.node.clone(),
            Locator::parse(&format!("file:{}@{}", self.room.as_str(), request.commit))?,
            digest,
            request.implementer.clone(),
        );
        let artifact = claim.verified(true, &self.who)?;
        let pending = Pr::open(
            request.node.clone(),
            request.implementer.clone(),
            request.branch.clone(),
        )?;
        let verified = pending.verified(&artifact)?;
        let mut result = Map::new();
        result.insert("branch".to_owned(), Value::String(branch.to_owned()));
        result.insert("merged".to_owned(), Value::Bool(true));
        result.insert(
            "verified_by".to_owned(),
            Value::String(verified.verified_by().to_owned()),
        );
        self.open.retain(|held| held.branch != request.branch);
        self.effects.push(PrEffect::Merged {
            request,
            by: self.who.clone(),
        });
        Payload::new(result)
    }
}

pub struct PrTool {
    meta: ToolMeta,
    desk: Rc<RefCell<PrDesk>>,
}

impl PrTool {
    /// # Errors
    /// Propagates a malformed tool name or parameter schema.
    pub fn new(room: Address, desk: Rc<RefCell<PrDesk>>) -> Result<PrTool, AxError> {
        let mut properties = Map::new();
        for (field, kind, description) in [
            (
                "action",
                "string",
                "`open` to offer your work, `list` to see what is waiting, `check` to run \
                 someone else's done check",
            ),
            ("branch", "string", "check only: the branch you looked at"),
            (
                "passed",
                "boolean",
                "check only: whether that node's own done check passed",
            ),
            ("why", "string", "check only: why it did not pass"),
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
        Ok(PrTool {
            meta: ToolMeta {
                name: ToolName::parse("pr")?,
                disclosure:
                    "Offer your work for review, or check someone else's; nothing you wrote \
                     reaches the building until another resident has checked it."
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

impl Tool for PrTool {
    fn meta(&self) -> &ToolMeta {
        &self.meta
    }

    fn invoke(&mut self, call: &ToolCall) -> Result<ToolOutcome, AxError> {
        if call.name != self.meta.name {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "work a pull request",
                format!("call routed to the wrong tool: {}", call.name.as_str()),
            ));
        }
        let args = call.args.as_map();
        let mut desk = self.desk.try_borrow_mut().map_err(|_| {
            AxError::failure(
                AxCode::InvalidArgs,
                "reach the request register",
                "the register is already in use",
            )
            .with_recovery("call the tool once at a time")
        })?;
        let result = match text(args, "action", "read a pull request action")? {
            "open" => desk.open()?,
            "list" => desk.list()?,
            "check" => desk.check(args)?,
            other => {
                return Err(AxError::failure(
                    AxCode::InvalidArgs,
                    "read a pull request action",
                    other.to_owned(),
                )
                .with_recovery("use `open`, `list` or `check`"));
            }
        };
        Ok(ToolOutcome { result })
    }
}

fn text<'a>(args: &'a Map<String, Value>, key: &str, action: &str) -> Result<&'a str, AxError> {
    args.get(key).and_then(Value::as_str).ok_or_else(|| {
        AxError::failure(
            AxCode::InvalidArgs,
            action.to_owned(),
            format!("missing string argument `{key}`"),
        )
        .with_recovery(format!("pass `{key}` as a string"))
    })
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

    fn request(branch: &str, implementer: &str) -> OpenRequest {
        OpenRequest {
            node: NodeId::parse(branch).unwrap(),
            implementer: implementer.to_owned(),
            branch: branch.to_owned(),
            commit: "0123456789abcdef0123456789abcdef01234567".to_owned(),
        }
    }

    fn tool(
        who: &str,
        branch: Option<&str>,
        open: Vec<OpenRequest>,
    ) -> (PrTool, Rc<RefCell<PrDesk>>) {
        let desk = Rc::new(RefCell::new(PrDesk::new(
            who.to_owned(),
            Address::parse("lab/room1").unwrap(),
            branch.map(str::to_owned),
            branch.and_then(|b| NodeId::parse(b).ok()),
            open,
        )));
        let tool = PrTool::new(Address::parse("lab/room1").unwrap(), Rc::clone(&desk)).unwrap();
        (tool, desk)
    }

    fn call(args: Value) -> ToolCall {
        ToolCall {
            id: "tu_1".to_owned(),
            name: ToolName::parse("pr").unwrap(),
            args: Payload::new(args.as_object().unwrap().clone()).unwrap(),
        }
    }

    #[test]
    fn the_resident_who_wrote_it_cannot_be_the_one_who_checks_it() {
        let (mut tool, desk) = tool(
            "lab/room1",
            Some("tree-a"),
            vec![request("tree-a", "lab/room1")],
        );
        let refusal = tool
            .invoke(&call(serde_json::json!({
                "action": "check",
                "branch": "tree-a",
                "passed": true,
            })))
            .unwrap_err();
        assert_eq!(refusal.code(), &AxCode::GateDenied);
        assert!(refusal.recovery().contains("another resident"));
        assert!(
            desk.borrow_mut().take_effects().is_empty(),
            "a refused check moves nothing"
        );
    }

    #[test]
    fn a_check_that_passes_merges_and_one_that_fails_records_why() {
        let (mut checker, desk) = tool("lab/tests", None, vec![request("tree-a", "lab/room1")]);
        let outcome = checker
            .invoke(&call(serde_json::json!({
                "action": "check",
                "branch": "tree-a",
                "passed": true,
            })))
            .unwrap();
        assert_eq!(
            outcome
                .result
                .as_map()
                .get("merged")
                .and_then(Value::as_bool),
            Some(true)
        );
        let effects = desk.borrow_mut().take_effects();
        assert!(matches!(effects[0], PrEffect::Merged { .. }));

        let (mut second, desk) = tool("lab/tests", None, vec![request("tree-b", "lab/room1")]);
        second
            .invoke(&call(serde_json::json!({
                "action": "check",
                "branch": "tree-b",
                "passed": false,
                "why": "the tests do not run",
            })))
            .unwrap();
        let effects = desk.borrow_mut().take_effects();
        match &effects[0] {
            PrEffect::Rejected { why, .. } => assert_eq!(why, "the tests do not run"),
            other => panic!("a failed check rejects, not {other:?}"),
        }
    }

    #[test]
    fn a_run_without_a_tree_has_nothing_to_offer_and_says_so() {
        let (mut tool, _desk) = tool("lab/room1", None, Vec::new());
        let refusal = tool
            .invoke(&call(serde_json::json!({ "action": "open" })))
            .unwrap_err();
        assert_eq!(refusal.code(), &AxCode::ToolUnavailable);
        assert!(refusal.recovery().contains("review"));
    }

    #[test]
    fn one_branch_carries_one_request() {
        let (mut tool, _desk) = tool(
            "lab/room1",
            Some("tree-a"),
            vec![request("tree-a", "lab/room1")],
        );
        assert!(
            tool.invoke(&call(serde_json::json!({ "action": "open" })))
                .is_err()
        );
    }

    #[test]
    fn a_request_reads_back_as_the_request_that_was_made() {
        let original = request("tree-a", "lab/room1");
        let read = OpenRequest::from_payload(&original.payload().unwrap()).unwrap();
        assert_eq!(read, original);
    }

    #[test]
    fn listing_says_which_ones_are_yours() {
        let (mut tool, _desk) = tool(
            "lab/room1",
            Some("tree-a"),
            vec![
                request("tree-a", "lab/room1"),
                request("tree-b", "lab/room2"),
            ],
        );
        let outcome = tool
            .invoke(&call(serde_json::json!({ "action": "list" })))
            .unwrap();
        let rows = outcome
            .result
            .as_map()
            .get("requests")
            .and_then(Value::as_array)
            .unwrap()
            .clone();
        assert_eq!(rows.len(), 2);
        let mine: Vec<bool> = rows
            .iter()
            .filter_map(|row| row.get("yours").and_then(Value::as_bool))
            .collect();
        assert_eq!(mine, vec![true, false]);
    }
}
