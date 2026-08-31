// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The signal tool: the face `collab::inbox` shows a model.
//!
//! Two orderings carry the design. Sending only queues an effect — the
//! delivery happens after the drive returns and after the enqueue is in
//! the ledger, because a projection may only change as a consequence of
//! a record that already exists. And the room's inbox is *lent* to the
//! desk for the length of the run rather than copied into it: two
//! queues would be two authorities on what order signals arrive in, and
//! the one that drifts is always the one nobody reads.
//!
//! Reach is handed in, not worked out here. Which building an address
//! belongs to is `city`'s answer, and this crate cannot name that crate;
//! what it can do is refuse anything outside the boundary it was given.

use std::cell::RefCell;
use std::rc::Rc;

use kernel::{
    Address, AxCode, AxError, CostTier, Effect, Payload, RenderIntent, RunId, Temporal, TimeMs,
    Tool, ToolCall, ToolMeta, ToolName, ToolOutcome, Version,
};
use serde_json::{Map, Value};

use crate::inbox::{Inbox, Signal, SignalId, SignalKind};

/// What the run did to the city's signals, in the order it did it. The
/// worker turns each of these into a ledger line once the drive is over.
///
/// Exhaustive on purpose, unlike most enums that cross a crate boundary
/// here: every variant is something the worker must write down, so a new
/// one has to be a compile error at the place that writes, not a runtime
/// arm nobody reaches until a signal quietly goes unrecorded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SignalEffect {
    /// Queued for delivery; nobody has it yet.
    Enqueued(Signal),
    /// Taken out of the run's own inbox and shown to the model.
    Consumed { signal: Signal, by: String },
}

/// The run's side of the city's signal traffic: its own inbox, on loan,
/// plus what it has done that the ledger does not know yet.
pub struct SignalDesk {
    run: RunId,
    room: Address,
    who: String,
    reach: Address,
    at: TimeMs,
    inbox: Inbox,
    effects: Vec<SignalEffect>,
    minted: u32,
}

impl std::fmt::Debug for SignalDesk {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SignalDesk")
            .field("room", &self.room)
            .field("pending", &self.inbox.pending())
            .field("effects", &self.effects.len())
            .finish()
    }
}

impl SignalDesk {
    /// `reach` bounds where this run may send: an address outside it is
    /// refused. `at` is the run's stamp — the tool face has no clock, and
    /// time only ever arrives as a parameter.
    #[must_use]
    pub fn new(
        run: RunId,
        room: Address,
        who: String,
        reach: Address,
        at: TimeMs,
        inbox: Inbox,
    ) -> SignalDesk {
        SignalDesk {
            run,
            room,
            who,
            reach,
            at,
            inbox,
            effects: Vec::new(),
            minted: 0,
        }
    }

    /// What `status` reports as `signals_pending` for this room.
    #[must_use]
    pub fn pending(&self) -> u32 {
        self.inbox.pending()
    }

    /// Takes one steer waiting for this run, ready to land at its next
    /// safe point.
    ///
    /// The landing is the same one the person's steer uses, and the
    /// attribution is what keeps them apart: [`crate::Steer::from_signal`]
    /// can only write `@<the sender's address>`, and only
    /// `Steer::from_person` can write `user`. A resident reading its own
    /// window can therefore tell who spoke, and the prefix it reads is
    /// the address it answers to — which is what makes a reply
    /// possible at all.
    ///
    /// Consuming it here is what the ledger records: a steer that landed
    /// in a window has been read, whether or not the model acts on it.
    pub fn take_steer(&mut self) -> Option<crate::Steer> {
        let signal = self.inbox.take_steer()?;
        let landed = crate::Steer::from_signal(&signal).ok()?;
        self.effects.push(SignalEffect::Consumed {
            signal,
            by: self.who.clone(),
        });
        Some(landed)
    }

    /// What the worker has to record. Draining is deliberate: an effect
    /// read twice would be a signal delivered twice.
    pub fn take_effects(&mut self) -> Vec<SignalEffect> {
        std::mem::take(&mut self.effects)
    }

    /// Gives the room its inbox back. The caller must do this on both
    /// the failing and the succeeding path — an inbox left in a dropped
    /// desk is a queue the city forgot it had.
    #[must_use]
    pub fn take_inbox(&mut self) -> Inbox {
        std::mem::replace(&mut self.inbox, Inbox::new(0, 1))
    }

    fn mint(&mut self) -> Result<SignalId, AxError> {
        self.minted = self.minted.checked_add(1).ok_or_else(|| {
            AxError::failure(
                AxCode::InvalidArgs,
                "mint a signal id",
                "this run has sent as many signals as one run can",
            )
            .with_recovery("freeze the run and dispatch again")
        })?;
        SignalId::parse(&format!("{}-s{}", self.run, self.minted))
    }

