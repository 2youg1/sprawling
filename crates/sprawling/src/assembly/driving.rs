// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! One drive, and what it leaves behind.

use std::path::Path;

use kernel::{AxCode, AxError};
use kernel::{RunId, TimeMs};
use runtime::Interrupt;
use runtime::bench::{BenchOutcome, ToolBench};
use runtime::run::{RunHooks, RunPlan, SafePoint, drive};

use super::{RunWorker, now_ms};

/// What one drive is handed: the machinery it runs on, and the run it
/// runs as.
///
/// Seven values that arrive together and are meaningless apart - a bench
/// without the resident that invokes it derives the wrong key, a fence
/// scope without the tree it fences covers the wrong files. They were
/// seven parameters, and `#[expect(clippy::too_many_arguments)]` sat
/// above them saying so.
pub(super) struct Driving<'a> {
    /// The model this run calls, already chosen and already credentialed.
    pub(super) adapter: &'a mut dyn kernel::Model,
    /// What routes a call the model makes.
    pub(super) bench: &'a mut ToolBench,
    /// Where a steer from a resident lands while the drive is going.
    pub(super) signals: &'a std::rc::Rc<std::cell::RefCell<collab::SignalDesk>>,
    /// The tree the run writes in: its own worktree under review, the
    /// city itself otherwise.
    pub(super) write_root: &'a Path,
    /// What a checkpoint fence covers, from [`Site::fence_scope`].
    pub(super) fence_scope: String,
    /// The resident this run works as, as three hooks will name it.
    pub(super) who: &'a str,
    pub(super) run_id: RunId,
}

/// What one drive left behind, beside the run it froze.
///
/// Every field is written by a hook while the driver owns the ledger and
/// read after it gives it back, so none of them may be acted on until the
/// drive has returned.
pub(super) struct Driven {
    pub(super) outcome: Result<runtime::Run<runtime::run::Frozen>, AxError>,
    /// The commits each wave fenced against; the first is what the sweep
    /// restores a discarded file from.
    pub(super) fenced: Vec<String>,
    /// The run's own commands, as (passed, failed).
    pub(super) ran: (u32, u32),
    /// What a gate escalated while the ledger was not the worker's.
    pub(super) raised: Vec<kernel::ApprovalItem>,
}

