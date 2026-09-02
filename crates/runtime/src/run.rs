// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The run driver: dispatch, turns, freeze — the single authority for the
//! event sequence a run leaves behind. The real city and citysim call this
//! same code, so the simulator's byte fixtures are evidence about
//! production rather than about a second implementation of the same idea.
//!
//! Time arrives through the `now` hook and is never sampled here. The
//! caller decides what a clock is: a counter in the simulator, the one
//! sanctioned wall-clock sample in `bin::assembly`.

use kernel::{
    Address, AxCode, AxError, BuildingPolicy, Carrier, Completion, EventDraft, EventKind, Evidence,
    Ledger, Locator, Model, Payload, RunId, TimeMs, ToolCall, ToolDef, ToolOutcome,
};
use serde_json::{Map, Value};

use crate::catalog::SkillPin;
use crate::handoff::Handoff;
use crate::prefix::FrozenPrefix;
use crate::turn::{CallShape, Interrupt, PhaseOutcome, Turn};
use crate::window::{Opening, Window};

/// Everything constant about one run. Assembled by the caller, because
/// what a prefix contains and which tools exist are decisions of the city
/// that dispatches, not of the loop that runs.
pub struct RunPlan {
    pub run: RunId,
    pub who: String,
    pub addr: Address,
    pub task: String,
    pub goal: String,
    /// Whether this session was handed a written task or a person.
    /// Decided by the city when it laid the brief down, carried here so
    /// the window and the prefix's run segment cannot disagree about
    /// which situation the agent is in.
    pub opening: Opening,
    pub job: Locator,
    /// The run that handed this work down, when one did. Written into
    /// `run_started` so the tree is a fact anybody folding the ledger
    /// can read, rather than an inference from two lines happening to
    /// be next to each other.
    pub parent: Option<RunId>,
    pub budget_turns: u32,
    /// What this run was allowed to spend. Written into `run_started`
    /// beside `parent` and for the same reason: once the process that
    /// knew it is gone, the ledger is the only thing that can still say
    /// what ceiling a run was sent out under - and something as ordinary
    /// as answering an approval hours later has to send the work on
    /// under that same ceiling.
    pub budget: kernel::BudgetCap,
    pub shape: CallShape,
    pub prefix: FrozenPrefix,
    pub policy: BuildingPolicy,
    pub tools: Vec<ToolDef>,
    /// The skills this run's reading room admitted, and what each one
    /// hashed to when the shelf was read. Written into `run_started`
    /// beside `parent` and the budget, and for the same reason: once
    /// the process that read the shelf is gone, the ledger is the only
    /// thing that can still say which bytes this run was given.
    pub skills: Vec<SkillPin>,
}

/// A steer changes what the model reads next, so the driver folds it into
/// the window it owns; the turn layer records that it arrived. Two halves
/// of one event, each held where its material is: the text belongs to the
/// window, the record belongs to the ledger.
///
/// Whichever safe point it arrives at, the fold takes effect at the next
/// assembly — a steer never rewrites a request already on the wire.
fn fold_steer(window: &mut Window, interrupt: &Interrupt) {
    if let Interrupt::Steer { source, text } = interrupt {
        window.push_steer(source, text);
    }
}

/// Where the driver stops to ask whether anything arrived. The turn layer
/// owns three cancellation-safe points; this enum is how the driver names
/// them to a caller that knows nothing about turn internals.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SafePoint {
    BeforeAssemble {
        turn: u32,
    },
    BeforeCall {
        turn: u32,
    },
    BeforeWave {
        turn: u32,
    },
    /// After the wave, before the run acts on what the turn decided.
    /// The one boundary a run that concluded still passes through, so a
    /// cancel arriving that late stops the work the turn handed down
    /// instead of arriving at a run that has already ended.
    BeforeSpawn {
        turn: u32,
    },
}

/// What one turn did. Exhaustive on purpose: a new ending has to make
/// every caller say what it does about it.
#[derive(Debug)]
pub enum Advance {
    Turned,
    Concluded(Completion),
}