    fn send(&mut self, args: &Map<String, Value>) -> Result<Payload, AxError> {
        let to = Address::parse(text(args, "to", "send a signal")?)?;
        if !to.is_within(&self.reach) {
            return Err(AxError::failure(
                AxCode::CrossBuildingDenied,
                "send a signal",
                to.as_str().to_owned(),
            )
            .with_recovery(format!(
                "signal an address inside {}, or ask the person to carry it across",
                self.reach.as_str()
            )));
        }
        let kind = match args.get("kind").and_then(Value::as_str) {
            Some(raw) => SignalKind::parse(raw)?,
            None => SignalKind::Mention,
        };
        let body = text(args, "text", "send a signal")?.to_owned();
        let mut payload = Map::new();
        payload.insert("text".to_owned(), Value::String(body));
        let signal = Signal::new(
            self.mint()?,
            kind,
            self.who.clone(),
            to.clone(),
            // No room carries a version until drafts have a writer
            // (P3.02): the sender saw a room nobody has revised.
            Version::FIRST,
            Payload::new(payload)?,
            self.at,
        )?;
        let mut result = Map::new();
        result.insert(
            "id".to_owned(),
            Value::String(signal.id().as_str().to_owned()),
        );
        result.insert("to".to_owned(), Value::String(to.as_str().to_owned()));
        result.insert(
            "kind".to_owned(),
            Value::String(signal.kind().as_str().to_owned()),
        );
        result.insert("queued".to_owned(), Value::Bool(true));
        self.effects.push(SignalEffect::Enqueued(signal));
        Payload::new(result)
    }

    fn pull(&mut self) -> Result<Payload, AxError> {
        let taken = self.inbox.pull()?;
        let mut rows = Vec::with_capacity(taken.len());
        for signal in taken {
            let mut row = Map::new();
            row.insert("from".to_owned(), Value::String(signal.from().to_owned()));
            row.insert(
                "kind".to_owned(),
                Value::String(signal.kind().as_str().to_owned()),
            );
            row.insert(
                "text".to_owned(),
                signal
                    .payload()
                    .as_map()
                    .get("text")
                    .cloned()
                    .unwrap_or(Value::String(String::new())),
            );
            rows.push(Value::Object(row));
            self.effects.push(SignalEffect::Consumed {
                signal,
                by: self.who.clone(),
            });
        }
        let mut result = Map::new();
        result.insert("signals".to_owned(), Value::Array(rows));
        // The count in `status` is the fact as it stood when the run was
        // dispatched; this one is the fact as it stands now.
        result.insert(
            "remaining".to_owned(),
            Value::Number(self.inbox.pending().into()),
        );
        Payload::new(result)
    }
}

/// The tool itself: a thin router onto the desk, which is shared with
/// the worker because a registered tool is behind a `Box<dyn Tool>` and
/// nothing can reach into it afterwards.
pub struct SignalTool {
    meta: ToolMeta,
    desk: Rc<RefCell<SignalDesk>>,
}

