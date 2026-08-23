// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The thin executor, second Main. Since P1.01 the loop itself lives in
//! `runtime::run`: this file supplies the simulated world — a counter
//! clock, scripted interruptions, the checkpoint net and the tool bench —
//! and the city runs the same driver against real ones.
//!
//! Event order (no cancel, natural conclusion):
//! checkpoint_committed, run_started, then per turn prompt_assembled,
//! model_called, model_returned, (tool_called, tool_result)*, and finally
//! handoff_written, run_frozen.

use std::cell::Cell;

use kernel::{
    Address, ApprovalId, AxCode, AxError, B3Hash, BuildingPolicy, FrozenConfig, GateContext,
    IdemKey, Locator, Payload, RunId, Seq, Temporal, TimeMs,
};
use memory::Checkpoint;
use runtime::clock::StampGate;
use runtime::handoff::Handoff;
use runtime::pipeline;
use runtime::prefix::{FrozenPrefix, FrozenSegment, SegmentSlot};
use runtime::run::{RunHooks, RunPlan, SafePoint, drive};
use runtime::turn::{BenchOutcome, CallShape, Interrupt, ToolBench};
use serde_json::{Map, Value};

use crate::mem_ledger::MemLedger;
use crate::script_model::ScriptModel;

/// Where the cancel arrives, in boundary vocabulary. Turns count from 0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CancelPoint {
    BeforeAssemble { turn: u32 },
    BeforeCall { turn: u32 },
    BeforeWave { turn: u32 },
}

pub struct Scenario {
    pub run: RunId,
    pub who: String,
    pub addr: Address,
    pub task: String,
    pub goal: String,
    pub job_md: String,
    pub model: ScriptModel,
    /// The real bench (S3.14): gate routing is the turn layer's, and the
    /// simulator drives the same code the city runs.
    pub bench: ToolBench,
    /// Frozen configuration, evaluated once per run: the stamp gate reads
    /// its granularity and zone ladder from here, never from a sample.
    pub config: FrozenConfig,
    /// The checkpoint net, when the scenario runs against a real tree.
    /// A14's leading half is per wave, not per suspicious command: the
    /// fence goes up before every wave, so anything a wave deletes has a
    /// commit to come back from. An unchanged wave still commits — a
    /// chain that rebuilds is worth more than a saved object.
    pub checkpoint: Option<(Checkpoint, String)>,
    pub cancel: Option<CancelPoint>,
    /// A human Steer, delivered at the wave boundary of the given turn.
    /// It appends to the next result and does not interrupt the action in
    /// flight, so the scenario asserts the loop carries on rather than
    /// that it stops.
    pub steer: Option<(u32, String)>,
    pub budget_turns: u32,
}

pub struct ScenarioReport {
    pub lines: Vec<Vec<u8>>,
    pub completion: &'static str,
}

fn payload(map: Map<String, Value>) -> Result<Payload, AxError> {
    Payload::new(map)
}

/// The JOB.md pin: a deterministic fake git oid derived from the content
/// hash — the simulated adapter's whole job is to forge the outside world
/// while keeping the event shapes identical to the real city's.
fn job_locator(addr: &Address, job_md: &str) -> Result<Locator, AxError> {
    let digest = B3Hash::digest(job_md.as_bytes()).to_string();
    let oid: String = digest.chars().take(40).collect();
    Locator::parse(&format!("file:{}@{}", addr.as_str(), oid))
}

/// The scenario's answer at a safe point. Cancel wins over Steer at the
/// same boundary: stopping and redirecting are contradictory instructions,
/// and stopping is the one that cannot be undone by waiting.
fn answer_at(
    point: SafePoint,
    cancel: Option<CancelPoint>,
    steer: Option<&(u32, String)>,
) -> Interrupt {
    let (here, index) = match point {
        SafePoint::BeforeAssemble { turn } => (CancelPoint::BeforeAssemble { turn }, turn),
        SafePoint::BeforeCall { turn } => (CancelPoint::BeforeCall { turn }, turn),
        SafePoint::BeforeWave { turn } => (CancelPoint::BeforeWave { turn }, turn),
        _ => return Interrupt::None,
    };
    if cancel == Some(here) {
        return Interrupt::Cancel;
    }
    let wave = matches!(point, SafePoint::BeforeWave { .. });
    match steer {
        Some((at, text)) if wave && *at == index => Interrupt::Steer {
            source: "user".to_owned(),
            text: text.clone(),
        },
        _ => Interrupt::None,
    }
}

fn clock_overflow() -> AxError {
    AxError::failure(
        AxCode::InvalidArgs,
        "advance scenario clock",
        "u64 overflow",
    )
}

