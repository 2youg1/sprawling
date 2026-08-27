// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! The goal tool: the face `kernel::goal` and `collab::arbiter` show a
//! model.
//!
//! Three layers, none of them duplicated here. Detection answers whether
//! two goals clash; arbitration answers who settles it; this module only
//! joins them into something a resident can call, and refuses to
//! register anything that clashed.
//!
//! A refusal is where the design lives. "No" alone teaches a model to
//! rephrase and try again, so the refusal carries the level that decides
//! the clash — wait for the other goal, go and agree with its owner, or
//! ask the person — and the model can act on exactly one of those.

use std::cell::RefCell;
use std::rc::Rc;

use kernel::{
    Address, AxCode, AxError, CostTier, Effect, GoalEntry, GoalId, GoalResource, Payload,
    RenderIntent, RunId, Temporal, Tool, ToolCall, ToolMeta, ToolName, ToolOutcome,
};
use serde_json::{Map, Value};

use crate::arbiter::{Circumstance, Level, arbitrate};

/// What the run did to the city's goal register. Exhaustive for the
/// same reason as `SignalEffect`: a variant the worker does not record
/// is a claim the city never made.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GoalEffect {
    /// Nothing held this ground; the claim stands once it is recorded.
    Registered(GoalEntry),
    /// Someone was here first. The level says who settles it.
    Conflicted { entry: GoalEntry, level: Level },
}

/// The run's side of the goal register: what the city already holds,
/// plus what this run has claimed that the ledger does not know yet.
#[derive(Debug)]
pub struct GoalDesk {
    run: RunId,
    owner: String,
    registered: Vec<GoalEntry>,
    effects: Vec<GoalEffect>,
    minted: u32,
}

impl GoalDesk {
    #[must_use]
    pub fn new(run: RunId, owner: String, registered: Vec<GoalEntry>) -> GoalDesk {
        GoalDesk {
            run,
            owner,
            registered,
            effects: Vec::new(),
            minted: 0,
        }
    }

    /// What the worker has to record, drained so it cannot be recorded
    /// twice.
    pub fn take_effects(&mut self) -> Vec<GoalEffect> {
        std::mem::take(&mut self.effects)
    }

    fn mint(&mut self) -> Result<GoalId, AxError> {
        self.minted = self.minted.checked_add(1).ok_or_else(|| {
            AxError::failure(
                AxCode::InvalidArgs,
                "mint a goal id",
                "this run has registered as many goals as one run can",
            )
            .with_recovery("freeze the run and dispatch again")
        })?;
        GoalId::new(format!("{}-g{}", self.run, self.minted)).ok_or_else(|| {
            AxError::failure(AxCode::InvalidArgs, "mint a goal id", "empty id")
                .with_recovery("this cannot happen with a run id in hand; report it")
        })
    }

    fn register(&mut self, args: &Map<String, Value>) -> Result<Payload, AxError> {
        let statement = text(args, "statement")?.to_owned();
        let mut resources = Vec::new();
        for raw in strings(args, "paths") {
            resources.push(GoalResource::Path(Address::parse(&raw)?));
        }
        for raw in strings(args, "externals") {
            resources.push(GoalResource::External(raw));
        }
        if resources.is_empty() {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "register a goal",
                "no resources named",
            )
            .with_recovery(
                "name at least one path or external resource; a goal that claims nothing \
                 cannot keep anyone off it",
            ));
        }
        let entry = GoalEntry {
            id: self.mint()?,
            owner: self.owner.clone(),
            resources,
            statement,
            standing: args
                .get("standing")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        };
        // The two facts a machine caller cannot know are left false on
        // purpose: whether a gate refused, and whether the clash is
        // about intent, are the caller's knowledge, and a tool that
        // guessed at them would be guessing at the person's business.
        match arbitrate(&self.registered, &entry, Circumstance::default()) {
            None => {
                let mut result = Map::new();
                result.insert("id".to_owned(), Value::String(entry.id.as_str().to_owned()));
                result.insert("registered".to_owned(), Value::Bool(true));
                self.registered.push(entry.clone());
                self.effects.push(GoalEffect::Registered(entry));
                Payload::new(result)
            }
            Some(level) => {
                let refusal = AxError::failure(
                    AxCode::GoalConflict,
                    "register a goal",
                    entry.statement.clone(),
                )
                .with_recovery(next_move(&level));
                self.effects.push(GoalEffect::Conflicted { entry, level });
                Err(refusal)
            }
        }
    }
}

/// The third part of the refusal: one thing to do, named after the level
/// that decides the clash.
fn next_move(level: &Level) -> String {
    match level {
        Level::Serialize { after } => format!(
            "`{}` holds this ground and is the standing goal; wait for it, then register again",
            after.as_str()
        ),
        Level::Arbitrate { with } => format!(
            "signal the resident who registered `{}` and agree which of you takes it",
            with.as_str()
        ),
        Level::Owner { with, because } => format!(
            "ask the person: this clashes with `{}` ({})",
            with.as_str(),
            because.as_str()
        ),
    }
}

pub struct GoalTool {
    meta: ToolMeta,
    desk: Rc<RefCell<GoalDesk>>,
}

