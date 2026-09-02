// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The turn typestate: Assembling -> Calling ->
//! ToolWave -> Recording, phase changes carrying `&mut dyn Ledger`.
//! Interrupts are consumed *only* at phase boundaries — the boundary
//! snapshot is a parameter of every transition, and no method exists that
//! could observe one mid-phase. That absence is A9's structural half.
//! Steer consumes at a boundary too, but advances: it is an addition to
//! the window, not an ending.
//!
//! All events of one turn share the timestamp given to [`Turn::begin`]:
//! order is `seq`'s business, time is a parameter, never sampled.

use kernel::{
    AxCode, AxError, B3Hash, BuildingPolicy, ChatRequest, ContentBlock, EventDraft, EventKind,
    EventRef, Ledger, Model, ModelRequest, ModelReturn, Payload, RunId, TimeMs, ToolCall, ToolDef,
    ToolOutcome, content_from_message,
};
use serde_json::{Map, Value};

use crate::prefix::FrozenPrefix;
use crate::window::Window;

/// Boundary snapshot, supplied by the executor at every phase change.
/// `Cancel` ends the turn at the boundary; `Steer` records and advances
/// (the executor folds the text into its `Window`).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Interrupt {
    None,
    Cancel,
    Steer { source: String, text: String },
}

/// A phase change either advances or ends the turn at the boundary.
/// Deliberately exhaustive: a new outcome must force every executor to
/// decide, not fall through a catch-all (14.3's non_exhaustive rule
/// covers wire enums, not verdicts).
#[derive(Debug)]
pub enum PhaseOutcome<Next> {
    Advanced(Next),
    Cancelled(TurnCancelled),
}

/// The turn ended at a boundary: `cancel_received` is on the ledger and
/// its ref is the last entry. Freezing (handoff + run_frozen) is the run
/// loop's move, not the turn's.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnCancelled {
    refs: Vec<EventRef>,
}

impl TurnCancelled {
    pub fn refs(&self) -> &[EventRef] {
        &self.refs
    }
}

/// What a completed turn hands the run loop. `assistant` and
/// `wave_results` are the window-folding material — the same content the
/// ledger carries in `model_returned.data.content` and `tool_result`
/// events (C16: live folding and offline rebuild share one source).
#[derive(Debug, Clone, PartialEq)]
pub struct TurnReport {
    refs: Vec<EventRef>,
    model_returned: EventRef,
    calls_made: usize,
    assistant: Vec<ContentBlock>,
    wave_results: Vec<ContentBlock>,
}

impl TurnReport {
    pub fn refs(&self) -> &[EventRef] {
        &self.refs
    }

    /// The in-window evidence candidate for `Completion::Done`.
    pub fn model_returned(&self) -> &EventRef {
        &self.model_returned
    }

    pub fn calls_made(&self) -> usize {
        self.calls_made
    }

    pub fn assistant(&self) -> &[ContentBlock] {
        &self.assistant
    }

    pub fn wave_results(&self) -> &[ContentBlock] {
        &self.wave_results
    }
}

/// Which model this run calls, how much it may say, and how hard it may
/// think — the frozen `[model]` config section as the turn sees it.
/// `effort` comes from [`kernel::FrozenConfig`] and cannot change within
/// the run: the provider renders it into the cached prompt prefix.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CallShape {
    pub model: String,
    pub max_tokens: u64,
    pub effort: Option<kernel::Effort>,
}

/// The typestate carrier. Phase data lives in `S` and is private to this
/// module: a phase literal cannot be forged, a phase cannot be skipped,
/// and no method returns an earlier phase.
#[derive(Debug)]
pub struct Turn<S> {
    run: RunId,
    who: String,
    t: TimeMs,
    refs: Vec<EventRef>,
    state: S,
}

#[derive(Debug)]
pub struct Assembling(());

#[derive(Debug)]
pub struct Calling {
    segments: [B3Hash; 4],
    chat: ChatRequest,
}

#[derive(Debug)]
pub struct ToolWave {
    calls: Vec<ToolCall>,
    model_returned: EventRef,
    assistant: Vec<ContentBlock>,
}

