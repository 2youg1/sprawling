// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The run driver owns one sequence: dispatch, turns, freeze. These tests
//! pin that sequence and the three ways a run can end, so a later change
//! to the loop has to argue with the event order rather than with prose.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

use kernel::{
    Address, AxError, B3Hash, BuildingPolicy, Completion, ContentBlock, EventDraft, EventRef,
    GENESIS_PREV, Ledger, Locator, Model, ModelRequest, ModelReturn, Payload, RunId, TimeMs,
    ToolCall, ToolName, ToolOutcome, chain_hash, message_payload,
};
use runtime::handoff::Handoff;
use runtime::prefix::{FrozenPrefix, FrozenSegment, SegmentSlot};
use runtime::run::{Advance, Run, RunHooks, RunPlan, SafePoint, drive};
use runtime::turn::{CallShape, Interrupt};

struct RecordingLedger {
    lines: Vec<Vec<u8>>,
    next: kernel::Seq,
    prev: B3Hash,
}

impl RecordingLedger {
    fn new() -> Self {
        RecordingLedger {
            lines: Vec::new(),
            next: kernel::Seq::FIRST,
            prev: GENESIS_PREV,
        }
    }

    fn field(&self, key: &str) -> Vec<String> {
        self.lines
            .iter()
            .map(|line| {
                let value: serde_json::Value = serde_json::from_slice(line).unwrap();
                value[key].to_string().trim_matches('"').to_owned()
            })
            .collect()
    }

    fn kinds(&self) -> Vec<String> {
        self.field("kind")
    }

    fn stamps(&self) -> Vec<u64> {
        self.lines
            .iter()
            .map(|line| {
                let value: serde_json::Value = serde_json::from_slice(line).unwrap();
                value["t"].as_u64().unwrap()
            })
            .collect()
    }
}