impl GoalTool {
    /// `room` is what the tool declares it writes into; the register
    /// itself is the city's, and the gate that matters is the clash.
    ///
    /// # Errors
    /// Propagates a malformed tool name or parameter schema.
    pub fn new(room: Address, desk: Rc<RefCell<GoalDesk>>) -> Result<GoalTool, AxError> {
        let mut properties = Map::new();
        for (field, kind, description) in [
            (
                "statement",
                "string",
                "what you intend to do, in one sentence",
            ),
            (
                "paths",
                "array",
                "addresses you will be working in; overlapping ones clash",
            ),
            (
                "externals",
                "array",
                "named resources outside the city that only one goal may hold",
            ),
            (
                "standing",
                "boolean",
                "true for ongoing responsibility, false for one piece of work",
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
            Value::Array(vec![Value::String("statement".to_owned())]),
        );
        Ok(GoalTool {
            meta: ToolMeta {
                name: ToolName::parse("goal")?,
                disclosure:
                    "Claim the ground you are about to work on, so two residents do not edit the \
                     same thing; refused if someone holds it already."
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

impl Tool for GoalTool {
    fn meta(&self) -> &ToolMeta {
        &self.meta
    }

    fn invoke(&mut self, call: &ToolCall) -> Result<ToolOutcome, AxError> {
        if call.name != self.meta.name {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "register a goal",
                format!("call routed to the wrong tool: {}", call.name.as_str()),
            ));
        }
        let mut desk = self.desk.try_borrow_mut().map_err(|_| {
            AxError::failure(
                AxCode::InvalidArgs,
                "reach the goal register",
                "the register is already in use",
            )
            .with_recovery("call the tool once at a time")
        })?;
        let result = desk.register(call.args.as_map())?;
        Ok(ToolOutcome { result })
    }
}

fn text<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str, AxError> {
    args.get(key).and_then(Value::as_str).ok_or_else(|| {
        AxError::failure(
            AxCode::InvalidArgs,
            "register a goal",
            format!("missing string argument `{key}`"),
        )
        .with_recovery(format!("pass `{key}` as a string"))
    })
}

fn strings(args: &Map<String, Value>, key: &str) -> Vec<String> {
    args.get(key)
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default()
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

    fn tool(registered: Vec<GoalEntry>) -> (GoalTool, Rc<RefCell<GoalDesk>>) {
        let desk = Rc::new(RefCell::new(GoalDesk::new(
            RunId::CITY,
            "potter@lab.1".to_owned(),
            registered,
        )));
        let tool = GoalTool::new(Address::parse("lab/room1").unwrap(), Rc::clone(&desk)).unwrap();
        (tool, desk)
    }

    fn call(args: Value) -> ToolCall {
        ToolCall {
            id: "tu_1".to_owned(),
            name: ToolName::parse("goal").unwrap(),
            args: Payload::new(args.as_object().unwrap().clone()).unwrap(),
        }
    }

    fn held(id: &str, path: &str, standing: bool) -> GoalEntry {
        GoalEntry {
            id: GoalId::new(id).unwrap(),
            owner: "mason@lab.2".to_owned(),
            resources: vec![GoalResource::Path(Address::parse(path).unwrap())],
            statement: "keep the kiln".to_owned(),
            standing,
        }
    }

    #[test]
    fn a_clear_claim_registers_and_the_next_one_in_the_same_run_sees_it() {
        let (mut tool, desk) = tool(Vec::new());
        tool.invoke(&call(serde_json::json!({
            "statement": "rewrite the notes",
            "paths": ["lab/room1/notes.md"],
        })))
        .unwrap();
        let second = tool.invoke(&call(serde_json::json!({
            "statement": "rewrite them again",
            "paths": ["lab/room1/notes.md"],
        })));
        assert!(
            second.is_err(),
            "a run that could claim the same ground twice would not be claiming anything"
        );
        let effects = desk.borrow_mut().take_effects();
        assert_eq!(effects.len(), 2);
        assert!(matches!(effects[0], GoalEffect::Registered(_)));
        assert!(matches!(effects[1], GoalEffect::Conflicted { .. }));
    }

    #[test]
    fn a_claim_on_held_ground_is_refused_with_the_level_that_decides_it() {
        let (mut tool, _desk) = tool(vec![held("g-held", "lab/room1", true)]);
        let refusal = tool
            .invoke(&call(serde_json::json!({
                "statement": "repaint the room",
                "paths": ["lab/room1/wall.md"],
                "standing": false,
            })))
            .unwrap_err();
        assert_eq!(refusal.code(), &AxCode::GoalConflict);
        let recovery = refusal.recovery();
        assert!(
            recovery.contains("g-held") && recovery.contains("wait"),
            "one standing goal against one piece of work serialises, and the refusal says so: \
             {recovery}"
        );
    }

    #[test]
    fn a_reading_that_no_machine_can_do_goes_to_a_resident_not_to_the_person() {
        let (mut tool, _desk) = tool(vec![held("g-held", "lab/room1", true)]);
        let refusal = tool
            .invoke(&call(serde_json::json!({
                "statement": "also keep the kiln",
                "paths": ["lab/room1"],
                "standing": true,
            })))
            .unwrap_err();
        let recovery = refusal.recovery();
        assert!(
            recovery.contains("signal the resident"),
            "two standing goals is a reading, and reading is a model's work: {recovery}"
        );
    }

    #[test]
    fn a_goal_that_claims_nothing_is_refused() {
        let (mut tool, _desk) = tool(Vec::new());
        let refusal = tool
            .invoke(&call(serde_json::json!({ "statement": "be helpful" })))
            .unwrap_err();
        assert_eq!(refusal.code(), &AxCode::InvalidArgs);
    }
}