#[derive(Debug)]
pub struct Recording {
    model_returned: EventRef,
    calls_made: usize,
    assistant: Vec<ContentBlock>,
    wave_results: Vec<ContentBlock>,
}

fn payload(map: Map<String, Value>) -> Result<Payload, AxError> {
    Payload::new(map)
}

impl Turn<Assembling> {
    /// Opens a turn. `t` stamps every event of this turn; the executor
    /// advances it between turns (determinism rule 2).
    pub fn begin(run: RunId, who: String, t: TimeMs) -> Turn<Assembling> {
        Turn {
            run,
            who,
            t,
            refs: Vec::new(),
            state: Assembling(()),
        }
    }

    /// Boundary 1 (before assembly). Builds the canonical request from
    /// the frozen prefix (system blocks), the window (messages) and the
    /// catalog's tool defs; appends `prompt_assembled` with the prefix's
    /// full source notes.
    pub fn assemble(
        mut self,
        interrupt: Interrupt,
        ledger: &mut dyn Ledger,
        prefix: &FrozenPrefix,
        window: &Window,
        tools: &[ToolDef],
        shape: &CallShape,
    ) -> Result<PhaseOutcome<Turn<Calling>>, AxError> {
        if let Some(cancelled) = self.consume_boundary(interrupt, ledger)? {
            return Ok(PhaseOutcome::Cancelled(cancelled));
        }
        let prompt = prefix.prompt_payload()?;
        let echo = ledger.append(self.draft(EventKind::PromptAssembled, prompt))?;
        self.refs.push(echo);
        let chat = ChatRequest {
            model: shape.model.clone(),
            max_tokens: shape.max_tokens,
            system: prefix.system_blocks()?,
            messages: window.messages().to_vec(),
            tools: tools.to_vec(),
            effort: shape.effort,
        };
        Ok(PhaseOutcome::Advanced(Turn {
            run: self.run,
            who: self.who,
            t: self.t,
            refs: self.refs,
            state: Calling {
                segments: prefix.segment_hashes(),
                chat,
            },
        }))
    }
}

impl Turn<Calling> {
    /// Boundary 2 (before the provider call). Appends `model_called` and
    /// `model_returned`; a provider Err propagates after nothing but the
    /// boundary consumption touched the ledger.
    /// `'sink` is named rather than elided because the caller holds the
    /// sink for the whole run and hands it to every turn: with an elided
    /// lifetime the reborrow would have to shrink the trait object's own
    /// lifetime, which `&mut` does not permit.
    pub fn call<'sink>(
        mut self,
        interrupt: Interrupt,
        ledger: &mut dyn Ledger,
        model: &mut dyn Model,
        policy: &BuildingPolicy,
        deltas: Option<&mut (dyn FnMut(&str) + 'sink)>,
    ) -> Result<PhaseOutcome<Turn<ToolWave>>, AxError> {
        if let Some(cancelled) = self.consume_boundary(interrupt, ledger)? {
            return Ok(PhaseOutcome::Cancelled(cancelled));
        }
        let Calling { segments, chat } = self.state;
        let request = ModelRequest {
            policy: policy.clone(),
            segments,
            chat,
        };
        let mut called = Map::new();
        called.insert(
            "segments".to_owned(),
            Value::Array(
                request
                    .segments
                    .iter()
                    .map(|hash| Value::String(hash.to_string()))
                    .collect(),
            ),
        );
        called.insert(
            "model".to_owned(),
            Value::String(request.chat.model.clone()),
        );
        let echo = ledger.append(EventDraft {
            run: self.run,
            t: self.t,
            who: self.who.clone(),
            addr: None,
            kind: EventKind::ModelCalled,
            data: payload(called)?,
            ig: false,
        })?;
        self.refs.push(echo);
        // The streaming door when somebody is watching, the blocking one
        // when nobody is. Both return the same `ModelReturn`, and the
        // record below is written from that return in either case - so
        // what a page sees arriving and what the ledger keeps cannot come
        // from two different readings of one reply.
        let returned_value = match deltas {
            Some(onto) => model.call_streaming(&request, onto)?,
            None => model.call(&request)?,
        };
        let ModelReturn {
            message,
            calls,
            usage,
            stop,
            billed_usd_micros,
        } = returned_value;
        let assistant = content_from_message(&message)?;
        let mut returned = Map::new();
        returned.insert(
            "message".to_owned(),
            serde_json::to_value(&message).map_err(|err| {
                AxError::failure(AxCode::InvalidArgs, "encode model message", err.to_string())
            })?,
        );
        let calls_len = u64::try_from(calls.len()).map_err(|_| {
            AxError::failure(AxCode::InvalidArgs, "encode model return", "wave too large")
        })?;
        returned.insert("calls".to_owned(), Value::Number(calls_len.into()));
        if let Some(usage) = &usage {
            returned.insert(
                "usage".to_owned(),
                serde_json::to_value(usage).map_err(|err| {
                    AxError::failure(AxCode::InvalidArgs, "encode usage", err.to_string())
                })?,
            );
        }
        if let Some(stop) = &stop {
            returned.insert(
                "stop".to_owned(),
                serde_json::to_value(stop).map_err(|err| {
                    AxError::failure(AxCode::InvalidArgs, "encode stop reason", err.to_string())
                })?,
            );
        }
        if let Some(billed) = billed_usd_micros {
            returned.insert(
                "billed_usd_micros".to_owned(),
                Value::Number(billed.get().into()),
            );
        }
        // The window already holds the blocks this turn will send back,
        // so redacting here cannot break a thinking block's signature.
        // What it does stop is a key the model repeated from becoming a
        // permanent, exportable line of history.
        let (returned, _redacted) = crate::redact::redact(&returned);
        let model_returned = ledger.append(EventDraft {
            run: self.run,
            t: self.t,
            who: self.who.clone(),
            addr: None,
            kind: EventKind::ModelReturned,
            data: payload(returned)?,
            ig: false,
        })?;
        self.refs.push(model_returned);
        Ok(PhaseOutcome::Advanced(Turn {
            run: self.run,
            who: self.who,
            t: self.t,
            refs: self.refs,
            state: ToolWave {
                calls,
                model_returned,
                assistant,
            },
        }))
    }
}

