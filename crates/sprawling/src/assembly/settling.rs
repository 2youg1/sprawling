// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! What a drive left, on the ledger before it is made true.

use kernel::{AxCode, AxError, EventKind};
use kernel::{Locator, Payload, RunId};

use crate::effect;

use super::{Assignment, Desks, Dispatched, Reporter, RunWorker, Site, new_inbox, now_ms};

/// What the sweep after a drive works on.
///
/// The three arrive together because they answer one question - what a
/// wave left in the tree that the history has not accounted for yet: the
/// commits to restore a discarded file from, the escalations a gate
/// raised while the driver held the ledger, and the pin of the work an
/// escalation interrupted. `raised` is borrowed mutably because the
/// sweep adds to it: a discard a person has to answer is raised here
/// rather than during the drive.
pub(super) struct Sweep<'a> {
    pub(super) fenced: &'a [String],
    pub(super) raised: &'a mut Vec<kernel::ApprovalItem>,
    pub(super) job_locator: &'a Locator,
}

/// What one drive ended with, as the conclusion reads it.
///
/// The drive's own outcome stays a `Result` here rather than being
/// propagated: a run that failed still has a tree to give back and
/// approvals to file, and both are worse left undone than the failure
/// that caused them.
pub(super) struct Ending<'a> {
    pub(super) driven: Result<runtime::Run<runtime::run::Frozen>, AxError>,
    pub(super) raised: Vec<kernel::ApprovalItem>,
    pub(super) delegates: &'a std::rc::Rc<std::cell::RefCell<collab::DelegateDesk>>,
}

/// Writes the plan back, creating nothing that was not there: a
/// building without a plan is a building whose residents have nothing
/// to claim, and inventing one here would put a denominator on screen
/// that no person wrote.
pub(super) fn write_plan(path: &std::path::Path, text: &str) -> Result<(), AxError> {
    std::fs::write(path, text).map_err(|err| {
        AxError::failure(
            AxCode::StorageFatal,
            "write the plan",
            format!("{}: {err}", path.display()),
        )
        .with_recovery("fix the file's permissions; the claim was not recorded")
    })
}

