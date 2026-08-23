// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The face delegation shows a model: ask for one piece of work to be
//! carried out by somebody else, one level down.
//!
//! Everything this tool decides was already decided elsewhere. Whether a
//! spawn is admitted is `kernel::gate::spawn`, which has held the rule
//! since S2 and had no caller in production until this file: **a
//! delegated position spawns nothing, whatever kind it asks for.** What
//! this module adds is the desk that remembers what was asked for, so
//! the assembly layer - the only thing that can build a run - can start
//! it when the parent's wave is over.
//!
//! **A request is not a run.** The tool answers with the room the work
//! will happen in, not with a result, because starting a run inside a
//! tool call would mean driving a run from inside another run's tool
//! bench. The city dispatches what this desk holds after the parent's
//! turn settles, and the child's own `run_started` carries that room -
//! which is how a reader connects the two without a second event kind
//! for a fact the ledger already holds twice.

use kernel::{
    Address, AxCode, AxError, CostTier, DelegateKind, Depth, Effect, GateOutcome, Payload,
    RenderIntent, Temporal, Tool, ToolCall, ToolMeta, ToolName, ToolOutcome,
};
use serde_json::{Map, Value};

/// One piece of work a run asked somebody else to carry out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Delegated {
    /// Where the delegate works. Inside the parent's building, always:
    /// delegation shares a building's files, and a delegate sent to
    /// another building would be a cross-building write with no gate on
    /// it.
    pub room: Address,
    pub task: String,
    pub goal: String,
    pub kind: DelegateKind,
}

/// What a run asked for, held until the city can act on it.
#[derive(Debug)]
pub struct DelegateDesk {
    /// Where the asking run stands. A delegate holds `Delegated` and is
    /// refused by the gate; the value is carried rather than inferred,
    /// because a run that had to work out its own depth would be one
    /// wrong answer away from a grand-delegate.
    depth: Depth,
    /// The building the asking run works in, which bounds where a
    /// delegate may be put.
    building: Address,
    asked: Vec<Delegated>,
}

impl DelegateDesk {
    #[must_use]
    pub fn new(depth: Depth, building: Address) -> DelegateDesk {
        DelegateDesk {
            depth,
            building,
            asked: Vec::new(),
        }
    }

    /// Admits one request, or refuses it in the gate's own words.
    ///
    /// # Errors
    /// `E_DELEGATION_DEPTH` when a delegate asks to delegate, and
    /// `E_CROSS_BUILDING_DENIED` when the room named is not inside the
    /// asking run's building.
    pub fn ask(&mut self, work: Delegated) -> Result<&Delegated, AxError> {
        if let GateOutcome::Deny { refusal } = kernel::spawn(self.depth, &work.kind) {
            return Err(*refusal);
        }
        if !work.room.is_within(&self.building) {
            return Err(AxError::failure(
                AxCode::CrossBuildingDenied,
                "delegate work",
                work.room.as_str().to_owned(),
            )
            .with_recovery(format!(
                "put the delegate in a room inside {}; work in another building is asked for \
                 with `signal`",
                self.building.as_str()
            )));
        }
        self.asked.push(work);
        self.asked.last().ok_or_else(|| {
            AxError::failure(AxCode::InvalidArgs, "delegate work", "the request vanished")
        })
    }

    /// Everything asked for, in the order it was asked for, leaving the
    /// desk empty. Called once when the parent's turn settles.
    pub fn take(&mut self) -> Vec<Delegated> {
        std::mem::take(&mut self.asked)
    }
}

/// The tool itself.
pub struct DelegateTool {
    desk: std::rc::Rc<std::cell::RefCell<DelegateDesk>>,
    meta: ToolMeta,
}

impl DelegateTool {
    /// # Errors
    /// Propagates a malformed parameter schema, which is a build-time
    /// defect rather than a runtime one.
    pub fn new(
        desk: std::rc::Rc<std::cell::RefCell<DelegateDesk>>,
    ) -> Result<DelegateTool, AxError> {
        let mut properties = Map::new();
        for (field, description) in [
            ("room", "where the delegate works, as building/room"),
            ("task", "one line: what the delegate is to produce"),
            (
                "goal",
                "what counts as done and when to stop; a delegate without one does not stop",
            ),
            (
                "kind",
                "`ephemeral` for one piece of work, `resident` for somebody who stays. \
                 Defaults to ephemeral",
            ),
        ] {
            let mut spec = Map::new();
            spec.insert("type".to_owned(), Value::String("string".to_owned()));
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
            Value::Array(
                ["room", "task", "goal"]
                    .into_iter()
                    .map(|field| Value::String(field.to_owned()))
                    .collect(),
            ),
        );
        Ok(DelegateTool {
            desk,
            meta: ToolMeta {
                name: ToolName::parse("delegate")?,
                disclosure: "Hand one piece of work to another agent in this building, one level \
                             down. Only when the person has allowed it."
                    .to_owned(),
                params: Payload::new(params)?,
                effect: Effect::Read,
                cost_tier: CostTier::Heavy,
                timeout: None,
                render: RenderIntent::Generic,
                temporal: Temporal::Timeless,
            },
        })
    }
}

fn arg<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str, AxError> {
    args.get(key).and_then(Value::as_str).ok_or_else(|| {
        AxError::failure(
            AxCode::InvalidArgs,
            "delegate work",
            format!("missing string argument `{key}`"),
        )
        .with_recovery("pass room, task and goal - all strings")
    })
}