impl Turn<ToolWave> {
    /// Boundary 3 (before tool execution). Serial: parallel execution
    /// with serial accounting stays future work; accounting order is the
    /// call order. A tool Err is not a turn Err — it lands in
    /// `tool_result` and goes back to the model (the model is the
    /// recovery subject).
    pub fn execute(
        mut self,
        interrupt: Interrupt,
        ledger: &mut dyn Ledger,
        invoke: &mut dyn FnMut(&ToolCall) -> Result<ToolOutcome, AxError>,
    ) -> Result<PhaseOutcome<Turn<Recording>>, AxError> {
        if let Some(cancelled) = self.consume_boundary(interrupt, ledger)? {
            return Ok(PhaseOutcome::Cancelled(cancelled));
        }
        let calls = std::mem::take(&mut self.state.calls);
        let mut wave_results = Vec::new();
        for call in &calls {
            let mut called = Map::new();
            called.insert("id".to_owned(), Value::String(call.id.clone()));
            called.insert("name".to_owned(), Value::String(call.name.to_string()));
            called.insert(
                "args".to_owned(),
                serde_json::to_value(&call.args).map_err(|err| {
                    AxError::failure(AxCode::InvalidArgs, "encode tool args", err.to_string())
                })?,
            );
            let echo = ledger.append(self.draft(EventKind::ToolCalled, payload(called)?))?;
            self.refs.push(echo);
            let mut result = Map::new();
            result.insert("tool_use_id".to_owned(), Value::String(call.id.clone()));
            result.insert("name".to_owned(), Value::String(call.name.to_string()));
            let (content, is_error) = match invoke(call) {
                Ok(ToolOutcome { result: outcome }) => {
                    let value = serde_json::to_value(&outcome).map_err(|err| {
                        AxError::failure(AxCode::InvalidArgs, "encode tool result", err.to_string())
                    })?;
                    result.insert("result".to_owned(), value.clone());
                    (
                        serde_json::to_string(&value).map_err(|err| {
                            AxError::failure(
                                AxCode::InvalidArgs,
                                "encode tool result",
                                err.to_string(),
                            )
                        })?,
                        false,
                    )
                }
                Err(tool_err) => {
                    let value = serde_json::to_value(&tool_err).map_err(|err| {
                        AxError::failure(AxCode::InvalidArgs, "encode tool error", err.to_string())
                    })?;
                    result.insert("error".to_owned(), value.clone());
                    (
                        serde_json::to_string(&value).map_err(|err| {
                            AxError::failure(
                                AxCode::InvalidArgs,
                                "encode tool error",
                                err.to_string(),
                            )
                        })?,
                        true,
                    )
                }
            };
            wave_results.push(ContentBlock::ToolResult {
                tool_use_id: call.id.clone(),
                content,
                is_error,
            });
            let echo = ledger.append(self.draft(EventKind::ToolResult, payload(result)?))?;
            self.refs.push(echo);
        }
        Ok(PhaseOutcome::Advanced(Turn {
            run: self.run,
            who: self.who,
            t: self.t,
            refs: self.refs,
            state: Recording {
                model_returned: self.state.model_returned,
                calls_made: calls.len(),
                assistant: self.state.assistant,
                wave_results,
            },
        }))
    }
}