impl RunWorker {
    /// Settles the four desks that leave lines behind, in the order the
    /// history takes them.
    ///
    /// Each one goes through `RunWorker::settle`, which appends before it
    /// changes anything: `effect::Then` has no source but
    /// `Landing::record`, so a change that outran its own line cannot be
    /// written from here (section 8-24).
    ///
    /// The room's queue comes home first and on both paths - an inbox
    /// left in a dropped desk is a queue the city forgot it had.
    ///
    /// # Errors
    /// Propagates a payload that will not build, any line the ledger
    /// refuses, and a shared plan that cannot be read or written.
    pub(super) fn settle_desks(
        &mut self,
        site: &Site,
        at: &Assignment,
        desks: &Desks,
        sweep: Sweep<'_>,
    ) -> Result<(), AxError> {
        let (addr, who, run_id) = (&at.addr, site.who.as_str(), site.run_id);
        let (write_root, building) = (site.write_root.as_path(), &site.building);
        // The lent queue comes home first, and on both paths: an inbox
        // left in a dropped desk is a queue the city forgot it had.
        let (signal_effects, returned) = {
            let mut desk = desks.signals.borrow_mut();
            (desk.take_effects(), desk.take_inbox())
        };
        self.inboxes.insert(addr.clone(), returned);
        // Every desk below settles through one door, and that door
        // appends before it changes anything: `effect::Then` has no
        // other source than `Landing::record`, so a change that outran
        // its own line cannot be written here.
        let spoken = effect::Landing::signals(signal_effects, addr, who)?;
        self.settle(at, run_id, spoken)?;
        let ground = effect::Landing::goals(desks.goals.borrow_mut().take_effects(), addr, who)?;
        self.settle(at, run_id, ground)?;
        // The sweep the forecast cannot replace. A command can be
        // obfuscated past a text prediction; what is missing from the
        // working tree cannot be talked out of. The base is the first
        // fence of this drive, so everything the whole drive deleted is
        // reported once rather than once per wave.
        let sweep_base = sweep.fenced.first().cloned();
        if let Some(base) = sweep_base {
            let discarded = memory::Checkpoint::open(write_root)
                .map_err(memory::MemoryError::into_ax)?
                .wave_post(&base)
                .map_err(memory::MemoryError::into_ax)?;
            let swept = discarded.len();
            let lost = effect::Landing::discards(discarded, addr, who);
            self.settle(at, run_id, lost)?;
            // Over the threshold a person is told, and the class is one
            // no policy can waive. Each file is restorable on its own;
            // what the count says is that nobody meant this.
            if swept
                > usize::try_from(kernel::consts_policy::DISCARD_FILES_MAX).unwrap_or(usize::MAX)
            {
                let item = kernel::ApprovalItem {
                    id: kernel::ApprovalId::new(format!("discard-{run_id}")).ok_or_else(|| {
                        AxError::failure(AxCode::InvalidArgs, "mint approval id", "empty id")
                    })?,
                    source: kernel::ApprovalSource::Gate,
                    actor: who.to_owned(),
                    artifact: sweep.job_locator.clone(),
                    action_desc: format!("{swept} files were deleted in one dispatch"),
                    cluster_key: kernel::ClusterKey {
                        class: kernel::ApprovalClass::DiscardEscalate,
                        detail: addr.as_str().to_owned(),
                    },
                    created: now_ms()?,
                    tainted: false,
                };
                sweep.raised.push(item);
                self.note(
                    runtime::diagnostics::Level::Refuse,
                    "memory::checkpoint",
                    &format!(
                        "{swept} files deleted under {}; each one can be restored from {base}",
                        addr.as_str()
                    ),
                );
            }
        }
        // What the run did to the plan.
        let (claim_effects, plan_after) = {
            let mut desk = desks.plan.borrow_mut();
            (desk.take_effects(), desk.roadmap().map(str::to_owned))
        };
        if let Some(text) = plan_after {
            let on_disk = city::roadmap(&self.city_root, building.addr())?;
            match effect::Claims::of(
                &claim_effects,
                &on_disk,
                text,
                desks.plan_path.clone(),
                addr,
                who,
            )? {
                effect::Claims::Landed(taken) => {
                    self.settle(at, run_id, *taken)?;
                    self.tell_whoever_is_behind(
                        at,
                        Reporter {
                            run_id,
                            building: building.addr(),
                            who,
                        },
                        &claim_effects,
                    )?;
                }
                effect::Claims::Stale(nodes) => {
                    for node in nodes {
                        self.note(
                            runtime::diagnostics::Level::Refuse,
                            "collab::claim_tool",
                            &format!(
                                "node {node} moved before this run's claim landed; nothing was \
                                 written"
                            ),
                        );
                    }
                }
            }
        }
        // What the run asked the building to remember, filed after the
        // drive like every other effect - and inside the fence. A
        // building under review is not the owner of what a run decided
        // until somebody checks it, and a shelf entry is exactly the
        // kind of thing a later run reads as the building's settled
        // knowledge.
        let remembered = effect::Landing::shelf(
            desks.shelf.borrow_mut().take_effects(),
            write_root,
            building.addr(),
            now_ms()?,
            addr,
            who,
        )?;
        self.settle(at, run_id, remembered)?;
        Ok(())
    }