impl Tool for DelegateTool {
    fn meta(&self) -> &ToolMeta {
        &self.meta
    }

    fn invoke(&mut self, call: &ToolCall) -> Result<ToolOutcome, AxError> {
        if call.name != self.meta.name {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "delegate work",
                format!("call routed to the wrong tool: {}", call.name.as_str()),
            ));
        }
        let args = call.args.as_map();
        let room = Address::parse(arg(args, "room")?)?;
        // Exhaustive rather than permissive: an unknown word is refused
        // instead of quietly becoming the cheaper kind.
        let kind = match args.get("kind").and_then(Value::as_str) {
            None | Some("ephemeral") => DelegateKind::Ephemeral,
            Some("resident") => DelegateKind::Resident,
            Some(other) => {
                return Err(AxError::failure(
                    AxCode::InvalidArgs,
                    "delegate work",
                    format!("no such delegate kind: {other}"),
                )
                .with_recovery("use `ephemeral` for one piece of work, or `resident`"));
            }
        };
        let work = Delegated {
            room,
            task: arg(args, "task")?.to_owned(),
            goal: arg(args, "goal")?.to_owned(),
            kind,
        };
        let mut desk = self.desk.try_borrow_mut().map_err(|_| {
            AxError::failure(AxCode::InvalidArgs, "delegate work", "the desk is in use")
        })?;
        let accepted = desk.ask(work)?;
        let mut out = Map::new();
        out.insert(
            "room".to_owned(),
            Value::String(accepted.room.as_str().to_owned()),
        );
        out.insert(
            "starts".to_owned(),
            Value::String("when this turn settles".to_owned()),
        );
        Ok(ToolOutcome {
            result: Payload::new(out)?,
        })
    }
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

    fn desk(depth: Depth) -> std::rc::Rc<std::cell::RefCell<DelegateDesk>> {
        std::rc::Rc::new(std::cell::RefCell::new(DelegateDesk::new(
            depth,
            Address::parse("lab").unwrap(),
        )))
    }

    fn call(room: &str, kind: Option<&str>) -> ToolCall {
        let mut args = Map::new();
        args.insert("room".to_owned(), Value::String(room.to_owned()));
        args.insert("task".to_owned(), Value::String("measure it".to_owned()));
        args.insert(
            "goal".to_owned(),
            Value::String("a number, then stop".to_owned()),
        );
        if let Some(kind) = kind {
            args.insert("kind".to_owned(), Value::String(kind.to_owned()));
        }
        ToolCall {
            id: "call-1".to_owned(),
            name: ToolName::parse("delegate").unwrap(),
            args: Payload::new(args).unwrap(),
        }
    }

    #[test]
    fn a_root_run_may_hand_work_down_and_is_told_where_it_will_happen() {
        let desk = desk(Depth::Root);
        let mut tool = DelegateTool::new(std::rc::Rc::clone(&desk)).unwrap();
        let outcome = tool.invoke(&call("lab/helper", None)).unwrap();
        assert_eq!(outcome.result.as_map()["room"], "lab/helper");

        let taken = desk.borrow_mut().take();
        assert_eq!(taken.len(), 1);
        assert_eq!(taken[0].kind, DelegateKind::Ephemeral);
        assert!(desk.borrow_mut().take().is_empty(), "taken once");
    }

    /// The rule this whole mechanism exists to hold, now with a caller:
    /// one level deep, whatever kind is asked for.
    #[test]
    fn a_delegate_cannot_delegate_and_the_refusal_says_what_to_do_instead() {
        let desk = desk(Depth::Delegated);
        let mut tool = DelegateTool::new(std::rc::Rc::clone(&desk)).unwrap();
        for kind in [None, Some("ephemeral"), Some("resident")] {
            let err = tool.invoke(&call("lab/helper", kind)).unwrap_err();
            assert_eq!(err.code(), &AxCode::DelegationDepth);
            let refusal = err.gate().expect("a gate refusal has three parts");
            assert!(refusal.alternative().contains("return this subtask"));
        }
        assert!(desk.borrow_mut().take().is_empty());
    }

    #[test]
    fn a_delegate_stays_inside_the_building_that_asked_for_it() {
        let desk = desk(Depth::Root);
        let mut tool = DelegateTool::new(std::rc::Rc::clone(&desk)).unwrap();
        let err = tool.invoke(&call("shop/helper", None)).unwrap_err();
        assert_eq!(err.code(), &AxCode::CrossBuildingDenied);
        assert!(err.recovery().contains("signal"));
    }

    #[test]
    fn an_unknown_kind_is_refused_rather_than_rounded_down() {
        let desk = desk(Depth::Root);
        let mut tool = DelegateTool::new(std::rc::Rc::clone(&desk)).unwrap();
        let err = tool
            .invoke(&call("lab/helper", Some("apprentice")))
            .unwrap_err();
        assert_eq!(err.code(), &AxCode::InvalidArgs);
    }

    /// Fail-closed identity, the same assertion the port's conformance
    /// suite makes, written here because this crate does not turn that
    /// feature on.
    #[test]
    fn the_tool_refuses_another_tools_call_and_still_answers() {
        let desk = desk(Depth::Root);
        let mut tool = DelegateTool::new(desk).unwrap();
        let mut wrong = call("lab/helper", None);
        wrong.name = ToolName::parse("signal").unwrap();
        let err = tool.invoke(&wrong).unwrap_err();
        assert_eq!(err.code(), &AxCode::InvalidArgs);
        assert_eq!(tool.meta().name.as_str(), "delegate");
    }
}