impl RunWorker {
    /// Runs the plan, and hands back what the drive left behind.
    ///
    /// The three hooks live here because they are the only code that
    /// touches the ledger while the driver owns it: one interrupt source
    /// merging the person and the residents, one fence going up before
    /// each wave, and one invocation point deriving the key for a call.
    /// Everything they collect - the commits a wave fenced against, what
    /// the run's own commands did, and the items a gate raised - is
    /// theirs only for the length of the drive, so it comes back as one
    /// value rather than as four cells the caller has to keep in step.
    ///
    /// The drive's own outcome stays a `Result` inside [`Driven`] rather
    /// than being propagated: a run that failed still has desks to
    /// settle, and settling them is what puts its last lines on the
    /// history.
    ///
    /// # Errors
    /// Propagates a checkpoint that will not open, which is the one
    /// failure that happens before the run starts.
    pub(super) fn drive_dispatch(
        &mut self,
        plan: RunPlan,
        handoff: &runtime::handoff::Handoff,
        driving: Driving<'_>,
    ) -> Result<Driven, AxError> {
        let Driving {
            adapter,
            bench,
            signals,
            write_root,
            fence_scope,
            who,
            run_id,
        } = driving;
        let mut now = || now_ms();
        // Taken before the hooks borrow `self`: the sink outlives one
        // drive and belongs to the worker, not to this call.
        let watching = self.watching.clone();
        let bench_who = who.to_owned();
        let mut fence_point =
            memory::Checkpoint::open(write_root).map_err(memory::MemoryError::into_ax)?;
        let fence_who = who.to_owned();
        // What the bench fenced, so the sweep afterwards knows which
        // commit a deleted file can be restored from.
        let fenced: std::rc::Rc<std::cell::RefCell<Vec<String>>> =
            std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let fenced_by_bench = std::rc::Rc::clone(&fenced);
        // Taken for the length of the drive and put back after: the
        // hooks cannot borrow the worker, and a source that stayed
        // behind would be a second one.
        let mut source = self.interrupts.take();
        // What the run's own commands did. It is the only evidence of
        // "the tests passed" the city can observe without being told,
        // and being told is what a mode is supposed to check.
        let ran: std::rc::Rc<std::cell::RefCell<(u32, u32)>> =
            std::rc::Rc::new(std::cell::RefCell::new((0, 0)));
        let ran_by_bench = std::rc::Rc::clone(&ran);
        // What the bench raised while the driver held the ledger.
        let raised: std::rc::Rc<std::cell::RefCell<Vec<kernel::ApprovalItem>>> =
            std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let raised_by_bench = std::rc::Rc::clone(&raised);
        let driven = {
            let raised = raised_by_bench;
            let fenced = fenced_by_bench;
            let ran = ran_by_bench;
            // Two speakers, one landing, and the person outranks the
            // resident. What keeps them apart where the model reads them
            // is `collab::Steer`: only the person's entrance can write
            // the `user` prefix, and a resident's writes `@` and its own
            // address - the address a reply is sent to. A run that could
            // not tell the two apart would answer the person by
            // signalling them, and answer a neighbour by talking to
            // nobody.
            let steers = std::rc::Rc::clone(signals);
            let mut interrupt = |_: SafePoint| {
                let from_person = match source.as_mut() {
                    Some(ask) => ask(run_id),
                    None => Interrupt::None,
                };
                if !matches!(from_person, Interrupt::None) {
                    return from_person;
                }
                // A desk in use answers nothing rather than refusing:
                // the interrupt runs between tool calls, so this borrow
                // is free in practice, and a safe point is the wrong
                // place to fail over a lock.
                let Ok(mut desk) = steers.try_borrow_mut() else {
                    return Interrupt::None;
                };
                match desk.take_steer() {
                    Some(steer) => Interrupt::Steer {
                        source: steer.source().to_owned(),
                        text: steer.text().to_owned(),
                    },
                    None => Interrupt::None,
                }
            };
            // Where this call sits in this run. The key used to derive
            // from the turn's millisecond stamp and the tool's name,
            // which broke twice over: it took a clock, which determinism
            // rule 7 forbids outright, and it ignored the arguments - so
            // two `read`s of two different files in one turn were one
            // key, and the second came back "this call was already
            // made". A model reads that as a fault in itself.
            let placed = std::cell::Cell::new(0u64);
            let mut invoke = |call: &kernel::ToolCall, t: TimeMs| {
                let at = placed.get();
                placed.set(at.saturating_add(1));
                // What the action is, is the tool face's to say
                // (kernel-SPEC 8-23). Two identical calls at two
                // positions are two keys and both run; the same position
                // replayed is one key, which is what deduplication is
                // for.
                let key = kernel::IdemKey::derive(&run_id, kernel::Seq::new(at), &call.action()?);
                let ctx = kernel::GateContext {
                    actor: bench_who.clone(),
                    now: t,
                    item_id: kernel::ApprovalId::new(format!("item-{}", t.value())).ok_or_else(
                        || AxError::failure(AxCode::InvalidArgs, "mint approval id", "empty id"),
                    )?,
                };
                match bench.invoke(call, &key, &ctx)? {
                    BenchOutcome::Ran {
                        outcome,
                        fenced: at,
                    } => {
                        if let Some(oid) = at {
                            fenced.borrow_mut().push(oid);
                        }
                        if call.name.as_str() == "exec" {
                            let failed = outcome
                                .result
                                .as_map()
                                .get("exit_code")
                                .and_then(serde_json::Value::as_i64)
                                .is_some_and(|code| code != 0);
                            let mut counts = ran.borrow_mut();
                            if failed {
                                counts.1 = counts.1.saturating_add(1);
                            } else {
                                counts.0 = counts.0.saturating_add(1);
                            }
                        }
                        Ok(outcome)
                    }
                    BenchOutcome::Refused { refusal } => Err(*refusal),
                    BenchOutcome::Pending { item } => {
                        // Stashed rather than recorded here: the ledger is
                        // the driver's for the length of the run, and one
                        // writer is the whole point. The record is written
                        // the moment the drive returns.
                        let id = item.id.as_str().to_owned();
                        raised.borrow_mut().push(*item);
                        Err(
                            AxError::failure(AxCode::ApprovalPending, "await approval", id)
                                .with_recovery(
                                    "answer the approval in the inbox, then dispatch again",
                                ),
                        )
                    }
                    BenchOutcome::Duplicate => Err(AxError::failure(
                        AxCode::InvalidArgs,
                        "invoke tool",
                        "this call was already made",
                    )),
                }
            };
            let mut fence = |t: TimeMs| {
                let payload = fence_point
                    .wave_pre(&fence_scope, t, &fence_who)
                    .map_err(memory::MemoryError::into_ax)?;
                if let Some(oid) = payload
                    .as_map()
                    .get("oid")
                    .and_then(serde_json::Value::as_str)
                {
                    fenced.borrow_mut().push(oid.to_owned());
                }
                Ok(payload)
            };
            // Where text goes while the model is still saying it. The
            // sink is whatever the assembly layer was given - a socket
            // task in a running city, nothing at all in citysim and in
            // replay - so a run with nobody watching asks the provider
            // for no stream and behaves exactly as it always did.
            let mut watched = |said: &str| {
                if let Some(onto) = watching.as_ref() {
                    onto(channels::Delta {
                        run: run_id,
                        text: said.to_owned(),
                    });
                }
            };
            let mut hooks = RunHooks {
                now: &mut now,
                interrupt: &mut interrupt,
                fence: Some(&mut fence),
                invoke: &mut invoke,
                deltas: watching.is_some().then_some(&mut watched),
            };
            drive(plan, &mut self.ledger, adapter, &mut hooks, handoff)
        };
        self.interrupts = source;
        Ok(Driven {
            outcome: driven,
            fenced: fenced.borrow().clone(),
            ran: *ran.borrow(),
            raised: raised.borrow().clone(),
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
    use crate::assembly::fixture::*;
    use crate::assembly::*;
    use crate::serving::CommandDesk;
    use crate::views::Views;

    /// The fake provider hands out its scripted replies in order, so
    /// whatever decides that a turn happened decides what every later
    /// turn answers. A client may open a socket and write nothing, or
    /// stop halfway through a body; neither asked anything, and neither
    /// may spend a reply the next question needs. This arrived as a run
    /// that never opened the request its script told it to, three turns
    /// downstream, in a saturated workspace run and never alone.
    #[test]
    fn a_socket_that_carried_no_request_spends_no_scripted_reply() {
        let (base_url, provider) = fake_openai(
            &["m-local"],
            vec![
                completion("the first turn", None),
                completion("the second turn", None),
            ],
        );
        let addr = base_url
            .trim_start_matches("http://")
            .trim_end_matches("/v1")
            .to_owned();

        // Opened and abandoned: a pooled socket nobody wrote to.
        drop(std::net::TcpStream::connect(&addr).unwrap());

        // Opened and cut short: a body that stops before content-length.
        let mut cut = std::net::TcpStream::connect(&addr).unwrap();
        std::io::Write::write_all(
            &mut cut,
            b"POST /v1/chat/completions HTTP/1.1\r\ncontent-length: 64\r\n\r\n{\"cut\":",
        )
        .unwrap();
        drop(cut);

        // The first socket that actually asked gets the first reply.
        let body = "{\"model\":\"m-local\"}";
        let mut asking = std::net::TcpStream::connect(&addr).unwrap();
        std::io::Write::write_all(
            &mut asking,
            format!(
                "POST /v1/chat/completions HTTP/1.1\r\ncontent-length: {}\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .unwrap();
        let mut answered = String::new();
        std::io::Read::read_to_string(&mut asking, &mut answered).unwrap();

        assert!(
            answered.contains("the first turn"),
            "a socket that asked nothing spent the first reply: {answered}"
        );
        assert_eq!(
            provider.exchanges().len(),
            1,
            "only what arrived whole is on the record: {:?}",
            provider.exchanges()
        );
    }

    #[test]
    fn a_dispatch_runs_a_whole_turn_loop_and_the_chain_still_verifies() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();

        let (base_url, _provider) = fake_openai(
            &["m-local"],
            vec![
                completion("editing", Some(("tu_1", "lab/room1/notes.md"))),
                completion("done", None),
            ],
        );
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&seen);
        worker.observe(Box::new(move |record: &EventRecord| {
            sink.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(record.kind());
        }));

        worker
            .handle(channels::Command::Dispatch {
                addr: Address::parse("lab/room1").unwrap(),
                task: "say hello".to_owned(),
                goal: "one turn is enough".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"dispatch"),
                session: None,
                effort: None,
            })
            .unwrap();

        let kinds = seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert!(kinds.contains(&EventKind::RunStarted), "{kinds:?}");
        assert!(kinds.contains(&EventKind::ToolCalled), "{kinds:?}");
        assert!(kinds.contains(&EventKind::HandoffWritten), "{kinds:?}");
        assert!(kinds.contains(&EventKind::RunFrozen), "{kinds:?}");

        // The effect, not only the freeze: the canary edit really landed.
        // Asserting kinds alone once passed while every edit was refused.
        assert_eq!(
            std::fs::read_to_string(dir.path().join("lab/room1/notes.md")).unwrap(),
            "noted\n"
        );

        // The whole thing is one verifiable chain, genesis included.
        let verified = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
        assert!(verified.raw_lines().len() > kinds.len());
    }

    #[test]
    fn the_views_answer_from_the_ledger_and_rebuild_to_the_same_answer() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let (base_url, _provider) = fake_openai(
            &["m-local"],
            vec![
                completion("editing", Some(("tu_1", "lab/room1/notes.md"))),
                completion("done", None),
            ],
        );
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        let live = std::sync::Arc::new(std::sync::Mutex::new(Views::new(dir.path())));
        let folding = std::sync::Arc::clone(&live);
        worker.observe(Box::new(move |record: &EventRecord| {
            folding
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .apply(record)
                .unwrap();
        }));
        worker
            .handle(channels::Command::Dispatch {
                addr: Address::parse("lab/room1").unwrap(),
                task: "say hello".to_owned(),
                goal: "one turn is enough".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"dispatch"),
                session: None,
                effort: None,
            })
            .unwrap();

        let mut live = live
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let channels::Answer::City(city) = live.answer(&channels::Query::CityView) else {
            panic!("CityView answers with a city");
        };
        assert_eq!(city.frozen, 1, "the dispatched run reached its freeze");
        let run = city.runs.iter().find(|row| row.frozen).unwrap().run;
        let channels::Answer::Run(Some(one)) = live.answer(&channels::Query::RunView { run })
        else {
            panic!("RunView answers about a run the city has");
        };
        assert_eq!(one.last_kind, EventKind::RunFrozen);

        // The same answer arrives from a cold rebuild: a view is
        // disposable exactly to the extent that this holds.
        let mut rebuilt = rebuild_views(&report.ledger_dir).unwrap();
        let channels::Answer::City(again) = rebuilt.answer(&channels::Query::CityView) else {
            panic!("CityView answers with a city");
        };
        assert_eq!(city, again);

        // Every query this build carries now answers; the shape that
        // named itself unavailable is gone because nothing is left to
        // name. What a page must still be able to tell apart is "this
        // city archived nothing" from "this build cannot say", and the
        // first is an empty list rather than a refusal.
        let channels::Answer::Registry(registry) = live.answer(&channels::Query::RegistryView)
        else {
            panic!("RegistryView answers with a registry");
        };
        assert!(registry.assets.is_empty());
    }