/// The four things the driver cannot decide for itself. Four closures
/// rather than four traits: no second implementation exists yet, and this
/// library introduces a trait at a seam that already has one.
pub struct RunHooks<'a> {
    /// The clock. Called once per dispatch line, once per turn, and once
    /// per freeze that is not a cancellation.
    pub now: &'a mut dyn FnMut() -> Result<TimeMs, AxError>,
    /// Answers what arrived at a safe point.
    pub interrupt: &'a mut dyn FnMut(SafePoint) -> Interrupt,
    /// The pre-wave checkpoint fence. `None` runs without a net, which
    /// the tool layer refuses for anything that can delete.
    pub fence: Option<&'a mut dyn FnMut(TimeMs) -> Result<Payload, AxError>>,
    /// Runs one tool call. The turn's stamp rides along because the tool
    /// layer stamps results and derives idempotency keys from it, and a
    /// caller that sampled its own clock there would be a second time
    /// source inside one turn.
    pub invoke: &'a mut dyn FnMut(&ToolCall, TimeMs) -> Result<ToolOutcome, AxError>,
    /// Where text goes while the model is still saying it.
    ///
    /// `None` runs the model without asking for a stream, which is what
    /// citysim and every offline replay do: an increment changes nothing
    /// a run decides, so a driver with nowhere to put one asks for none
    /// and the call is byte-identical to what it always was.
    ///
    /// It takes no result, because nothing here may fail a call. What
    /// arrives is a thing to look at; the record of what was said is
    /// written from `model_returned`, once, afterwards.
    pub deltas: Option<&'a mut (dyn FnMut(&str) + 'a)>,
}

/// An active run: turns may still be taken.
pub struct Active {
    window: Window,
    turns: u32,
    last_turn_t: Option<TimeMs>,
}

/// A frozen run. There is no method back to [`Active`]: waking an old run
/// is not something this type can spell, and `resume` takes a [`Handoff`]
/// rather than a run.
pub struct Frozen {
    completion: Completion,
    turns: u32,
}

pub struct Run<S> {
    plan: RunPlan,
    state: S,
}

impl Run<Active> {
    /// The dispatch pair: the job pin lands first, then the run exists.
    /// Two ledger lines, two clock samples — the pin is a fact about the
    /// city and the start is a fact about the run.
    ///
    /// # Errors
    /// Propagates whatever the ledger says; nothing else here can fail.
    pub fn dispatch(
        plan: RunPlan,
        ledger: &mut dyn Ledger,
        hooks: &mut RunHooks<'_>,
    ) -> Result<Run<Active>, AxError> {
        let pin_t = (hooks.now)()?;
        let mut pin = Map::new();
        pin.insert("job".to_owned(), Value::String(plan.job.to_string()));
        ledger.append(EventDraft {
            run: RunId::CITY,
            t: pin_t,
            who: "city".to_owned(),
            addr: Some(plan.addr.clone()),
            kind: EventKind::CheckpointCommitted,
            data: payload(pin)?,
            ig: false,
        })?;

        let start_t = (hooks.now)()?;
        let mut started = Map::new();
        started.insert("task".to_owned(), Value::String(plan.task.clone()));
        started.insert("goal".to_owned(), Value::String(plan.goal.clone()));
        started.insert("job".to_owned(), Value::String(plan.job.to_string()));
        if let Some(parent) = plan.parent {
            started.insert("parent".to_owned(), Value::String(parent.to_string()));
        }
        // Integers, like every quantity on the ledger (determinism rule
        // 6). Unconditional rather than omitted when zero: "no ceiling"
        // and "a ceiling of nothing" are the same fact here, and a key
        // that comes and goes is a shape a reader has to guess at.
        started.insert(
            "usd_micros".to_owned(),
            Value::Number(plan.budget.usd.get().into()),
        );
        started.insert(
            "tokens".to_owned(),
            Value::Number(plan.budget.tokens.get().into()),
        );
        // Unconditional for the same reason the budget is: a key that
        // comes and goes is a shape a reader has to guess at, and "this
        // building admits nothing" is a fact worth recording rather than
        // an absence to infer.
        started.insert(
            "skills".to_owned(),
            Value::Array(
                plan.skills
                    .iter()
                    .map(|pin| {
                        let mut row = Map::new();
                        row.insert("name".to_owned(), Value::String(pin.name.clone()));
                        row.insert("hash".to_owned(), Value::String(pin.hash.to_string()));
                        Value::Object(row)
                    })
                    .collect(),
            ),
        );
        ledger.append(EventDraft {
            run: plan.run,
            t: start_t,
            who: "city".to_owned(),
            addr: Some(plan.addr.clone()),
            kind: EventKind::RunStarted,
            data: payload(started)?,
            ig: false,
        })?;

        let mut window = Window::new();
        window.push_task_lines(&plan.task, &plan.goal, plan.opening);
        Ok(Run {
            plan,
            state: Active {
                window,
                turns: 0,
                last_turn_t: None,
            },
        })
    }