impl Ledger for RecordingLedger {
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

/// One tool call on the first turn, nothing after: the shortest script
/// that still exercises a wave.
struct ScriptedModel {
    waves: Vec<Vec<ToolCall>>,
    /// Every user message this model was shown, in order, so a test can
    /// ask what actually reached the window rather than what was
    /// recorded about it.
    seen: std::rc::Rc<std::cell::RefCell<Vec<String>>>,
}

impl Model for ScriptedModel {
    fn call(&mut self, req: &ModelRequest) -> Result<ModelReturn, AxError> {
        self.seen
            .borrow_mut()
            .push(format!("{:?}", req.chat.messages));
        let calls = if self.waves.is_empty() {
            Vec::new()
        } else {
            self.waves.remove(0)
        };
        Ok(ModelReturn::bare(
            message_payload(&[ContentBlock::Text {
                text: "working".to_owned(),
            }])
            .unwrap(),
            calls,
        ))
    }
}

fn call(id: &str) -> ToolCall {
    ToolCall {
        id: id.to_owned(),
        name: ToolName::parse("status").unwrap(),
        args: Payload::empty(),
    }
}

fn plan(budget_turns: u32) -> RunPlan {
    let addr = Address::parse("lab/room1").unwrap();
    RunPlan {
        run: RunId::from_bytes([7; 16]),
        who: "resident".to_owned(),
        addr: addr.clone(),
        task: "close the loop".to_owned(),
        goal: "one turn, then stop".to_owned(),
        opening: runtime::Opening::FromJob,
        job: Locator::parse(&format!("file:{}/JOB.md@{}", addr.as_str(), "a".repeat(40))).unwrap(),
        budget_turns,
        shape: CallShape {
            model: "script".to_owned(),
            max_tokens: 4096,
            effort: None,
        },
        prefix: FrozenPrefix::assemble(
            FrozenSegment::new(SegmentSlot::City, b"city".to_vec()),
            FrozenSegment::new(SegmentSlot::Building, b"building".to_vec()),
            FrozenSegment::new(SegmentSlot::Resident, b"resident".to_vec()),
            FrozenSegment::new(SegmentSlot::Run, b"run".to_vec()),
        )
        .unwrap(),
        policy: BuildingPolicy::default(),
        tools: Vec::new(),
    }
}

fn handoff() -> Handoff {
    Handoff::new(
        vec![Locator::parse(&format!("file:lab/room1/JOB.md@{}", "a".repeat(40))).unwrap()],
        "driver test".to_owned(),
        "see roadmap".to_owned(),
        "scripted world".to_owned(),
        "resume from the job file".to_owned(),
    )
    .unwrap()
}

/// A counter clock: the same closure citysim uses, so the assertions here
/// and the simulator's byte fixtures are talking about one discipline.
fn counter() -> impl FnMut() -> Result<TimeMs, AxError> {
    let mut tick: u64 = 0;
    move || {
        let now = TimeMs::new(tick);
        tick = tick.saturating_add(1);
        Ok(now)
    }
}

#[test]
fn a_run_that_finishes_writes_dispatch_turns_and_freeze_in_that_order() {
    let mut ledger = RecordingLedger::new();
    let mut model = ScriptedModel {
        seen: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
        waves: vec![vec![call("t-1")]],
    };
    let mut now = counter();
    let mut interrupt = |_: SafePoint| Interrupt::None;
    let mut invoke = |_: &ToolCall, _: TimeMs| {
        Ok(ToolOutcome {
            result: Payload::empty(),
        })
    };
    let mut hooks = RunHooks {
        now: &mut now,
        interrupt: &mut interrupt,
        fence: None,
        invoke: &mut invoke,
    };

    let frozen = drive(plan(4), &mut ledger, &mut model, &mut hooks, &handoff()).unwrap();

    assert_eq!(
        ledger.kinds(),
        vec![
            "checkpoint_committed",
            "run_started",
            "prompt_assembled",
            "model_called",
            "model_returned",
            "tool_called",
            "tool_result",
            "prompt_assembled",
            "model_called",
            "model_returned",
            "handoff_written",
            "run_frozen",
        ]
    );
    assert!(matches!(frozen.completion(), Completion::Done(_)));
    assert_eq!(frozen.turns(), 2);
    // Dispatch takes two stamps, each turn one, and the freeze one more
    // with run_frozen derived from it: two ledger lines, one event.
    let stamps = ledger.stamps();
    assert_eq!(stamps[0], 0);
    assert_eq!(stamps[1], 1);
    assert_eq!(stamps[2], 2);
    assert_eq!(stamps[7], 3);
    assert_eq!(stamps[10], 4);
    assert_eq!(stamps[11], 5);
}

#[test]
fn a_budget_that_runs_out_freezes_at_limit_rather_than_running_on() {
    let mut ledger = RecordingLedger::new();
    let mut model = ScriptedModel {
        seen: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
        waves: vec![vec![call("t-1")], vec![call("t-2")]],
    };
    let mut now = counter();
    let mut interrupt = |_: SafePoint| Interrupt::None;
    let mut invoke = |_: &ToolCall, _: TimeMs| {
        Ok(ToolOutcome {
            result: Payload::empty(),
        })
    };
    let mut hooks = RunHooks {
        now: &mut now,
        interrupt: &mut interrupt,
        fence: None,
        invoke: &mut invoke,
    };

    let frozen = drive(plan(2), &mut ledger, &mut model, &mut hooks, &handoff()).unwrap();

    assert!(matches!(frozen.completion(), Completion::Limit));
    assert_eq!(frozen.turns(), 2);
    let kinds = ledger.kinds();
    assert_eq!(kinds[kinds.len() - 2], "handoff_written");
    assert_eq!(kinds[kinds.len() - 1], "run_frozen");
}

#[test]
fn a_cancel_at_a_safe_point_freezes_inside_the_interrupted_turn() {
    let mut ledger = RecordingLedger::new();
    let mut model = ScriptedModel {
        seen: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
        waves: vec![vec![call("t-1")]],
    };
    let mut now = counter();
    let mut interrupt = |point: SafePoint| match point {
        SafePoint::BeforeCall { turn: 0 } => Interrupt::Cancel,
        _ => Interrupt::None,
    };
    let mut invoke = |_: &ToolCall, _: TimeMs| {
        Ok(ToolOutcome {
            result: Payload::empty(),
        })
    };
    let mut hooks = RunHooks {
        now: &mut now,
        interrupt: &mut interrupt,
        fence: None,
        invoke: &mut invoke,
    };

    let frozen = drive(plan(4), &mut ledger, &mut model, &mut hooks, &handoff()).unwrap();

    assert!(matches!(frozen.completion(), Completion::Cancelled));
    let kinds = ledger.kinds();
    assert_eq!(kinds[kinds.len() - 3], "cancel_received");
    assert_eq!(kinds[kinds.len() - 2], "handoff_written");
    assert_eq!(kinds[kinds.len() - 1], "run_frozen");
    // The freeze belongs to the interrupted turn, so it carries that
    // turn's stamp rather than sampling a fresh one.
    let stamps = ledger.stamps();
    assert_eq!(stamps[2], 2);
    assert_eq!(stamps[stamps.len() - 2], 2);
    assert_eq!(stamps[stamps.len() - 1], 3);
}

#[test]
fn a_fence_runs_before_the_wave_and_carries_the_turns_stamp() {
    let mut ledger = RecordingLedger::new();
    let mut model = ScriptedModel {
        seen: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
        waves: vec![vec![call("t-1")]],
    };
    let mut now = counter();
    let mut interrupt = |_: SafePoint| Interrupt::None;
    let mut invoke = |_: &ToolCall, _: TimeMs| {
        Ok(ToolOutcome {
            result: Payload::empty(),
        })
    };
    let mut fenced: Vec<u64> = Vec::new();
    let mut fence = |t: TimeMs| {
        fenced.push(t.value());
        Ok(Payload::empty())
    };
    {
        let mut hooks = RunHooks {
            now: &mut now,
            interrupt: &mut interrupt,
            fence: Some(&mut fence),
            invoke: &mut invoke,
        };
        let frozen = drive(plan(4), &mut ledger, &mut model, &mut hooks, &handoff()).unwrap();
        assert!(matches!(frozen.completion(), Completion::Done(_)));
    }
    // The fence goes up before *every* wave, including the last turn's
    // empty one: an unchanged wave still commits, because a chain that
    // rebuilds is worth more than a saved object.
    assert_eq!(fenced, vec![2, 3]);
    let kinds = ledger.kinds();
    assert_eq!(kinds[5], "checkpoint_committed");
    assert_eq!(kinds[6], "tool_called");
}

#[test]
fn advance_reports_each_turn_so_a_caller_can_stop_between_them() {
    let mut ledger = RecordingLedger::new();
    let mut model = ScriptedModel {
        seen: std::rc::Rc::new(std::cell::RefCell::new(Vec::new())),
        waves: vec![vec![call("t-1")]],
    };
    let mut now = counter();
    let mut interrupt = |_: SafePoint| Interrupt::None;
    let mut invoke = |_: &ToolCall, _: TimeMs| {
        Ok(ToolOutcome {
            result: Payload::empty(),
        })
    };
    let mut hooks = RunHooks {
        now: &mut now,
        interrupt: &mut interrupt,
        fence: None,
        invoke: &mut invoke,
    };

    let mut run = Run::dispatch(plan(4), &mut ledger, &mut hooks).unwrap();
    assert!(matches!(
        run.advance(&mut ledger, &mut model, &mut hooks).unwrap(),
        Advance::Turned
    ));
    let ending = run.advance(&mut ledger, &mut model, &mut hooks).unwrap();
    let Advance::Concluded(completion) = ending else {
        panic!("the second turn makes no calls, so the run concludes");
    };
    assert!(matches!(completion, Completion::Done(_)));
    let frozen = run
        .freeze(&mut ledger, &handoff(), completion, &mut hooks)
        .unwrap();
    assert_eq!(frozen.turns(), 2);
}

#[test]
fn a_steer_at_a_safe_point_reaches_the_next_window_and_not_only_the_ledger() {
    let mut ledger = RecordingLedger::new();
    let seen = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let mut model = ScriptedModel {
        seen: std::rc::Rc::clone(&seen),
        waves: vec![vec![call("t-1")]],
    };
    let mut now = counter();
    // One steer, at the boundary before the second assembly.
    let mut interrupt = |point: SafePoint| match point {
        SafePoint::BeforeAssemble { turn: 1 } => Interrupt::Steer {
            source: "user".to_owned(),
            text: "measure it in metres".to_owned(),
        },
        _ => Interrupt::None,
    };
    let mut invoke = |_: &ToolCall, _: TimeMs| {
        Ok(ToolOutcome {
            result: Payload::empty(),
        })
    };
    let mut hooks = RunHooks {
        now: &mut now,
        interrupt: &mut interrupt,
        fence: None,
        invoke: &mut invoke,
    };

    drive(plan(4), &mut ledger, &mut model, &mut hooks, &handoff()).unwrap();

    assert!(
        ledger.kinds().contains(&"steer_received".to_owned()),
        "the arrival is recorded"
    );
    let windows = seen.borrow();
    assert_eq!(windows.len(), 2, "two turns, two windows");
    assert!(
        !windows[0].contains("measure it in metres"),
        "a steer never rewrites a request already assembled"
    );
    assert!(
        windows[1].contains("measure it in metres"),
        "and the next assembly carries it: {}",
        windows[1]
    );
    assert!(windows[1].contains("user"), "attributed to whoever sent it");
}