/// Runs one scenario to its frozen end. Every path out of here has passed
/// through handoff_written + run_frozen; the report names which ending.
pub fn run_scenario(scenario: Scenario) -> Result<ScenarioReport, AxError> {
    let Scenario {
        run,
        who,
        addr,
        task,
        goal,
        job_md,
        mut model,
        mut bench,
        config,
        mut checkpoint,
        cancel,
        steer,
        budget_turns,
    } = scenario;
    let mut ledger = MemLedger::new();
    let mut stamps = StampGate::new(config.clock_stamp);
    let job = job_locator(&addr, &job_md)?;

    // The scenario clock: ticks are handed out in the order the driver
    // asks for them, which is what makes a failure reproducible from a
    // seed rather than from a stopwatch.
    let tick = Cell::new(0u64);
    let mut now = || {
        let value = tick.get();
        tick.set(value.checked_add(1).ok_or_else(clock_overflow)?);
        Ok(TimeMs::new(value))
    };
    let mut interrupt = |point: SafePoint| answer_at(point, cancel, steer.as_ref());

    let handoff = Handoff::new(
        vec![job.clone()],
        "scenario run".to_owned(),
        "see roadmap".to_owned(),
        "scripted world".to_owned(),
        "resume from the job file".to_owned(),
    )?;

    let prefix = FrozenPrefix::assemble(
        FrozenSegment::new(SegmentSlot::City, b"citysim city segment".to_vec()),
        FrozenSegment::new(SegmentSlot::Building, addr.as_str().as_bytes().to_vec()),
        FrozenSegment::new(SegmentSlot::Resident, who.as_bytes().to_vec()),
        FrozenSegment::new(SegmentSlot::Run, job.to_string().into_bytes()),
    )?;

    let plan = RunPlan {
        run,
        who: who.clone(),
        addr: addr.clone(),
        task,
        goal,
        opening: runtime::Opening::FromJob,
        job,
        budget_turns,
        shape: CallShape {
            model: "script".to_owned(),
            max_tokens: 4096,
            effort: None,
        },
        prefix,
        policy: BuildingPolicy::default(),
        tools: Vec::new(),
    };

    let bench_who = who.clone();
    let mut invoke = |call: &kernel::ToolCall, t: TimeMs| {
        // Every door the call must pass is the bench's to route; the
        // simulator stays thin (Handoff verdict 10).
        let key = IdemKey::derive(&run, Seq::new(t.value()), call.name.as_str().as_bytes());
        let ctx = GateContext {
            actor: bench_who.clone(),
            now: t,
            item_id: ApprovalId::new(format!("item-{}", t.value())).ok_or_else(|| {
                AxError::failure(AxCode::InvalidArgs, "mint approval id", "empty id")
            })?,
        };
        let temporal = bench
            .meta_of(call.name.as_str())
            .map_or(Temporal::Timeless, |meta| meta.temporal);
        match bench.invoke(call, &key, &ctx)? {
            BenchOutcome::Ran { outcome, .. } => {
                // The envelope is the caller's to hang: a clock line when
                // one is due, inside this result's byte budget.
                let bytes = serde_json::to_vec(&outcome.result).map_err(|err| {
                    AxError::failure(AxCode::InvalidArgs, "encode tool result", err.to_string())
                })?;
                let stamp = stamps.observe(t, temporal, &config.clock_zones)?;
                let packaged = pipeline::package(
                    &bytes,
                    pipeline::PackContext {
                        cap_bytes: 16_384,
                        stamp,
                        net_notice: false,
                        steer: None,
                        offload: None,
                    },
                )?;
                let mut wrapped = Map::new();
                wrapped.insert("content".to_owned(), Value::String(packaged.content));
                Ok(kernel::ToolOutcome {
                    result: payload(wrapped)?,
                })
            }
            // A refusal reaches the model as this call's error, which the
            // turn records as a tool_result: boundary feedback, not a dead
            // turn.
            BenchOutcome::Refused { refusal } => Err(*refusal),
            BenchOutcome::Pending { item } => Err(AxError::failure(
                AxCode::ApprovalPending,
                "await approval",
                item.id.as_str().to_owned(),
            )),
            BenchOutcome::Duplicate => Err(AxError::failure(
                AxCode::InvalidArgs,
                "invoke tool",
                "this call was already made",
            )),
            // An outcome this simulator does not know is refused rather
            // than guessed: a new bench verdict must be taught here before
            // it can pass.
            _ => Err(AxError::failure(
                AxCode::InvalidArgs,
                "invoke tool",
                "the bench returned an outcome this scenario does not handle",
            )),
        }
    };

    let frozen = match checkpoint.as_mut() {
        Some((net, scope)) => {
            let fence_who = who.clone();
            let mut fence = |t: TimeMs| {
                net.wave_pre(scope, t, &fence_who)
                    .map_err(memory::MemoryError::into_ax)
            };
            let mut hooks = RunHooks {
                now: &mut now,
                interrupt: &mut interrupt,
                fence: Some(&mut fence),
                invoke: &mut invoke,
            };
            drive(plan, &mut ledger, &mut model, &mut hooks, &handoff)?
        }
        None => {
            let mut hooks = RunHooks {
                now: &mut now,
                interrupt: &mut interrupt,
                fence: None,
                invoke: &mut invoke,
            };
            drive(plan, &mut ledger, &mut model, &mut hooks, &handoff)?
        }
    };

    Ok(ScenarioReport {
        lines: ledger.raw_lines().to_vec(),
        completion: frozen.completion().name(),
    })
}