    /// Takes one turn. Every exit from a phase is either the next phase or
    /// a cancellation, so the three safe points are the only places an
    /// interruption can land.
    ///
    /// # Errors
    /// Propagates ledger and provider failures. A tool failure is not one
    /// of them: it reaches the model as this call's result.
    pub fn advance(
        &mut self,
        ledger: &mut dyn Ledger,
        model: &mut dyn Model,
        hooks: &mut RunHooks<'_>,
    ) -> Result<Advance, AxError> {
        let index = self.state.turns;
        let t = (hooks.now)()?;
        self.state.last_turn_t = Some(t);

        let turn = Turn::begin(self.plan.run, self.plan.who.clone(), t);
        let opening = (hooks.interrupt)(SafePoint::BeforeAssemble { turn: index });
        fold_steer(&mut self.state.window, &opening);
        let turn = match turn.assemble(
            opening,
            ledger,
            &self.plan.prefix,
            &self.state.window,
            &self.plan.tools,
            &self.plan.shape,
        )? {
            PhaseOutcome::Advanced(next) => next,
            PhaseOutcome::Cancelled(_) => return Ok(Advance::Concluded(Completion::Cancelled)),
        };
        let calling = (hooks.interrupt)(SafePoint::BeforeCall { turn: index });
        fold_steer(&mut self.state.window, &calling);
        let turn = match turn.call(
            calling,
            ledger,
            model,
            &self.plan.policy,
            // Reborrowed rather than moved: the sink belongs to the
            // hooks and every later turn needs it too.
            hooks.deltas.as_deref_mut(),
        )? {
            PhaseOutcome::Advanced(next) => next,
            PhaseOutcome::Cancelled(_) => return Ok(Advance::Concluded(Completion::Cancelled)),
        };

        // The fence goes up before the wave, not before a suspicious call:
        // anything the wave deletes then has a commit to come back from.
        if let Some(fence) = hooks.fence.as_mut() {
            let committed = fence(t)?;
            ledger.append(EventDraft {
                run: self.plan.run,
                t,
                who: self.plan.who.clone(),
                addr: Some(self.plan.addr.clone()),
                kind: EventKind::CheckpointCommitted,
                data: committed,
                ig: false,
            })?;
        }

        let wave = (hooks.interrupt)(SafePoint::BeforeWave { turn: index });
        fold_steer(&mut self.state.window, &wave);
        let invoke = &mut hooks.invoke;
        let mut stamped = |call: &ToolCall| invoke(call, t);
        let turn = match turn.execute(wave, ledger, &mut stamped)? {
            PhaseOutcome::Advanced(next) => next,
            PhaseOutcome::Cancelled(_) => return Ok(Advance::Concluded(Completion::Cancelled)),
        };

        let settling = (hooks.interrupt)(SafePoint::BeforeSpawn { turn: index });
        fold_steer(&mut self.state.window, &settling);
        let report = match turn.record(settling, ledger)? {
            PhaseOutcome::Advanced(report) => report,
            PhaseOutcome::Cancelled(_) => return Ok(Advance::Concluded(Completion::Cancelled)),
        };
        self.state.turns = self.state.turns.saturating_add(1);
        self.state
            .window
            .push_assistant(report.assistant().to_vec());
        self.state
            .window
            .push_tool_results(report.wave_results().to_vec());
        if report.calls_made() == 0 {
            let evidence = Evidence::new(vec![*report.model_returned()])?;
            return Ok(Advance::Concluded(Completion::Done(evidence)));
        }
        Ok(Advance::Turned)
    }

