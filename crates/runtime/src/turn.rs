// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

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

use std::collections::{BTreeMap, BTreeSet};

use kernel::{
    Address, ApprovalItem, AxCode, AxError, B3Hash, BuildingPolicy, ChatMessage, ChatRequest,
    ContentBlock, DedupVerdict, DiscardForecast, Effect, EgressOutcome, EgressTarget, EventDraft,
    EventKind, EventRef, GateContext, GateOutcome, IdemKey, Ledger, Locator, Model, ModelRequest,
    ModelReturn, Payload, Role, RunId, TaintSet, TimeMs, Tool, ToolCall, ToolDef, ToolOutcome,
    WriteDomain, content_from_message,
};
use serde_json::{Map, Value};

use memory::Checkpoint;

use crate::prefix::FrozenPrefix;

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

/// How the first user message opens.
///
/// Exhaustive, and the choice is made once by the city that wrote (or
/// did not write) the job file. It is not a formatting preference: a
/// session working from an assignment and a session talking with the
/// person want different first words, and inferring which from an empty
/// string would make the emptiness of a goal mean two things.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Opening {
    /// Somebody wrote the task down; the job file's text is in the prefix.
    FromJob,
    /// Nobody did; the person is on the other side of this message.
    WithPerson,
}

/// The run's conversation history, owned by the executor and folded
/// forward turn by turn. Frozen-prefix bytes never live here — the
/// window is the volatile half of the request.
#[derive(Debug, Clone, Default)]
pub struct Window {
    messages: Vec<ChatMessage>,
}

impl Window {
    pub fn new() -> Window {
        Window::default()
    }

    /// The dispatch lines: deterministic from `run_started`'s recorded
    /// inputs, hence rebuildable.
    ///
    /// No pointer to the job file. Its text is the run segment of the
    /// frozen prefix, so a line sending the agent to fetch what it has
    /// already been handed costs a turn and buys nothing; the content
    /// hash that line used to carry is recorded twice in the ledger,
    /// which is where provenance belongs.
    pub fn push_task_lines(&mut self, task: &str, goal: &str, opening: Opening) {
        self.push_user_text(match opening {
            Opening::FromJob => format!("Task: {task}\nGoal: {goal}"),
            // The person's own line, unwrapped. A conversational turn
            // dressed in field labels reads as a form, and a form is
            // answered with a form.
            Opening::WithPerson => task.to_owned(),
        });
    }

    /// Steer joins the tail of the last user message, or opens one if none is open.
    pub fn push_steer(&mut self, source: &str, text: &str) {
        self.push_user_text(format!("{source}: {text}"));
    }

    pub fn push_assistant(&mut self, content: Vec<ContentBlock>) {
        if !content.is_empty() {
            self.messages.push(ChatMessage {
                role: Role::Assistant,
                content,
            });
        }
    }

    /// Tool results open the next user message.
    pub fn push_tool_results(&mut self, results: Vec<ContentBlock>) {
        if results.is_empty() {
            return;
        }
        match self.messages.last_mut() {
            Some(last) if last.role == Role::User => last.content.extend(results),
            _ => self.messages.push(ChatMessage {
                role: Role::User,
                content: results,
            }),
        }
    }

    pub fn messages(&self) -> &[ChatMessage] {
        &self.messages
    }