impl SignalTool {
    /// # Errors
    /// Propagates a malformed tool name or parameter schema, neither of
    /// which can happen with the literals below — the fallibility is the
    /// constructors' contract, not a runtime condition.
    pub fn new(desk: Rc<RefCell<SignalDesk>>) -> Result<SignalTool, AxError> {
        let room = desk.borrow().room.clone();
        let mut properties = Map::new();
        for (field, description) in [
            (
                "action",
                "`send` to speak to another resident, `pull` to take what is waiting for you",
            ),
            ("to", "send only: the address you are speaking to"),
            (
                "kind",
                "send only: mention, thread, broadcast or steer; defaults to mention",
            ),
            ("text", "send only: what you want them to read"),
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
            Value::Array(vec![Value::String("action".to_owned())]),
        );
        Ok(SignalTool {
            meta: ToolMeta {
                name: ToolName::parse("signal")?,
                disclosure:
                    "Speak to another resident, or take the signals waiting for you; call it when \
                     status says signals are pending."
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

impl Tool for SignalTool {
    fn meta(&self) -> &ToolMeta {
        &self.meta
    }

    fn invoke(&mut self, call: &ToolCall) -> Result<ToolOutcome, AxError> {
        if call.name != self.meta.name {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "send a signal",
                format!("call routed to the wrong tool: {}", call.name.as_str()),
            ));
        }
        let args = call.args.as_map();
        let mut desk = self.desk.try_borrow_mut().map_err(|_| {
            AxError::failure(
                AxCode::InvalidArgs,
                "reach the signal desk",
                "the desk is already in use",
            )
            .with_recovery("call the tool once at a time")
        })?;
        let result = match text(args, "action", "read a signal action")? {
            "send" => desk.send(args)?,
            "pull" => desk.pull()?,
            other => {
                return Err(AxError::failure(
                    AxCode::InvalidArgs,
                    "read a signal action",
                    other.to_owned(),
                )
                .with_recovery("use `send` or `pull`"));
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
    clippy::arithmetic_side_effects,
    reason = "test code"
)]
mod tests {
    use super::*;

    fn desk(room: &str, reach: &str) -> Rc<RefCell<SignalDesk>> {
        Rc::new(RefCell::new(SignalDesk::new(
            RunId::CITY,
            Address::parse(room).unwrap(),
            "potter@lab.1".to_owned(),
            Address::parse(reach).unwrap(),
            TimeMs::new(1_700_000_000_000),
            Inbox::new(64, 4),
        )))
    }

    fn call(args: Value) -> ToolCall {
        ToolCall {
            id: "tu_1".to_owned(),
            name: ToolName::parse("signal").unwrap(),
            args: Payload::new(args.as_object().unwrap().clone()).unwrap(),
        }
    }

    #[test]
    fn sending_queues_an_effect_and_delivers_nothing_yet() {
        let shared = desk("lab/room1", "lab");
        let mut tool = SignalTool::new(Rc::clone(&shared)).unwrap();
        tool.invoke(&call(serde_json::json!({
            "action": "send",
            "to": "lab/room2",
            "text": "the kiln is free",
        })))
        .unwrap();
        let mut borrowed = shared.borrow_mut();
        assert_eq!(
            borrowed.pending(),
            0,
            "the sender's own inbox is not where a sent signal goes"
        );
        let effects = borrowed.take_effects();
        assert_eq!(effects.len(), 1);
        match &effects[0] {
            SignalEffect::Enqueued(signal) => {
                assert_eq!(signal.room().as_str(), "lab/room2");
                assert_eq!(signal.from(), "potter@lab.1");
            }
            other => panic!("a send queues an enqueue, not {other:?}"),
        }
        assert!(
            borrowed.take_effects().is_empty(),
            "an effect read twice would be a signal delivered twice"
        );
    }

    #[test]
    fn a_signal_addressed_outside_the_building_is_refused_with_somewhere_to_go() {
        let shared = desk("lab/room1", "lab");
        let mut tool = SignalTool::new(Rc::clone(&shared)).unwrap();
        let refusal = tool
            .invoke(&call(serde_json::json!({
                "action": "send",
                "to": "mill/room1",
                "text": "hello",
            })))
            .unwrap_err();
        assert_eq!(refusal.code(), &AxCode::CrossBuildingDenied);
        assert!(
            refusal.recovery().contains("lab"),
            "the third part of a refusal names where the caller may go instead"
        );
        assert!(
            shared.borrow_mut().take_effects().is_empty(),
            "a refused send leaves nothing behind"
        );
    }

    #[test]
    fn pulling_takes_what_is_waiting_and_says_what_is_left() {
        let shared = desk("lab/room1", "lab");
        for n in 0..5u32 {
            let mut body = Map::new();
            body.insert("text".to_owned(), Value::String(format!("note {n}")));
            let signal = Signal::new(
                SignalId::parse(&format!("s{n}")).unwrap(),
                SignalKind::Mention,
                "mason@lab.2".to_owned(),
                Address::parse("lab/room1").unwrap(),
                Version::FIRST,
                Payload::new(body).unwrap(),
                TimeMs::new(10),
            )
            .unwrap();
            shared.borrow_mut().inbox.deliver(&signal).unwrap();
        }
        let mut tool = SignalTool::new(Rc::clone(&shared)).unwrap();
        let outcome = tool
            .invoke(&call(serde_json::json!({ "action": "pull" })))
            .unwrap();
        let map = outcome.result.as_map();
        assert_eq!(
            map.get("signals").and_then(Value::as_array).unwrap().len(),
            4,
            "a pull takes the receiver's bandwidth, not the sender's volume"
        );
        assert_eq!(
            map.get("remaining").and_then(Value::as_u64),
            Some(1),
            "what is left is in the answer, because status only knows the dispatch"
        );
        assert_eq!(shared.borrow_mut().take_effects().len(), 4);
    }

    #[test]
    fn an_action_this_tool_does_not_have_is_refused_by_name() {
        let shared = desk("lab/room1", "lab");
        let mut tool = SignalTool::new(Rc::clone(&shared)).unwrap();
        let refusal = tool
            .invoke(&call(serde_json::json!({ "action": "broadcast_all" })))
            .unwrap_err();
        assert_eq!(refusal.code(), &AxCode::InvalidArgs);
        assert!(refusal.recovery().contains("pull"));
    }

    #[test]
    fn the_lent_inbox_comes_back() {
        let shared = desk("lab/room1", "lab");
        let mut body = Map::new();
        body.insert("text".to_owned(), Value::String("later".to_owned()));
        let signal = Signal::new(
            SignalId::parse("s1").unwrap(),
            SignalKind::Mention,
            "mason@lab.2".to_owned(),
            Address::parse("lab/room1").unwrap(),
            Version::FIRST,
            Payload::new(body).unwrap(),
            TimeMs::new(10),
        )
        .unwrap();
        shared.borrow_mut().inbox.deliver(&signal).unwrap();
        let returned = shared.borrow_mut().take_inbox();
        assert_eq!(returned.pending(), 1);
        assert_eq!(
            shared.borrow().pending(),
            0,
            "what was handed back is no longer held twice"
        );
    }
}