    /// The only exit. Both lines are written here, so no caller can end a
    /// run by simply dropping it and leaving the ledger without a verdict.
    ///
    /// A cancelled run freezes inside the turn it interrupted and carries
    /// that turn's stamp; any other ending samples the clock once, and
    /// `run_frozen` follows one millisecond later because the two lines
    /// record one event.
    ///
    /// # Errors
    /// Propagates ledger failures and a clock that has run past `u64`.
    pub fn freeze(
        self,
        ledger: &mut dyn Ledger,
        handoff: &Handoff,
        completion: Completion,
        hooks: &mut RunHooks<'_>,
    ) -> Result<Run<Frozen>, AxError> {
        let t = match (&completion, self.state.last_turn_t) {
            (Completion::Cancelled, Some(turn_t)) => turn_t,
            _ => (hooks.now)()?,
        };
        ledger.append(EventDraft {
            run: self.plan.run,
            t,
            who: self.plan.who.clone(),
            addr: None,
            kind: EventKind::HandoffWritten,
            data: handoff.payload()?,
            ig: false,
        })?;
        let closing = t.value().checked_add(1).ok_or_else(|| {
            AxError::failure(AxCode::InvalidArgs, "stamp run_frozen", "u64 overflow")
                .with_recovery("check the clock the caller injected")
        })?;
        let mut frozen = Map::new();
        completion.extend_payload(&mut frozen)?;
        ledger.append(EventDraft {
            run: self.plan.run,
            t: TimeMs::new(closing),
            who: self.plan.who.clone(),
            addr: None,
            kind: EventKind::RunFrozen,
            data: payload(frozen)?,
            ig: false,
        })?;
        let turns = self.state.turns;
        Ok(Run {
            plan: self.plan,
            state: Frozen { completion, turns },
        })
    }

    pub fn turns_taken(&self) -> u32 {
        self.state.turns
    }
}

impl Run<Frozen> {
    pub fn completion(&self) -> &Completion {
        &self.state.completion
    }

    pub fn turns(&self) -> u32 {
        self.state.turns
    }

    pub fn run_id(&self) -> RunId {
        self.plan.run
    }
}

/// Dispatch, turn until an ending, freeze. The loop is here rather than in
/// each caller because the ending rules — an empty wave concludes, an
/// exhausted budget is a limit, a cancellation is an ending too — are the
/// part that must not drift between the city and the simulator.
///
/// # Errors
/// Propagates ledger and provider failures.
pub fn drive(
    plan: RunPlan,
    ledger: &mut dyn Ledger,
    model: &mut dyn Model,
    hooks: &mut RunHooks<'_>,
    handoff: &Handoff,
) -> Result<Run<Frozen>, AxError> {
    let budget = plan.budget_turns;
    let mut run = Run::dispatch(plan, ledger, hooks)?;
    let mut ending = Completion::Limit;
    while run.turns_taken() < budget {
        match run.advance(ledger, model, hooks) {
            Ok(Advance::Turned) => {}
            Ok(Advance::Concluded(completion)) => {
                ending = completion;
                break;
            }
            // A run always ends. A mid-turn failure whose code has a
            // carrier event (provider down, budget, watchdog) is written
            // into history under that carrier and the run freezes as
            // cancelled - before this arm existed, a 401 from a provider
            // left a run permanently "started": no event, no freeze, an
            // event stream that simply went quiet. Loadtime codes still
            // propagate: when the ledger itself is the casualty there is
            // nothing truthful left to write.
            Err(err) => {
                let Carrier::Event(kind) = err.code().carrier() else {
                    return Err(err);
                };
                let t = (hooks.now)()?;
                let mut data = Map::new();
                if let Ok(Value::Object(fields)) = serde_json::to_value(&err) {
                    data = fields;
                }
                ledger.append(EventDraft {
                    run: run.plan.run,
                    t,
                    who: run.plan.who.clone(),
                    addr: Some(run.plan.addr.clone()),
                    kind,
                    data: payload(data)?,
                    ig: false,
                })?;
                ending = Completion::Cancelled;
                break;
            }
        }
    }
    run.freeze(ledger, handoff, ending, hooks)
}

fn payload(map: Map<String, Value>) -> Result<Payload, AxError> {
    Payload::new(map)
}