    fn push_user_text(&mut self, text: String) {
        let block = ContentBlock::Text { text };
        match self.messages.last_mut() {
            Some(last) if last.role == Role::User => last.content.push(block),
            _ => self.messages.push(ChatMessage {
                role: Role::User,
                content: vec![block],
            }),
        }
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
    pub fn call(
        mut self,
        interrupt: Interrupt,
        ledger: &mut dyn Ledger,
        model: &mut dyn Model,
        policy: &BuildingPolicy,
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
        let returned_value = model.call(&request)?;
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

/// The tool bench: the turn layer's routing of a call through the gate
/// its own declared Effect names (Handoff verdict 10 — gate routing is
/// turn-layer work, so the executor stays thin).
///
/// Three orderings are load-bearing. Dedup runs before any side effect,
/// so a replayed call cannot bill or write twice. `exec` is forecast for
/// discards before the Write door, because "this command deletes things"
/// is a stronger claim than "this command writes somewhere" and deserves
/// the stricter door. And a Deny comes back as a `tool_result` carrying
/// the refusal rather than ending the turn: the model that asked for
/// something it may not have should learn that, and continue.
pub struct ToolBench {
    tools: BTreeMap<String, Box<dyn Tool>>,
    domain: WriteDomain,
    taint: TaintSet,
    seen: BTreeSet<IdemKey>,
    prior_public_egress: bool,
    /// The checkpoint net. A command the forecast suspects of deleting
    /// things does not get refused — text prediction is obfuscatable, so
    /// refusing on a substring would be security theatre that also
    /// blocks honest work. It gets fenced instead: commit first, then
    /// run, so whatever it deletes is restorable. Absent a net, such a
    /// command is refused, because running it unprotected is the one
    /// outcome nobody chose.
    checkpoint: Option<Checkpoint>,
    scope: String,
    /// Cluster keys the person has already allowed. Held rather than
    /// looked up: the bench runs inside a drive that owns the ledger,
    /// and a gate that read history mid-wave would be a second reader
    /// of the thing the driver is writing.
    granted: Vec<kernel::ClusterKey>,
    /// What this run was given to do, as the approvals list refers to
    /// it. An item that named no artifact would leave a person deciding
    /// about a spawn with nothing to open.
    job: Option<Locator>,
    /// Where this run works, which is what a delegation approval
    /// clusters by: the person is asked whether this resident may hand
    /// work down, once.
    asking: Option<Address>,
}

/// What the bench decided, alongside what the tool produced.
#[non_exhaustive]
#[derive(Debug)]
pub enum BenchOutcome {
    /// The tool ran; this is its result. `fenced` carries the commit
    /// the wave was fenced against when the forecast suspected a
    /// discard, so the post-wave sweep knows what to restore from.
    Ran {
        outcome: ToolOutcome,
        fenced: Option<String>,
    },
    /// A gate refused. The refusal travels back as a tool_result, which
    /// keeps the turn alive and tells the model what it may not do.
    Refused { refusal: Box<AxError> },
    /// A gate wants a human. S3 has no answering face, so the caller
    /// sees the pending item's code and the run parks.
    Pending { item: Box<ApprovalItem> },
    /// The call was already made. Its earlier result stands.
    Duplicate,
}

impl ToolBench {
    pub fn new(domain: WriteDomain) -> ToolBench {
        ToolBench {
            tools: BTreeMap::new(),
            domain,
            taint: TaintSet::empty(),
            seen: BTreeSet::new(),
            prior_public_egress: false,
            checkpoint: None,
            scope: String::new(),
            granted: Vec::new(),
            job: None,
            asking: None,
        }
    }

    /// Hands the bench the work it serves: where the run stands and what
    /// it was given to do.
    ///
    /// Without it a spawn is refused rather than allowed, because an
    /// approval item that named neither the asker nor an artifact would
    /// reach a person as a question about nothing.
    #[must_use]
    pub fn for_job(mut self, asking: Address, job: Locator) -> ToolBench {
        self.asking = Some(asking);
        self.job = Some(job);
        self
    }

    /// Records that this cluster has already been allowed.
    ///
    /// The caller folds these from the ledger's answers, so a resumed
    /// run does not stop at the door the person just opened.
    pub fn grant(&mut self, cluster: kernel::ClusterKey) {
        self.granted.push(cluster);
    }

    /// Hands the bench its checkpoint net. Without one, a suspected
    /// discard is refused rather than run unprotected.
    pub fn with_checkpoint(mut self, checkpoint: Checkpoint, scope: &str) -> ToolBench {
        self.checkpoint = Some(checkpoint);
        self.scope = scope.to_owned();
        self
    }

    /// Registers a tool under its own declared name. A second tool
    /// claiming a taken name is refused rather than shadowing the first.
    pub fn register(&mut self, tool: Box<dyn Tool>) -> Result<(), AxError> {
        let name = tool.meta().name.as_str().to_owned();
        if self.tools.contains_key(&name) {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "register tool",
                format!("`{name}` is already registered"),
            ));
        }
        self.tools.insert(name, tool);
        Ok(())
    }

    pub fn taint_mut(&mut self) -> &mut TaintSet {
        &mut self.taint
    }

    /// The registered tool's declaration. Callers packaging a result
    /// need its `temporal` to decide whether a clock line is due.
    pub fn meta_of(&self, name: &str) -> Option<&kernel::ToolMeta> {
        self.tools.get(name).map(|tool| tool.meta())
    }

    /// Routes one call: dedup, then the door its Effect names, then the
    /// tool itself.
    pub fn invoke(
        &mut self,
        call: &ToolCall,
        key: &IdemKey,
        ctx: &GateContext,
    ) -> Result<BenchOutcome, AxError> {
        // Before any unreplayable effect (8.2).
        if kernel::dedup(&self.seen, key) == DedupVerdict::Duplicate {
            return Ok(BenchOutcome::Duplicate);
        }
        let name = call.name.as_str().to_owned();
        let Some(tool) = self.tools.get(&name) else {
            return Err(AxError::failure(
                AxCode::ToolUnavailable,
                "invoke tool",
                format!("no tool named `{name}` is registered"),
            )
            .with_recovery("call one of the tools listed in your catalog"));
        };
        let effect = tool.meta().effect.clone();

        // exec is forecast first. A hit does not refuse: it fences.
        let mut fenced = None;
        if name == "exec"
            && let Ok(arm) = crate::tools::parse_arm(call.args.as_map())
            && let DiscardForecast::Suspected { pattern } = kernel::forecast(&arm)
        {
            let Some(checkpoint) = self.checkpoint.as_mut() else {
                return Err(AxError::failure(
                    AxCode::ToolUnavailable,
                    "invoke tool",
                    format!("`{pattern}` may discard files and no checkpoint net is configured"),
                )
                .with_recovery(
                    "configure the checkpoint net, or run a command that does not delete",
                ));
            };
            let payload = checkpoint
                .wave_pre(&self.scope, ctx.now, &ctx.actor)
                .map_err(kernel_error_from_memory)?;
            fenced = payload
                .as_map()
                .get("oid")
                .and_then(Value::as_str)
                .map(str::to_owned);
        }

        match &effect {
            Effect::Read => {}
            Effect::Write { domain: target } => {
                match kernel::domain(&self.domain, target, &self.taint) {
                    GateOutcome::Allow => {}
                    GateOutcome::Deny { refusal } => {
                        return Ok(BenchOutcome::Refused { refusal });
                    }
                    GateOutcome::Escalate { item } => {
                        // An answer the person already gave is not asked
                        // again. The grant is per cluster, which is the
                        // unit the person answered in: they were shown a
                        // group and said yes to that group, not to one
                        // call inside it.
                        if !self.granted.contains(&item.cluster_key) {
                            return Ok(BenchOutcome::Pending {
                                item: Box::new(item),
                            });
                        }
                    }
                }
            }
            Effect::Connector { label } => {
                // Same door, same scan; only the target differs. A
                // connector's destination is its registration's, so
                // there is nothing for the call to name and nothing for
                // a model to get wrong.
                let bytes = serde_json::to_vec(&call.args).map_err(|err| {
                    AxError::failure(AxCode::InvalidArgs, "scan connector args", err.to_string())
                })?;
                let spans = kernel::scan(&bytes);
                match kernel::egress(
                    &spans,
                    &EgressTarget::Connector {
                        label: label.clone(),
                    },
                    self.prior_public_egress,
                ) {
                    EgressOutcome::Allow {
                        first_public_egress,
                    } => {
                        if first_public_egress {
                            self.prior_public_egress = true;
                        }
                    }
                    EgressOutcome::Deny { refusal } => {
                        return Ok(BenchOutcome::Refused { refusal });
                    }
                }
            }
            Effect::Egress => {
                let bytes = serde_json::to_vec(&call.args).map_err(|err| {
                    AxError::failure(AxCode::InvalidArgs, "scan egress args", err.to_string())
                })?;
                let spans = kernel::scan(&bytes);
                // The target is the tool's to declare; a call that does
                // not say where it is sending cannot be judged, and an
                // unjudged egress is the one thing the door exists for.
                let host = call
                    .args
                    .as_map()
                    .get("host")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        AxError::failure(
                            AxCode::InvalidArgs,
                            "invoke tool",
                            format!("`{name}` declares Egress but named no host"),
                        )
                    })?
                    .to_owned();
                match kernel::egress(
                    &spans,
                    &EgressTarget::Public { host },
                    self.prior_public_egress,
                ) {
                    EgressOutcome::Allow {
                        first_public_egress,
                    } => {
                        if first_public_egress {
                            self.prior_public_egress = true;
                        }
                    }
                    EgressOutcome::Deny { refusal } => {
                        return Ok(BenchOutcome::Refused { refusal });
                    }
                }
            }
            Effect::Spawn => {
                let (Some(asking), Some(job)) = (self.asking.as_ref(), self.job.as_ref()) else {
                    return Err(AxError::failure(
                        AxCode::ToolUnavailable,
                        "invoke tool",
                        format!("`{name}` declares Spawn and this bench was built without a job"),
                    )
                    .with_recovery(
                        "build the bench with `for_job`; a spawn a person cannot be asked about \
                         is a spawn nobody allowed",
                    ));
                };
                // The room is the tool's own argument, so the person is
                // told where the work is going without this layer
                // learning the tool's schema: an unreadable room reads
                // as the asking address, and the item still names a real
                // place.
                let room = call
                    .args
                    .as_map()
                    .get("room")
                    .and_then(Value::as_str)
                    .and_then(|raw| Address::parse(raw).ok())
                    .unwrap_or_else(|| asking.clone());
                match kernel::delegation(ctx, asking, &room, job, &self.taint) {
                    GateOutcome::Allow => {}
                    GateOutcome::Deny { refusal } => {
                        return Ok(BenchOutcome::Refused { refusal });
                    }
                    GateOutcome::Escalate { item } => {
                        if !self.granted.contains(&item.cluster_key) {
                            return Ok(BenchOutcome::Pending {
                                item: Box::new(item),
                            });
                        }
                    }
                }
            }
            Effect::Spend => {
                // No Spend tool instance exists until the egress proxy
                // lands (P1); the door is wired so the first one meets it.
                return Err(AxError::failure(
                    AxCode::ToolUnavailable,
                    "invoke tool",
                    format!("`{name}` declares Spend, which has no instance before P1"),
                ));
            }
            _ => {
                return Err(AxError::failure(
                    AxCode::InvalidArgs,
                    "invoke tool",
                    format!("`{name}` declares an effect this bench does not route"),
                ));
            }
        }