    #[test]
    fn a_confidential_building_stops_the_run_before_a_remote_call() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        lay_rules(
            dir.path(),
            "vault",
            "# BUILDING.md\n\n## confidential\n\n`confidential: true`\n",
        );

        // A remote endpoint, wired as the provider for a confidential
        // building: the refusal must come from the adapter that could
        // leak, not from a routing table that a mistake could bypass.
        let endpoint = gateway::Endpoint::new(
            gateway::EndpointConfig {
                base_url: "http://127.0.0.1:1/v1/messages".to_owned(),
                dialect: kernel::DialectKind::Anthropic,
                model: "remote".to_owned(),
                auth: gateway::AuthSpec::None,
                extra_headers: Vec::new(),
                overrides: Vec::new(),
                timeout_ms: 1_000,
                pricing: None,
            },
            Box::new(|_reference: &kernel::SecretRef| {
                Err(AxError::failure(
                    AxCode::ConfigInvalid,
                    "resolve a credential",
                    "none configured",
                ))
            }),
        )
        .unwrap();

        let (base_url, provider) = fake_openai(&["m-local"], vec![completion("done", None)]);
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        let _ = endpoint;
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&seen);
        worker.observe(Box::new(move |record: &EventRecord| {
            sink.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push((record.kind(), serde_json::to_string(record.data()).unwrap()));
        }));
        // The keystroke is accepted; the refusal belongs to the run's own
        // account (drive backstop, card R1.05). What must never happen is
        // a chat POST reaching the endpoint.
        worker
            .handle(channels::Command::Dispatch {
                addr: Address::parse("vault/room1").unwrap(),
                task: "read the private notes".to_owned(),
                goal: "summarise them".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"dispatch"),
                session: None,
                effort: None,
            })
            .unwrap();
        let events = seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        let denied = events
            .iter()
            .find(|(kind, _)| *kind == EventKind::GateDenied)
            .expect("the refusal is written under its carrier");
        assert!(denied.1.contains("local model"), "{}", denied.1);
        assert!(
            events.iter().any(|(kind, _)| *kind == EventKind::RunFrozen),
            "a refused run still ends"
        );
        assert!(
            !provider
                .exchanges()
                .iter()
                .any(|head| head.starts_with("POST")),
            "no chat call may leave a confidential building"
        );
    }

    #[test]
    fn a_cancel_reaches_the_run_it_cancels_without_waiting_for_it_to_end() {
        let desk = CommandDesk::new();
        let mine = RunId::CITY;
        let other = kernel::RunId::from_bytes([7u8; 16]);
        // One key per ask, the way every client mints them: the desk
        // now reads the key, so three commands sharing one would be one
        // command repeated twice (section 8-33).
        let key =
            |material: &[u8]| kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, material);

        let nobody = || channels::Reply::nowhere();
        desk.post(
            channels::Command::Steer {
                run: other,
                text: "not for me".to_owned(),
                idem: key(b"not for me"),
            },
            nobody(),
        );
        desk.post(
            channels::Command::Steer {
                run: mine,
                text: "  measure it in metres  ".to_owned(),
                idem: key(b"measure it in metres"),
            },
            nobody(),
        );
        desk.post(
            channels::Command::Cancel {
                run: mine,
                idem: key(b"cancel"),
            },
            nobody(),
        );

        // Cancel outranks a steer that arrived first: stopping and
        // changing course are exclusive, and stopping cannot be undone.
        assert!(matches!(desk.interrupt_for(mine), Interrupt::Cancel));
        let Interrupt::Steer { source, text } = desk.interrupt_for(mine) else {
            panic!("the steer for this run is still waiting");
        };
        assert_eq!(source, "user");
        assert_eq!(text, "measure it in metres");
        assert!(matches!(desk.interrupt_for(mine), Interrupt::None));

        // And the other run's command kept its place in line.
        let Some(channels::Command::Steer { run, .. }) = desk.take() else {
            panic!("a command for another run is not consumed by this one");
        };
        assert_eq!(run, other);
    }

    #[test]
    fn a_steer_lands_at_the_end_of_the_next_tool_result() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let (base_url, provider) = fake_openai(
            &["m-local"],
            vec![
                completion("editing", Some(("tu_1", "lab/room1/notes.md"))),
                completion("done", None),
            ],
        );
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        // One steer, delivered at the first safe point that asks.
        let mut left = 1;
        worker.attach_interrupts(Box::new(move |_| {
            if left > 0 {
                left -= 1;
                return Interrupt::Steer {
                    source: "user".to_owned(),
                    text: "measure it in metres".to_owned(),
                };
            }
            Interrupt::None
        }));
        worker
            .handle(channels::Command::Dispatch {
                addr: Address::parse("lab/room1").unwrap(),
                task: "measure the thing".to_owned(),
                goal: "a number, then stop".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"dispatch"),
                session: None,
                effort: None,
            })
            .unwrap();

        let verified = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
        let history: String = verified
            .raw_lines()
            .iter()
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .collect::<Vec<String>>()
            .join("\n");
        assert!(history.contains("steer_received"));

        let asked = provider.bodies().join("\n");
        assert!(
            asked.contains("measure it in metres"),
            "the steer reaches the model in the window, not only the ledger"
        );
        assert!(
            history.contains("run_frozen"),
            "a steer changes course; it does not end the run"
        );
    }

    /// Deleting one file, in this platform's own words. The point of the
    /// test is the sweep, and the sweep does not care which command did
    /// it — which is exactly why the sweep is the defence.
    fn delete_command(name: &str) -> (String, Vec<String>) {
        if cfg!(windows) {
            (
                "cmd".to_owned(),
                vec!["/C".to_owned(), "del".to_owned(), name.to_owned()],
            )
        } else {
            ("rm".to_owned(), vec![name.to_owned()])
        }
    }

    #[test]
    fn a_file_an_exec_deleted_comes_back_with_somewhere_to_come_back_from() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let room = dir.path().join("lab").join("room1");
        std::fs::create_dir_all(&room).unwrap();
        std::fs::write(room.join("kiln.md"), "firing notes\n").unwrap();
        let (path, args) = delete_command("kiln.md");
        let (base_url, provider) = fake_openai(
            &["m-local"],
            vec![
                tool_completion(
                    "clearing up",
                    "tu_1",
                    "exec",
                    serde_json::json!({ "arm": { "program": { "path": path, "args": args } } }),
                ),
                completion("cleared", None),
            ],
        );
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        worker
            .handle(channels::Command::Dispatch {
                addr: Address::parse("lab/room1").unwrap(),
                task: "tidy the room".to_owned(),
                goal: "remove the stale note".to_owned(),
                mode: channels::ModeTag::parse("build").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::new(0), b"dispatch"),
                session: None,
                effort: None,
            })
            .unwrap();
        drop(provider);

        assert!(
            !room.join("kiln.md").exists(),
            "the command ran; if it did not, this test proves nothing"
        );
        let verified = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
        let history: String = verified
            .raw_lines()
            .iter()
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .collect::<Vec<String>>()
            .join("\n");
        assert!(
            history.contains("file_discarded"),
            "a deletion no forecast saw is still written down: {history}"
        );
        assert!(
            history.contains("restoration"),
            "and it is written down with the way back"
        );
    }

    /// What the Ledger means by "every effect becomes an EventRecord
    /// first", read from the one place where *first* is visible: the
    /// write observer, which `memory::jsonl` runs on the appending
    /// thread **after** the line is durable.
    ///
    /// One run files a decision on the building's shelf and takes a row
    /// from the shared plan. At the instant each line lands, the city
    /// must not already carry what that line announces - otherwise a
    /// process dying between the two leaves a shelf entry and a claimed
    /// row that the history never mentions, and the shelf's own comment
    /// ("nothing is on the shelf that the history does not already
    /// carry") is false.
    #[test]
    fn what_a_run_changes_is_changed_after_the_line_that_announces_it() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        let lab = dir.path().join("lab");
        std::fs::create_dir_all(lab.join("room1")).unwrap();
        std::fs::write(lab.join(city::ROADMAP_FILE), PLAN_TWO_FREE_ROWS).unwrap();

        let (base_url, provider) = fake_openai(
            &["m-local"],
            vec![
                tool_completion(
                    "remembering why",
                    "tu_1",
                    "archive",
                    serde_json::json!({
                        "action": "record",
                        "kind": "decision",
                        "text": "we chose the embedded store",
                    }),
                ),
                tool_completion(
                    "taking a row",
                    "tu_2",
                    "plan",
                    serde_json::json!({ "action": "claim", "node": "1" }),
                ),
                completion("done", None),
            ],
        );
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();

        // Which line landed, and whether the city already carried what
        // it announces at that moment.
        let seen: Arc<std::sync::Mutex<Vec<(&'static str, bool)>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let watch = Arc::clone(&seen);
        let root = dir.path().to_path_buf();
        worker.observe(Box::new(move |record: &EventRecord| {
            let lab = Address::parse("lab").unwrap();
            let carried = match record.kind() {
                EventKind::AssetArchived => Some((
                    "asset_archived",
                    !city::archive_index(&root, &lab).unwrap().is_empty(),
                )),
                EventKind::RoadmapClaimed => Some((
                    "roadmap_claimed",
                    city::roadmap(&root, &lab).unwrap().contains("In progress"),
                )),
                _ => None,
            };
            if let Some(landed) = carried {
                watch.lock().unwrap().push(landed);
            }
        }));

        worker
            .handle(channels::Command::Dispatch {
                addr: Address::parse("lab/room1").unwrap(),
                task: "remember one thing and take one row".to_owned(),
                goal: "one decision, one claim".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"order"),
                session: None,
                effort: None,
            })
            .unwrap();
        drop(provider);

        let landed = seen.lock().unwrap().clone();
        assert_eq!(
            landed.len(),
            2,
            "both effects reached the history: {landed:?}"
        );
        let early: Vec<&str> = landed
            .iter()
            .filter(|(_, already)| *already)
            .map(|(line, _)| *line)
            .collect();
        assert!(
            early.is_empty(),
            "the city already carried what these lines announce before they were on the ledger: \
             {early:?}"
        );

        // Ordered, not lost: both changes are the city's once the run
        // is over.
        let lab_addr = Address::parse("lab").unwrap();
        assert_eq!(
            city::archive_index(dir.path(), &lab_addr).unwrap().len(),
            1,
            "the decision is on the shelf"
        );
        assert!(
            city::roadmap(dir.path(), &lab_addr)
                .unwrap()
                .contains("| 1 | wire the kiln | 1 |  | In progress |"),
            "the node is claimed in the plan"
        );
    }

    #[test]
    fn a_resident_crosses_two_runs_with_the_same_identity_segment() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        let addr = Address::parse("lab/room1").unwrap();
        std::fs::create_dir_all(dir.path().join("lab").join("room1")).unwrap();
        std::fs::write(
            city::urbanite_path(dir.path(), &addr),
            "# URBANITE.md\n\nAsks rather than guesses.\n",
        )
        .unwrap();

        let (base_url, _provider) = fake_openai(
            &["m-local"],
            vec![
                completion("editing", Some(("tu_1", "lab/room1/notes.md"))),
                completion("done", None),
            ],
        );
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        let segments = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&segments);
        worker.observe(Box::new(move |record: &EventRecord| {
            if record.kind() == EventKind::ModelCalled
                && let Some(list) = record.data().as_map().get("segments")
            {
                sink.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(list.clone());
            }
        }));

        for task in ["first errand", "second errand"] {
            worker
                .handle(channels::Command::Dispatch {
                    addr: addr.clone(),
                    task: task.to_owned(),
                    goal: "one turn is enough".to_owned(),
                    mode: channels::ModeTag::parse("plan").unwrap(),
                    budget: kernel::BudgetCap::default(),
                    idem: kernel::IdemKey::derive(
                        &RunId::CITY,
                        kernel::Seq::FIRST,
                        task.as_bytes(),
                    ),
                    session: None,
                    effort: None,
                })
                .unwrap();
        }

        let seen = segments
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(seen.len() >= 2, "two runs, two model calls at least");
        // The resident segment is index 2 of the four; the run segment
        // differs between the two errands, the resident one does not.
        let first = seen[0].as_array().unwrap();
        let second = seen[seen.len() - 1].as_array().unwrap();
        assert_eq!(
            first[2], second[2],
            "the same resident reads the same instructions on every run"
        );
        assert_ne!(first[3], second[3], "and each run carries its own job");
    }

    #[test]
    fn a_provider_failure_freezes_the_run_instead_of_hanging_it() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        // A provider that lists a model, then answers 401 to every chat.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { return };
                let mut buf = [0u8; 8192];
                let mut head = String::new();
                loop {
                    let Ok(n) = std::io::Read::read(&mut stream, &mut buf) else {
                        return;
                    };
                    if n == 0 {
                        break;
                    }
                    head.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if head.contains("\r\n\r\n") && (head.starts_with("GET") || head.ends_with('}'))
                    {
                        break;
                    }
                }
                let body = if head.starts_with("GET") {
                    r#"{"data":[{"id":"m-503"}]}"#.to_owned()
                } else {
                    r#"{"error":{"message":"Authentication failed"}}"#.to_owned()
                };
                let status = if head.starts_with("GET") {
                    "200 OK"
                } else {
                    "401 Unauthorized"
                };
                let response = format!(
                    "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = std::io::Write::write_all(&mut stream, response.as_bytes());
            }
        });
        let mut worker =
            worker_with_provider(dir.path(), &format!("http://{addr}/v1"), "m-503").unwrap();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&seen);
        worker.observe(Box::new(move |record: &EventRecord| {
            sink.lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .push(record.kind());
        }));
        // The command surface must not error out: the failure belongs to
        // the run's own account, not to the person's keystroke.
        worker
            .handle(channels::Command::Dispatch {
                addr: Address::parse("lab/room1").unwrap(),
                task: "anything".to_owned(),
                goal: "an honest ending".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"dispatch"),
                session: None,
                effort: None,
            })
            .unwrap();
        let kinds = seen
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        // Before the drive backstop: events stopped at model_called and
        // the run never froze - a browser watching the stream saw it go
        // quiet forever.
        assert!(
            kinds.contains(&EventKind::ProviderDegraded),
            "the failure is written under its carrier: {kinds:?}"
        );
        assert!(
            kinds.contains(&EventKind::RunFrozen),
            "a run always ends: {kinds:?}"
        );
    }
}