    pub(super) fn settle(
        &mut self,
        at: &Assignment,
        run: RunId,
        landing: effect::Landing,
    ) -> Result<(), AxError> {
        let then = landing.record(&mut |line: effect::Line| self.record_for(run, line))?;
        match then {
            effect::Then::Nothing => Ok(()),
            effect::Then::Deliver(signals) => {
                for signal in signals {
                    self.inboxes
                        .entry(signal.room().clone())
                        .or_insert_with(new_inbox)
                        .deliver(&signal)?;
                    // Somebody was spoken to. Whether that starts a run
                    // is decided in one place, so that the two ways of
                    // reaching a resident stay one decision.
                    self.knock(&signal, &at.addr, at.mode, at.budget)?;
                }
                Ok(())
            }
            effect::Then::Hold(entries) => {
                self.goals.extend(entries);
                Ok(())
            }
            effect::Then::Roadmap { path, text } => write_plan(&path, &text),
            effect::Then::Shelf(filings) => {
                for filing in filings {
                    city::file_archive(&filing.entry, &filing.body)?;
                }
                Ok(())
            }
        }
    }

    /// Gives the tree back, files what is waiting for a person, and hands
    /// down the work this run asked for.
    ///
    /// Everything here happens whether the run finished or failed, which
    /// is why the drive's outcome arrives as a `Result` and is unwrapped
    /// only after the desks are settled: a lease left out and an approval
    /// nobody filed are both worse than the failure that caused them.
    ///
    /// # Errors
    /// Propagates the drive's own outcome, a tree that will not go back,
    /// a waiting item that will not serialise, and whatever a run handed
    /// down reports.
    pub(super) fn conclude(
        &mut self,
        site: Site,
        at: &Assignment,
        ending: Ending<'_>,
    ) -> Result<Dispatched, AxError> {
        let Ending {
            driven,
            mut raised,
            delegates,
        } = ending;
        let Site {
            lease,
            who,
            run_id,
            model,
            ..
        } = site;
        let (addr, model) = (at.addr.clone(), model.id.as_str());
        // The tree goes back whether the run finished or failed. What was
        // committed on its branch survives; what was not, does not.
        if let Some(held) = lease {
            memory::Worktrees::open(&self.city_root)
                .map_err(memory::MemoryError::into_ax)?
                .release(held)
                .map_err(memory::MemoryError::into_ax)?;
        }
        // An item that is waiting belongs in the inbox, not only in the
        // refusal the model saw.
        // Recorded against the run and the address that raised it, not
        // against the city: answering this item later has to be able to
        // find the work it was holding up.
        if self.tainted_arrival {
            // C15's marker bit, set where the reason for it is known. A
            // tainted item takes no policy and no delegate, so a run that
            // began with a stranger's text cannot have its approvals
            // waived by a rule somebody wrote for ordinary work.
            for item in raised.iter_mut() {
                item.tainted = true;
            }
        }
        for item in raised.iter() {
            let value = serde_json::to_value(item).map_err(|err| {
                AxError::failure(
                    AxCode::InvalidArgs,
                    "record a waiting item",
                    err.to_string(),
                )
            })?;
            let map = value.as_object().cloned().ok_or_else(|| {
                AxError::failure(
                    AxCode::InvalidArgs,
                    "record a waiting item",
                    "an approval item is an object",
                )
            })?;
            self.record_for(
                run_id,
                effect::Line {
                    who: who.to_owned(),
                    addr: addr.clone(),
                    kind: EventKind::ApprovalRequested,
                    data: Payload::new(map)?,
                },
            )?;
        }
        let frozen = driven?;
        let ending = frozen.completion().clone();
        // What it actually did, for the person reading afterwards. The
        // ledger holds the detail; this line is the pointer into it.
        self.note(
            runtime::diagnostics::Level::Effect,
            "runtime::run",
            &format!("dispatch at {} finished on {}", addr.as_str(), model),
        );
        // Who this run handed work to. Started here rather than inside
        // the tool call, because a run is built by this layer and a tool
        // that drove one would be driving a run from inside another
        // run's tool bench. Each child is dispatched at `Delegated`, so
        // the gate refuses the grand-delegate without anybody having to
        // work out their own depth.
        //
        // A cancelled run hands nothing down. The fourth safe point is
        // what makes that reachable: a cancel arriving after the last
        // wave used to have no boundary left to land on, so work asked
        // for by a turn nobody wanted started anyway.
        let handed = match ending {
            kernel::Completion::Cancelled => Vec::new(),
            _ => delegates.borrow_mut().take(),
        };
        for work in handed {
            self.note(
                runtime::diagnostics::Level::Effect,
                "collab::delegate",
                &format!("{} handed work to {}", addr.as_str(), work.room.as_str()),
            );
            // Carried rather than defaulted, for the reason `knock`
            // states next to its own `budget`: work handed down is the
            // same piece of work, so it is done under the same ceiling.
            // Defaulting here told a delegate its budget was zero while
            // its parent had been told the truth.
            let child = self.dispatch_in(
                Assignment {
                    addr: work.room,
                    session: None,
                    effort: None,
                    mode: at.mode,
                    budget: at.budget,
                    parent: Some(run_id),
                },
                work.task,
                work.goal,
            )?;
            self.deliver_handback(&addr, &child)?;
        }
        Ok(Dispatched {
            run: run_id,
            addr,
            who,
            completion: ending,
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

    #[test]
    fn an_allowed_item_carries_the_work_on_instead_of_asking_for_the_command_again() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let (base_url, _provider) = fake_openai(&["m-local"], vec![completion("thinking", None)]);
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        let addr = Address::parse("lab/room1").unwrap();
        worker
            .handle(channels::Command::Dispatch {
                addr: addr.clone(),
                task: "empty the archive".to_owned(),
                goal: "one sweep, then stop".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"first"),
                session: None,
                effort: None,
            })
            .unwrap();

        // The run that just ran asked for something the person has to
        // answer. Recorded the way the bench records it: against the run
        // and the address, because answering it later has to find the
        // work it was holding up.
        let run = {
            let verified = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
            let mut found = None;
            for line in verified.raw_lines() {
                let record = EventRecord::parse_line(line).unwrap();
                if record.kind() == EventKind::RunStarted {
                    found = Some(record.run());
                }
            }
            found.expect("a dispatch starts a run")
        };
        let item = kernel::ApprovalItem {
            id: kernel::ApprovalId::new("item-1").unwrap(),
            source: kernel::ApprovalSource::Gate,
            actor: "lab/room1".to_owned(),
            action_desc: "empty the archive".to_owned(),
            artifact: Locator::parse("file:lab/room1@0000000000000000000000000000000000000000")
                .unwrap(),
            cluster_key: kernel::ClusterKey {
                class: kernel::ApprovalClass::DiscardEscalate,
                detail: "lab/room1".to_owned(),
            },
            created: TimeMs::new(1),
            tainted: false,
        };
        let value = serde_json::to_value(&item).unwrap();
        worker
            .record_for(
                run,
                effect::Line {
                    who: "lab/room1".to_owned(),
                    addr: addr.clone(),
                    kind: EventKind::ApprovalRequested,
                    data: Payload::new(value.as_object().cloned().unwrap()).unwrap(),
                },
            )
            .unwrap();
        worker.governance.pending.insert("item-1".to_owned(), item);

        let before = started_runs(&report.ledger_dir);
        worker
            .handle(channels::Command::Approve {
                item: kernel::ApprovalId::new("item-1").unwrap(),
                verdict: kernel::PolicyVerdict::Allow,
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"approve"),
            })
            .unwrap();

        assert_eq!(
            started_runs(&report.ledger_dir),
            before + 1,
            "answering carries the work on; it does not wait for the command to be typed again"
        );
        let verified = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
        let history: String = verified
            .raw_lines()
            .iter()
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .collect::<Vec<String>>()
            .join("\n");
        assert!(
            history.contains("\"cluster\""),
            "the answer names the group it answered, so a restart knows what was allowed"
        );
    }