        // The key is recorded once the call is committed to, so a retry
        // after a gate refusal is not treated as a replay.
        self.seen.insert(*key);
        // Re-borrowed here: the forecast fence needed `self` mutably.
        let Some(tool) = self.tools.get_mut(&name) else {
            return Err(AxError::failure(
                AxCode::ToolUnavailable,
                "invoke tool",
                format!("no tool named `{name}` is registered"),
            ));
        };
        let outcome = tool.invoke(call)?;
        Ok(BenchOutcome::Ran { outcome, fenced })
    }
}

/// The memory crate owns its own error root; the turn layer speaks
/// AxError, so the conversion happens once, here.
fn kernel_error_from_memory(err: memory::MemoryError) -> AxError {
    err.into_ax()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]
mod bench_tests {
    use super::*;
    use crate::sandbox::{EchoSandbox, Fuel};
    use crate::tools::{EditTool, ExecTool};
    use kernel::{Address, ApprovalId, Seq};

    fn ctx() -> GateContext {
        GateContext {
            actor: "resident".to_owned(),
            now: TimeMs::new(1_700_000_000_000),
            item_id: ApprovalId::new("item-1").expect("id"),
        }
    }

    fn key(n: u64) -> IdemKey {
        IdemKey::derive(&RunId::from_bytes([1u8; 16]), Seq::new(n), b"action")
    }