impl Turn<Recording> {
    /// Closes the turn. Recording is the accounting boundary: nothing
    /// extra is appended here, because every effect is already on the
    /// ledger. What the phase exists for is the fourth boundary — the
    /// last moment before the run acts on what this turn decided,
    /// including the work it handed down.
    ///
    /// # Errors
    /// Propagates the ledger's refusal to record the boundary event.
    pub fn record(
        mut self,
        interrupt: Interrupt,
        ledger: &mut dyn Ledger,
    ) -> Result<PhaseOutcome<TurnReport>, AxError> {
        if let Some(cancelled) = self.consume_boundary(interrupt, ledger)? {
            return Ok(PhaseOutcome::Cancelled(cancelled));
        }
        Ok(PhaseOutcome::Advanced(TurnReport {
            refs: self.refs,
            model_returned: self.state.model_returned,
            calls_made: self.state.calls_made,
            assistant: self.state.assistant,
            wave_results: self.state.wave_results,
        }))
    }
}

impl<S> Turn<S> {
    fn draft(&self, kind: EventKind, data: Payload) -> EventDraft {
        EventDraft {
            run: self.run,
            t: self.t,
            who: self.who.clone(),
            addr: None,
            kind,
            data,
            ig: false,
        }
    }

    /// The one interrupt consumer. Cancel appends `cancel_received` and
    /// ends the turn; Steer appends `steer_received` and advances — the
    /// executor folds the text into its window for the next assembly.
    fn consume_boundary(
        &mut self,
        interrupt: Interrupt,
        ledger: &mut dyn Ledger,
    ) -> Result<Option<TurnCancelled>, AxError> {
        match interrupt {
            Interrupt::None => Ok(None),
            Interrupt::Cancel => {
                let echo =
                    ledger.append(self.draft(EventKind::CancelReceived, Payload::empty()))?;
                self.refs.push(echo);
                let mut refs = std::mem::take(&mut self.refs);
                refs.shrink_to_fit();
                Ok(Some(TurnCancelled { refs }))
            }
            Interrupt::Steer { source, text } => {
                let mut map = Map::new();
                map.insert("source".to_owned(), Value::String(source));
                map.insert("text".to_owned(), Value::String(text));
                let echo = ledger.append(self.draft(EventKind::SteerReceived, payload(map)?))?;
                self.refs.push(echo);
                Ok(None)
            }
        }
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
    use crate::prefix::{FrozenSegment, SegmentSlot};
    use crate::window::Opening;
    use kernel::{GENESIS_PREV, chain_hash};

    /// Minimal in-memory ledger for turn tests (the citysim MemLedger is
    /// the real second adapter; this one keeps the crate's tests local).
    struct TestLedger {
        lines: Vec<Vec<u8>>,
        next: kernel::Seq,
        prev: B3Hash,
    }

    impl TestLedger {
        fn new() -> Self {
            TestLedger {
                lines: Vec::new(),
                next: kernel::Seq::FIRST,
                prev: GENESIS_PREV,
            }
        }

        fn kinds(&self) -> Vec<String> {
            self.lines
                .iter()
                .map(|line| {
                    let value: serde_json::Value = serde_json::from_slice(line).unwrap();
                    value["kind"].as_str().unwrap().to_owned()
                })
                .collect()
        }
    }

    impl Ledger for TestLedger {
        fn append(&mut self, draft: EventDraft) -> Result<EventRef, AxError> {
            let record = kernel::EventRecord::from_draft(draft, self.next, self.prev);
            let line = record.canonical_line()?;
            self.prev = chain_hash(&line);
            self.next = self.next.next()?;
            let echo = record.to_ref();
            self.lines.push(line);
            Ok(echo)
        }
    }

    struct OneShotModel {
        calls: Vec<ToolCall>,
    }

    impl Model for OneShotModel {
        fn call(&mut self, _req: &ModelRequest) -> Result<ModelReturn, AxError> {
            Ok(ModelReturn::bare(
                kernel::message_payload(&[ContentBlock::Text {
                    text: "thinking".to_owned(),
                }])
                .unwrap(),
                std::mem::take(&mut self.calls),
            ))
        }
    }

    fn prefix() -> FrozenPrefix {
        FrozenPrefix::assemble(
            FrozenSegment::new(SegmentSlot::City, b"c".to_vec()),
            FrozenSegment::new(SegmentSlot::Building, b"b".to_vec()),
            FrozenSegment::new(SegmentSlot::Resident, b"r".to_vec()),
            FrozenSegment::new(SegmentSlot::Run, b"j".to_vec()),
        )
        .unwrap()
    }

    fn run_id() -> RunId {
        RunId::parse("0198f6a2-7c4a-7bbb-9d1e-00000000000a").unwrap()
    }

    fn shape() -> CallShape {
        CallShape {
            model: "script".to_owned(),
            max_tokens: 512,
            effort: None,
        }
    }

    fn advance<N>(outcome: PhaseOutcome<N>) -> N {
        match outcome {
            PhaseOutcome::Advanced(next) => next,
            PhaseOutcome::Cancelled(_) => panic!("expected the phase to advance"),
        }
    }

    fn probe_call() -> ToolCall {
        ToolCall {
            id: "call-1".to_owned(),
            name: kernel::ToolName::parse("probe").unwrap(),
            args: Payload::empty(),
        }
    }

    #[test]
    fn a_full_turn_appends_the_canonical_event_sequence() {
        let mut ledger = TestLedger::new();
        let mut model = OneShotModel {
            calls: vec![probe_call()],
        };
        let mut window = Window::new();
        window.push_task_lines("probe the city", "one probe", Opening::FromJob);
        let turn = Turn::begin(run_id(), "resident@sim.1".into(), TimeMs::new(1));
        let turn = advance(
            turn.assemble(
                Interrupt::None,
                &mut ledger,
                &prefix(),
                &window,
                &[],
                &shape(),
            )
            .unwrap(),
        );
        let turn = advance(
            turn.call(
                Interrupt::None,
                &mut ledger,
                &mut model,
                &BuildingPolicy::default(),
                None,
            )
            .unwrap(),
        );
        let mut invoked = 0u32;
        let turn = advance(
            turn.execute(Interrupt::None, &mut ledger, &mut |_call| {
                invoked += 1;
                Ok(ToolOutcome {
                    result: Payload::empty(),
                })
            })
            .unwrap(),
        );
        let PhaseOutcome::Advanced(report) = turn.record(Interrupt::None, &mut ledger).unwrap()
        else {
            panic!("the boundary was not interrupted");
        };
        assert_eq!(invoked, 1);
        assert_eq!(report.calls_made(), 1);
        assert_eq!(
            ledger.kinds(),
            [
                "prompt_assembled",
                "model_called",
                "model_returned",
                "tool_called",
                "tool_result"
            ]
        );
        assert_eq!(report.refs().len(), 5);
        assert_eq!(
            report.model_returned().kind(),
            kernel::EventKind::ModelReturned
        );
        // Window-folding material mirrors the ledger content.
        assert_eq!(report.assistant().len(), 1);
        assert_eq!(report.wave_results().len(), 1);
        match &report.wave_results()[0] {
            ContentBlock::ToolResult {
                tool_use_id,
                is_error,
                ..
            } => {
                assert_eq!(tool_use_id, "call-1");
                assert!(!is_error);
            }
            other => panic!("expected a tool_result block, got {other:?}"),
        }
        // model_called carries the duty name; tool events carry ids.
        let called: serde_json::Value = serde_json::from_slice(&ledger.lines[1]).unwrap();
        assert_eq!(called["data"]["model"], "script");
        let tool_called: serde_json::Value = serde_json::from_slice(&ledger.lines[3]).unwrap();
        assert_eq!(tool_called["data"]["id"], "call-1");
        let tool_result: serde_json::Value = serde_json::from_slice(&ledger.lines[4]).unwrap();
        assert_eq!(tool_result["data"]["tool_use_id"], "call-1");
    }

    #[test]
    fn cancel_at_the_call_boundary_stops_before_any_model_bytes() {
        let mut ledger = TestLedger::new();
        let mut model = OneShotModel { calls: vec![] };
        let turn = Turn::begin(run_id(), "resident@sim.1".into(), TimeMs::new(1));
        let turn = advance(
            turn.assemble(
                Interrupt::None,
                &mut ledger,
                &prefix(),
                &Window::new(),
                &[],
                &shape(),
            )
            .unwrap(),
        );
        let outcome = turn
            .call(
                Interrupt::Cancel,
                &mut ledger,
                &mut model,
                &BuildingPolicy::default(),
                None,
            )
            .unwrap();
        match outcome {
            PhaseOutcome::Cancelled(cancelled) => {
                assert_eq!(
                    cancelled.refs().last().unwrap().kind(),
                    kernel::EventKind::CancelReceived
                );
            }
            PhaseOutcome::Advanced(_) => panic!("cancel must end the turn at the boundary"),
        }
        assert_eq!(ledger.kinds(), ["prompt_assembled", "cancel_received"]);
    }

    #[test]
    fn steer_at_a_boundary_records_and_advances() {
        let mut ledger = TestLedger::new();
        let mut model = OneShotModel { calls: vec![] };
        let turn = Turn::begin(run_id(), "resident@sim.1".into(), TimeMs::new(4));
        let turn = advance(
            turn.assemble(
                Interrupt::Steer {
                    source: "user".to_owned(),
                    text: "prefer the short route".to_owned(),
                },
                &mut ledger,
                &prefix(),
                &Window::new(),
                &[],
                &shape(),
            )
            .unwrap(),
        );
        let _ = advance(
            turn.call(
                Interrupt::None,
                &mut ledger,
                &mut model,
                &BuildingPolicy::default(),
                None,
            )
            .unwrap(),
        );
        assert_eq!(
            ledger.kinds(),
            [
                "steer_received",
                "prompt_assembled",
                "model_called",
                "model_returned"
            ]
        );
        let steer: serde_json::Value = serde_json::from_slice(&ledger.lines[0]).unwrap();
        assert_eq!(steer["data"]["source"], "user");
        assert_eq!(steer["data"]["text"], "prefer the short route");
    }

    /// The two openings are two situations, and the words differ.
    /// A session nobody assigned a task to gets the person's own line,
    /// because a conversational turn wrapped in field labels reads as a
    /// form and is answered as one.
    #[test]
    fn a_session_with_a_person_opens_in_the_persons_own_words() {
        let mut assigned = Window::new();
        assigned.push_task_lines("close the loop", "one turn, then stop", Opening::FromJob);
        let ContentBlock::Text { text } = &assigned.messages()[0].content[0] else {
            panic!("the dispatch lines are text");
        };
        assert_eq!(text, "Task: close the loop\nGoal: one turn, then stop");

        let mut talking = Window::new();
        talking.push_task_lines("what do you make of this", "", Opening::WithPerson);
        let ContentBlock::Text { text } = &talking.messages()[0].content[0] else {
            panic!("the dispatch line is text");
        };
        assert_eq!(text, "what do you make of this");
    }

    /// The job file's text is the prefix's run segment, so nothing sends
    /// the agent to fetch what it was already handed. Before this, the
    /// opening line carried a `cas:` hash no tool in the city can resolve.
    #[test]
    fn no_opening_line_points_at_a_file_the_agent_already_has() {
        for (goal, opening) in [
            ("stop when it builds", Opening::FromJob),
            ("", Opening::WithPerson),
        ] {
            let mut window = Window::new();
            window.push_task_lines("do the thing", goal, opening);
            let ContentBlock::Text { text } = &window.messages()[0].content[0] else {
                panic!("the opening is text");
            };
            assert!(!text.contains("FULL READ"), "{opening:?} still points away");
            assert!(!text.contains("cas:"), "{opening:?} carries a content hash");
        }
    }

    #[test]
    fn the_window_folds_steer_into_the_open_user_message() {
        let mut window = Window::new();
        window.push_tool_results(vec![ContentBlock::ToolResult {
            tool_use_id: "call-1".to_owned(),
            content: "{}".to_owned(),
            is_error: false,
        }]);
        window.push_steer("user", "look again");
        assert_eq!(
            window.messages().len(),
            1,
            "steer rides the open user message"
        );
        assert_eq!(window.messages()[0].content.len(), 2);
        window.push_assistant(vec![ContentBlock::Text {
            text: "ok".to_owned(),
        }]);
        window.push_steer("@planner", "hurry");
        assert_eq!(
            window.messages().len(),
            3,
            "steer after assistant opens a new message"
        );
    }

    #[test]
    fn a_tool_error_lands_in_tool_result_not_in_the_turn() {
        let mut ledger = TestLedger::new();
        let mut model = OneShotModel {
            calls: vec![ToolCall {
                id: "call-9".to_owned(),
                name: kernel::ToolName::parse("broken").unwrap(),
                args: Payload::empty(),
            }],
        };
        let turn = Turn::begin(run_id(), "resident@sim.1".into(), TimeMs::new(2));
        let turn = advance(
            turn.assemble(
                Interrupt::None,
                &mut ledger,
                &prefix(),
                &Window::new(),
                &[],
                &shape(),
            )
            .unwrap(),
        );
        let turn = advance(
            turn.call(
                Interrupt::None,
                &mut ledger,
                &mut model,
                &BuildingPolicy::default(),
                None,
            )
            .unwrap(),
        );
        let turn = advance(
            turn.execute(Interrupt::None, &mut ledger, &mut |call| {
                Err(AxError::failure(
                    AxCode::ToolUnavailable,
                    "invoke tool",
                    call.name.to_string(),
                ))
            })
            .unwrap(),
        );
        let PhaseOutcome::Advanced(report) = turn.record(Interrupt::None, &mut ledger).unwrap()
        else {
            panic!("the boundary was not interrupted");
        };
        assert_eq!(report.calls_made(), 1);
        let last = ledger.lines.last().unwrap();
        let value: serde_json::Value = serde_json::from_slice(last).unwrap();
        assert_eq!(value["kind"], "tool_result");
        assert_eq!(value["data"]["error"]["code"], "E_TOOL_UNAVAILABLE");
        match &report.wave_results()[0] {
            ContentBlock::ToolResult { is_error, .. } => assert!(is_error),
            other => panic!("expected a tool_result block, got {other:?}"),
        }
    }

    #[test]
    fn the_ledger_chain_stays_verifiable_after_a_turn() {
        let mut ledger = TestLedger::new();
        let mut model = OneShotModel { calls: vec![] };
        let turn = Turn::begin(run_id(), "resident@sim.1".into(), TimeMs::new(3));
        let turn = advance(
            turn.assemble(
                Interrupt::None,
                &mut ledger,
                &prefix(),
                &Window::new(),
                &[],
                &shape(),
            )
            .unwrap(),
        );
        let turn = advance(
            turn.call(
                Interrupt::None,
                &mut ledger,
                &mut model,
                &BuildingPolicy::default(),
                None,
            )
            .unwrap(),
        );
        let turn = advance(
            turn.execute(Interrupt::None, &mut ledger, &mut |_call| {
                panic!("empty wave must not invoke")
            })
            .unwrap(),
        );
        let _report = turn.record(Interrupt::None, &mut ledger).unwrap();
        crate::replay::verify_lines(ledger.lines.clone()).unwrap();
    }
}