    fn started_runs(ledger_dir: &Path) -> usize {
        let verified = runtime::replay::verify_ledger_dir(ledger_dir).unwrap();
        verified
            .raw_lines()
            .iter()
            .filter(|line| {
                EventRecord::parse_line(line)
                    .map(|record| record.kind() == EventKind::RunStarted)
                    .unwrap_or(false)
            })
            .count()
    }

    /// A run that hands work down starts a real second run, and that
    /// run cannot hand work down again.
    ///
    /// `kernel::gate::spawn` had held the one-level rule since S2 with
    /// no caller in production; this is the caller. The child is
    /// dispatched after the parent's turn settles rather than inside the
    /// tool call, because a tool that drove a run would be driving one
    /// from inside another run's tool bench.
    #[test]
    fn work_handed_down_becomes_a_run_that_cannot_hand_it_down_again() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let (base_url, _provider) = fake_openai(
            &["m-local"],
            vec![
                completion_with(
                    "handing it down",
                    "delegate",
                    "tu_1",
                    serde_json::json!({
                        "room": "lab/helper",
                        "task": "measure the thing",
                        "goal": "a number, then stop",
                    }),
                ),
                completion("waiting on a person", None),
                completion_with(
                    "handing it down",
                    "delegate",
                    "tu_2",
                    serde_json::json!({
                        "room": "lab/helper",
                        "task": "measure the thing",
                        "goal": "a number, then stop",
                    }),
                ),
                completion("done", None),
            ],
        );
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        worker
            .handle(channels::Command::CreateBuilding {
                addr: Address::parse("lab").unwrap(),
                template: channels::TemplateName::parse("minimal").unwrap(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"create"),
            })
            .unwrap();
        worker
            .handle(channels::Command::Dispatch {
                addr: Address::parse("lab/room1").unwrap(),
                task: "get it measured".to_owned(),
                goal: "the number is written down, then stop".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"dispatch"),
                session: None,
                effort: None,
            })
            .unwrap();
        // Nothing has been handed down yet: the first spawn of a run
        // waits for the person, and answering carries the work on.
        assert!(
            !city::job_path(dir.path(), &Address::parse("lab/helper").unwrap()).exists(),
            "a delegate started before anybody allowed it"
        );
        let cluster = allow_the_one_pending_item(&mut worker);
        assert_eq!(cluster.class, kernel::ApprovalClass::Delegation);
        assert_eq!(
            cluster.detail, "lab/room1",
            "the person is asked once per resident, not once per room it picks"
        );

        let verified = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
        let history: String = verified
            .raw_lines()
            .iter()
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .collect::<Vec<String>>()
            .join("\n");
        assert!(
            history.contains("lab/helper"),
            "the delegate's own run started in the room it was given"
        );
        assert!(
            city::job_path(dir.path(), &Address::parse("lab/helper").unwrap()).exists(),
            "and it was given a task file of its own"
        );
        // The second run asked to hand work down in turn, and the gate
        // that had never been called refused it.
        assert!(
            history.contains("E_DELEGATION_DEPTH") || !history.contains("lab/grandchild"),
            "a delegate that delegated would have opened a third room"
        );
        // The way back. Before this, a delegate's result reached the
        // ledger and never the run that asked for it, and a person had
        // to watch the live page to find out.
        assert!(
            history.contains("handback"),
            "what came back is waiting in the room that asked: {history}"
        );
        assert!(
            history.contains("lab/helper finished"),
            "the handback names the room the work was done in"
        );
    }

    /// The result of handed-down work is waiting in the asking room's
    /// inbox, which is what `status.signals_pending` counts and what the
    /// `signal` tool takes. The parent's own run is frozen by the time a
    /// child starts - only the assembly layer can build a run, and it
    /// gets control back after the parent's last turn - so the way back
    /// crosses runs, and the door that already does that is the room's
    /// queue.
    #[test]
    fn what_came_back_from_a_delegate_waits_in_the_room_that_asked_for_it() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        let (base_url, _provider) = fake_openai(
            &["m-local"],
            vec![
                // Asked, refused pending, gave up. The person then
                // allows it, the work is dispatched again, and the
                // second ask goes through.
                completion_with(
                    "handing it down",
                    "delegate",
                    "tu_1",
                    serde_json::json!({
                        "room": "lab/helper",
                        "task": "measure the thing",
                        "goal": "a number, then stop",
                    }),
                ),
                completion("waiting on a person", None),
                completion_with(
                    "handing it down",
                    "delegate",
                    "tu_2",
                    serde_json::json!({
                        "room": "lab/helper",
                        "task": "measure the thing",
                        "goal": "a number, then stop",
                    }),
                ),
                completion_with("where did it go", "status", "tu_3", serde_json::json!({})),
                completion("done", None),
            ],
        );
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        worker
            .handle(channels::Command::CreateBuilding {
                addr: Address::parse("lab").unwrap(),
                template: channels::TemplateName::parse("minimal").unwrap(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"create"),
            })
            .unwrap();
        let room = Address::parse("lab/room1").unwrap();
        worker
            .handle(channels::Command::Dispatch {
                addr: room.clone(),
                task: "get it measured".to_owned(),
                goal: "the number is written down, then stop".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"dispatch"),
                session: None,
                effort: None,
            })
            .unwrap();
        allow_the_one_pending_item(&mut worker);

        let waiting = worker
            .inboxes
            .get(&room)
            .expect("the asking room has a queue")
            .pending();
        assert_eq!(
            waiting, 1,
            "exactly one handback per piece of work handed down"
        );
        let taken = worker
            .inboxes
            .get_mut(&room)
            .expect("the asking room has a queue")
            .pull()
            .unwrap();
        let body = taken[0].payload().as_map();
        assert_eq!(body["room"], "lab/helper");
        assert_eq!(
            taken[0].from(),
            "lab/helper",
            "the handback comes from the delegate, not from the city"
        );
        assert!(
            body["at"].as_str().unwrap().starts_with("cas:b3-"),
            "the account is pinned before it is judged"
        );
    }

    /// `status.children` was a hardcoded empty list, so the one field a
    /// run could have used to check what it had handed down always said
    /// "none".
    #[test]
    fn status_tells_a_run_where_the_work_it_handed_down_went() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let (base_url, _provider) = fake_openai(
            &["m-local"],
            vec![
                completion_with(
                    "handing it down",
                    "delegate",
                    "tu_1",
                    serde_json::json!({
                        "room": "lab/helper",
                        "task": "measure the thing",
                        "goal": "a number, then stop",
                        "kind": "ephemeral",
                    }),
                ),
                completion("waiting on a person", None),
                completion_with(
                    "handing it down",
                    "delegate",
                    "tu_2",
                    serde_json::json!({
                        "room": "lab/helper",
                        "task": "measure the thing",
                        "goal": "a number, then stop",
                        "kind": "ephemeral",
                    }),
                ),
                completion_with("where did it go", "status", "tu_3", serde_json::json!({})),
                completion("done", None),
            ],
        );
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        worker
            .handle(channels::Command::CreateBuilding {
                addr: Address::parse("lab").unwrap(),
                template: channels::TemplateName::parse("minimal").unwrap(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"create"),
            })
            .unwrap();
        worker
            .handle(channels::Command::Dispatch {
                addr: Address::parse("lab/room1").unwrap(),
                task: "get it measured".to_owned(),
                goal: "the number is written down, then stop".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"dispatch"),
                session: None,
                effort: None,
            })
            .unwrap();
        allow_the_one_pending_item(&mut worker);

        let verified = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
        let history: String = verified
            .raw_lines()
            .iter()
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .collect::<Vec<String>>()
            .join("\n");
        assert!(
            history.contains("children: lab/helper (ephemeral)"),
            "the status a model read did not name the work it had just handed down: {history}"
        );
        // And the child's own run says whose work it is, which is what
        // the interface folds into a tree.
        assert!(
            history.contains(r#""parent":"#),
            "run_started carries no parent: {history}"
        );
    }
}