    fn bench(root: &std::path::Path) -> ToolBench {
        let domain = WriteDomain::new(vec![Address::parse("work").unwrap()]).unwrap();
        let mut bench = ToolBench::new(domain.clone());
        bench
            .register(Box::new(
                EditTool::new(root, Address::parse("work").unwrap(), domain).unwrap(),
            ))
            .unwrap();
        bench
    }

    fn edit_call(path: &str, base: &str, old: &str, new: &str) -> ToolCall {
        let mut args = Map::new();
        for (k, v) in [
            ("path", path),
            ("base_version", base),
            ("old", old),
            ("new", new),
        ] {
            args.insert(k.to_owned(), Value::String(v.to_owned()));
        }
        ToolCall {
            id: "c1".to_owned(),
            name: kernel::ToolName::parse("edit").unwrap(),
            args: Payload::new(args).unwrap(),
        }
    }

    #[test]
    fn dedup_runs_before_the_side_effect() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("work")).unwrap();
        std::fs::write(tmp.path().join("work/a.txt"), "one\n").unwrap();
        let mut bench = bench(tmp.path());
        let version = crate::tools::version_of(b"one\n");
        let call = edit_call("work/a.txt", &version, "one", "two");

        let first = bench.invoke(&call, &key(1), &ctx()).unwrap();
        assert!(matches!(first, BenchOutcome::Ran { .. }));
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("work/a.txt")).unwrap(),
            "two\n"
        );
        // The same key again: the tool must not run a second time, and
        // the file must not change (the second edit would fail on the
        // stale version anyway — dedup means it is never attempted).
        let second = bench.invoke(&call, &key(1), &ctx()).unwrap();
        assert!(matches!(second, BenchOutcome::Duplicate));
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("work/a.txt")).unwrap(),
            "two\n"
        );
    }

    /// A tool that declares `Spawn` and nothing else. The bench's job
    /// here is the door, not the tool, so the tool does as little as a
    /// tool can.
    struct SpawnTool(kernel::ToolMeta);

    impl SpawnTool {
        fn new() -> SpawnTool {
            SpawnTool(kernel::ToolMeta {
                name: kernel::ToolName::parse("delegate").unwrap(),
                disclosure: "hand work down".to_owned(),
                params: Payload::empty(),
                effect: Effect::Spawn,
                cost_tier: kernel::CostTier::Heavy,
                timeout: None,
                render: kernel::RenderIntent::Generic,
                temporal: kernel::Temporal::Timeless,
            })
        }
    }

    impl Tool for SpawnTool {
        fn meta(&self) -> &kernel::ToolMeta {
            &self.0
        }

        fn invoke(&mut self, _call: &ToolCall) -> Result<ToolOutcome, AxError> {
            Ok(ToolOutcome {
                result: Payload::empty(),
            })
        }
    }

    fn spawn_call() -> ToolCall {
        let mut args = Map::new();
        args.insert("room".to_owned(), Value::String("work/helper".to_owned()));
        ToolCall {
            id: "c1".to_owned(),
            name: kernel::ToolName::parse("delegate").unwrap(),
            args: Payload::new(args).unwrap(),
        }
    }

    fn spawn_bench() -> ToolBench {
        let domain = WriteDomain::new(vec![Address::parse("work").unwrap()]).unwrap();
        let mut bench = ToolBench::new(domain).for_job(
            Address::parse("work/room1").unwrap(),
            Locator::parse(&format!("file:work/room1/JOB.md@{}", "a".repeat(40))).unwrap(),
        );
        bench.register(Box::new(SpawnTool::new())).unwrap();
        bench
    }

    /// City.md told a model not to delegate unless the person allowed
    /// it, and nothing checked. Now the first spawn stops at a door.
    #[test]
    fn a_spawn_waits_for_the_person_and_a_granted_cluster_walks_through() {
        let mut bench = spawn_bench();
        let waiting = bench.invoke(&spawn_call(), &key(1), &ctx()).unwrap();
        let BenchOutcome::Pending { item } = waiting else {
            panic!("the first spawn of a run is the person's to allow");
        };
        assert_eq!(item.cluster_key.class, kernel::ApprovalClass::Delegation);
        assert_eq!(
            item.cluster_key.detail, "work/room1",
            "the cluster is the resident asking, so one answer covers its whole session"
        );
        assert!(item.action_desc.contains("work/helper"), "{item:?}");

        let mut allowed = spawn_bench();
        allowed.grant(item.cluster_key.clone());
        assert!(matches!(
            allowed.invoke(&spawn_call(), &key(1), &ctx()).unwrap(),
            BenchOutcome::Ran { .. }
        ));
    }

    /// Fail-closed: a bench nobody told what work it serves cannot mint
    /// an item a person could answer, so it refuses rather than letting
    /// the spawn through unasked.
    #[test]
    fn a_spawn_on_a_bench_with_no_job_is_refused_rather_than_waved_through() {
        let domain = WriteDomain::new(vec![Address::parse("work").unwrap()]).unwrap();
        let mut bench = ToolBench::new(domain);
        bench.register(Box::new(SpawnTool::new())).unwrap();
        let err = bench.invoke(&spawn_call(), &key(1), &ctx()).unwrap_err();
        assert_eq!(*err.code(), AxCode::ToolUnavailable);
        assert!(err.recovery().contains("for_job"));
    }

    #[test]
    fn a_write_outside_the_domain_flows_back_as_a_refusal_not_a_dead_turn() {
        let tmp = tempfile::tempdir().unwrap();
        let domain = WriteDomain::new(vec![Address::parse("work").unwrap()]).unwrap();
        let elsewhere = WriteDomain::new(vec![Address::parse("elsewhere").unwrap()]).unwrap();
        let mut bench = ToolBench::new(domain);
        // A tool whose declared domain sits outside the run's domain.
        bench
            .register(Box::new(
                EditTool::new(tmp.path(), Address::parse("elsewhere").unwrap(), elsewhere).unwrap(),
            ))
            .unwrap();
        let outcome = bench
            .invoke(&edit_call("elsewhere/x", "v", "a", "b"), &key(2), &ctx())
            .unwrap();
        match outcome {
            BenchOutcome::Refused { refusal } => {
                assert_eq!(*refusal.code(), AxCode::OutsideWriteDomain);
            }
            other => panic!("expected a refusal that keeps the turn alive, got {other:?}"),
        }
    }

    #[test]
    fn a_suspected_discard_without_a_net_is_refused_and_with_one_is_fenced() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("work")).unwrap();
        std::fs::write(tmp.path().join("work/doomed.txt"), "bye").unwrap();
        let domain = WriteDomain::new(vec![Address::parse("work").unwrap()]).unwrap();

        let exec_call = |text: &str| {
            let mut args = Map::new();
            args.insert(
                "arm".to_owned(),
                serde_json::json!({ "shell": { "text": text } }),
            );
            ToolCall {
                id: "e1".to_owned(),
                name: kernel::ToolName::parse("exec").unwrap(),
                args: Payload::new(args).unwrap(),
            }
        };
        let exec_tool = || {
            Box::new(
                ExecTool::new(
                    tmp.path().to_path_buf(),
                    Vec::new(),
                    None,
                    Box::new(EchoSandbox::new()),
                    None,
                    Fuel(1000),
                    Address::parse("work").unwrap(),
                )
                .unwrap(),
            )
        };

        // No net: a command the forecast suspects is refused rather than
        // run unprotected.
        let mut bare = ToolBench::new(domain.clone());
        bare.register(exec_tool()).unwrap();
        let err = match bare.invoke(&exec_call("rm -rf work"), &key(3), &ctx()) {
            Err(err) => err,
            Ok(other) => panic!("expected a refusal, got {other:?}"),
        };
        assert_eq!(*err.code(), AxCode::ToolUnavailable);

        // With a net: the wave is fenced first, and the outcome carries
        // the commit the sweep will restore from.
        let checkpoint = Checkpoint::open(tmp.path()).unwrap();
        let mut fenced_bench = ToolBench::new(domain).with_checkpoint(checkpoint, "work");
        fenced_bench.register(exec_tool()).unwrap();
        // The shell arm is unconfigured, so the tool itself refuses —
        // but only after the fence went up, which is what we assert.
        let _ = fenced_bench.invoke(&exec_call("rm -rf work"), &key(4), &ctx());
        // The fence went up before the command was allowed to run: a
        // repository now exists with a commit to restore from.
        assert!(
            tmp.path().join(".git").exists(),
            "the checkpoint net was raised"
        );
        let mut probe = Checkpoint::open(tmp.path()).unwrap();
        let payload = probe
            .wave_pre("work", TimeMs::new(1_700_000_001_000), "probe")
            .unwrap();
        let oid = serde_json::to_value(&payload).unwrap()["oid"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(!oid.is_empty(), "the fence has a commit to restore from");
    }

    #[test]
    fn an_unregistered_name_is_refused_rather_than_routed_anywhere() {
        let tmp = tempfile::tempdir().unwrap();
        let mut bench = bench(tmp.path());
        let mut call = edit_call("work/a", "v", "a", "b");
        call.name = kernel::ToolName::parse("status").unwrap();
        let err = match bench.invoke(&call, &key(5), &ctx()) {
            Err(err) => err,
            Ok(other) => panic!("expected a refusal, got {other:?}"),
        };
        assert_eq!(*err.code(), AxCode::ToolUnavailable);
    }

    #[test]
    fn a_second_tool_claiming_a_taken_name_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let mut bench = bench(tmp.path());
        let err = match bench.register(Box::new(
            EditTool::new(
                tmp.path(),
                Address::parse("work").unwrap(),
                WriteDomain::new(vec![Address::parse("work").unwrap()]).unwrap(),
            )
            .unwrap(),
        )) {
            Err(err) => err,
            Ok(()) => panic!("a name collision must refuse, not shadow"),
        };
        assert_eq!(*err.code(), AxCode::InvalidArgs);
    }
}
