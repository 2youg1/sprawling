// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The verbs a person sends, and what each one does to the city.

use kernel::{Address, AxCode, AxError, EventKind};
use kernel::{Ledger, Locator, Payload, RunId, TimeMs};

use super::{
    Assignment, Ceilings, Chosen, Entered, HALTED, RELEASED, RunWorker, autonomy_name, ledger_dir,
    mode_of, not_built, now_ms, run_id_for, scope_name,
};

impl RunWorker {
    pub(super) fn run_command(&mut self, command: channels::Command) -> Result<(), AxError> {
        match command {
            channels::Command::Dispatch {
                addr,
                task,
                goal,
                mode,
                session,
                effort,
                budget,
                ..
            } => {
                // An empty goal is not a missing field. It already means
                // something here, and the meaning is better than the one
                // a filled-in copy of the task would carry: no job file
                // is written and the prefix tells the model a person is
                // at the other end. That is exactly the shape a single
                // sentence typed into the composer has, so the composer
                // sends the goal empty and this reads it as it always
                // did.
                let session = self.session_for(&addr, session, &task)?;
                let addr = self.room_for(addr, session.as_ref())?;
                // Written into the session's own layer rather than held
                // beside the run: the ladder that resolves city ->
                // building -> room is already the authority on how hard
                // a run thinks, and a second store would be a second
                // answer. Chosen once, it holds for every later run in
                // that room.
                if let Some(effort) = effort {
                    city::write_effort(&self.city_root, &addr, effort)?;
                }
                self.dispatch_in(
                    Assignment {
                        addr,
                        mode: mode_of(&mode),
                        budget,
                        parent: None,
                    },
                    task,
                    goal,
                )
                .map(drop)?;
                // Whoever this run spoke to answers next, and whoever
                // they speak to after that. The person asked for one
                // dispatch; what returns to them is the conversation it
                // started, finished.
                self.answer_knocks();
                Ok(())
            }
            channels::Command::Wake {
                source,
                subject,
                body,
                ..
            } => self.wake(&source, &subject, &body),
            channels::Command::ConfigureBuilding {
                addr, sandbox, mcp, ..
            } => self.configure_building(&addr, sandbox.as_ref(), mcp.as_deref()),
            channels::Command::ProbeEndpoint {
                name,
                base_url,
                dialect,
                secret,
                auth_header,
                ..
            } => self.probe_endpoint(Entered {
                name: name.as_str().to_owned(),
                base_url,
                dialect,
                secret,
                auth_header,
            }),
            channels::Command::AttachEndpoint {
                name,
                base_url,
                dialect,
                secret,
                auth_header,
                admit,
                ..
            } => self.attach_endpoint(
                Entered {
                    name: name.as_str().to_owned(),
                    base_url,
                    dialect,
                    secret,
                    auth_header,
                },
                &admit,
            ),
            channels::Command::SelectModel {
                endpoint,
                model,
                tag,
                context_tokens,
                max_output_tokens,
                ..
            } => self.select_model(
                Chosen {
                    endpoint: endpoint.as_str().to_owned(),
                    model,
                    tag,
                },
                Ceilings {
                    context_tokens,
                    max_output_tokens,
                },
            ),
            channels::Command::PutSecret { realm, name, value } => {
                self.put_secret(realm, name, value)
            }
            channels::Command::Login { provider, step, .. } => self.login(provider.as_str(), step),
            channels::Command::CreateBuilding { addr, template, .. } => {
                self.create_building(addr, template.as_str())
            }
            channels::Command::Approve { item, verdict, .. } => {
                // The control surface is the person's entrance, so the
                // answerer is a human here by construction. A resident
                // answering as a delegate arrives with the tool that
                // lets it, and takes the same door.
                self.answer_approval(&item, verdict, &kernel::Answerer::Human)
            }
            channels::Command::SetAutonomy {
                scope, autonomy, ..
            } => self.set_autonomy(&scope, autonomy),
            channels::Command::Pursue { addr, step, .. } => self.set_pursuit(&addr, step),
            channels::Command::Fork {
                run, at_seq, addr, ..
            } => self.fork(run, at_seq, addr).map(|_| ()),
            channels::Command::Halt { scope, .. } => self.set_admission(&scope, HALTED),
            channels::Command::Release { scope, .. } => self.set_admission(&scope, RELEASED),
            // Cancel and Steer have a second door. `Desk::interrupt_for`
            // lifts them off the queue at the next safe point of the run
            // they name, so arriving here means no run answered - which
            // is what the refusal says, instead of naming the verb.
            channels::Command::Cancel { run, .. } => Err(AxError::failure(
                AxCode::InvalidArgs,
                "cancel a run",
                run.to_string(),
            )
            .with_recovery(
                "no run in flight answers to that id: it has already finished, or it never started",
            )),
            channels::Command::Steer { run, .. } => Err(AxError::failure(
                AxCode::InvalidArgs,
                "steer a run",
                run.to_string(),
            )
            .with_recovery(
                "no run in flight answers to that id: steer one while it runs, or dispatch a new one",
            )),
            // Six verbs the wire spells and this city cannot perform.
            // Answered one at a time rather than by a catch-all, so that
            // a Command added without an executor stops the build here:
            // `channels::Command` is deliberately not `non_exhaustive`,
            // and this match is what that decision buys.
            channels::Command::Takeover { run, .. } => Err(not_built(
                "take over a run",
                run.to_string(),
                "steer the run instead; taking the wheel from it is not built",
            )),
            channels::Command::Rollback { checkpoint, .. } => Err(not_built(
                "roll a checkpoint back",
                checkpoint.to_string(),
                "the checkpoint stands and its contents are readable; undoing it is not built",
            )),
            channels::Command::CreatePolicy { from_item, .. } => Err(not_built(
                "turn an answer into a policy",
                from_item.as_str().to_owned(),
                "answer each request as it arrives; standing policies are not built",
            )),
            channels::Command::Attach { upload, .. } => Err(not_built(
                "attach an upload to a run",
                upload.as_str().to_owned(),
                "paste the text into the task instead; attaching a file is not built",
            )),
            channels::Command::BatchByBuilding { addr, .. } => Err(not_built(
                "run a building's work as one batch",
                addr.as_str().to_owned(),
                "dispatch the rooms one at a time; batching a building is not built",
            )),
            // The handshake is where a peer proves who it is: `Hello`
            // carries the pairing token and `channels::server` judges it
            // before any command is read. A second door for the same
            // question would be a second authority on it.
            channels::Command::Auth { .. } => Err(not_built(
                "authenticate over the command channel",
                "Auth".to_owned(),
                "the pairing token is proved in the handshake, not in a command",
            )),
        }
    }

    /// Shuts a scope to new work, or opens it again.
    ///
    /// Halting is admission control and nothing else: what is already
    /// running keeps running, because stopping a run in flight is
    /// `Cancel` and one verb that did two things would leave a person
    /// unable to ask for either alone. The refusal a halted city gives a
    /// dispatch says which scope refused and how to open it.
    fn set_admission(&mut self, scope: &channels::HaltScope, state: &str) -> Result<(), AxError> {
        let name = scope_name(scope);
        let mut map = serde_json::Map::new();
        map.insert("scope".to_owned(), serde_json::Value::String(name.clone()));
        map.insert(
            "state".to_owned(),
            serde_json::Value::String(state.to_owned()),
        );
        // Recorded and nothing else: the fold reads `city_halted` and
        // sets the scope, in the one place a restart reads it too.
        self.record(EventKind::CityHalted, Payload::new(map)?)
    }

    /// Which shut scope covers this address, if one does.
    ///
    /// The city covers everything; a building or a workshop covers what
    /// is inside it, by the same containment `WriteDomain` uses, so
    /// "inside" means one thing in this city rather than two.
    pub(super) fn halted_by(&self, addr: &Address) -> Option<String> {
        if self.governance.halted.contains("city") {
            return Some("city".to_owned());
        }
        self.governance
            .halted
            .iter()
            .find(|name| {
                name.split_once(':')
                    .and_then(|(_, rest)| Address::parse(rest).ok())
                    .is_some_and(|scope| addr.is_within(&scope))
            })
            .cloned()
    }

    /// Records who may answer for a scope from now on.
    ///
    /// The current value is folded from the ledger rather than mirrored
    /// anywhere: what the panel shows is what a replay can verify.
    fn set_autonomy(
        &mut self,
        scope: &channels::HaltScope,
        autonomy: kernel::Autonomy,
    ) -> Result<(), AxError> {
        let mut map = serde_json::Map::new();
        map.insert(
            "scope".to_owned(),
            serde_json::Value::String(scope_name(scope)),
        );
        map.insert(
            "autonomy".to_owned(),
            serde_json::Value::String(autonomy_name(&autonomy)),
        );
        self.record(EventKind::AutonomyChanged, Payload::new(map)?)
    }

    /// Answers one approval item.
    ///
    /// The rule lives in `kernel::approval`: humans answer everything, a
    /// resident answers only as the appointed delegate, never the three
    /// classes, never a tainted item, and never its own action. This
    /// method is where production consults it, so a delegate answering
    /// its own item is refused on the same path the person's answer
    /// takes rather than on a parallel one.
    pub(super) fn answer_approval(
        &mut self,
        item: &kernel::ApprovalId,
        verdict: kernel::PolicyVerdict,
        answerer: &kernel::Answerer,
    ) -> Result<(), AxError> {
        let pending = self
            .governance
            .pending
            .get(item.as_str())
            .cloned()
            .ok_or_else(|| {
                AxError::failure(
                    AxCode::InvalidArgs,
                    "answer an approval",
                    item.as_str().to_owned(),
                )
                .with_recovery("this item is not waiting; the inbox lists the ones that are")
            })?;
        match kernel::may_answer(&self.governance.autonomy, &pending, answerer) {
            kernel::AnswerVerdict::May => {}
            refused => {
                return Err(AxError::failure(
                    AxCode::ApprovalDenied,
                    "answer an approval",
                    format!("{}: {refused:?}", item.as_str()),
                )
                .with_recovery(
                    "a resident answers only as the appointed delegate, and never its own \
                     action; this one is the person's to answer",
                ));
            }
        }
        let mut map = serde_json::Map::new();
        map.insert(
            "id".to_owned(),
            serde_json::Value::String(item.as_str().to_owned()),
        );
        map.insert(
            "verdict".to_owned(),
            serde_json::Value::String(format!("{verdict:?}").to_lowercase()),
        );
        // The cluster travels with the answer. The person was shown a
        // group and answered the group, so what a resumed run may do
        // without asking again is exactly that group — and reading it
        // back from the ledger is what makes the answer survive a
        // restart.
        map.insert(
            "cluster".to_owned(),
            serde_json::to_value(&pending.cluster_key).map_err(|err| {
                AxError::failure(AxCode::InvalidArgs, "record an answer", err.to_string())
            })?,
        );
        // Read before the answer is recorded, because recording it is
        // what closes the item: the fold drops the origin along with the
        // pending entry, and carrying the work on is this method's job
        // rather than the record's.
        let blocked = self.governance.origins.get(item.as_str()).cloned();
        self.record(EventKind::ApprovalResolved, Payload::new(map)?)?;
        // The cluster the person allowed and the closing of the item are
        // both folded from the line just written. Setting either field
        // here as well would be a second authority for a rule the fold
        // already holds.
        if verdict == kernel::PolicyVerdict::Allow
            && let Some(job) = blocked
        {
            // The work the person just unblocked carries on without
            // them: an answer that still needed the same command typed
            // again would make the inbox a place to acknowledge things
            // rather than a place to decide them. Under the ceiling it
            // was sent with, for the reason `knock` gives beside its own
            // budget - this is the same piece of work, interrupted.
            self.dispatch_in(
                Assignment {
                    addr: job.addr,
                    mode: runtime::Mode::PlanGoal,
                    budget: job.budget,
                    parent: None,
                },
                job.task,
                job.goal,
            )
            .map(drop)?;
        }
        Ok(())
    }

    /// Starts whatever the schedule says should have started since the
    /// last tick, and returns how many runs that was.
    ///
    /// `now` arrives as a parameter, so a test drives a year of
    /// schedule in three calls and the city behaves the same way it
    /// would over a real year. A fresh worker begins owing from the
    /// moment it opened: a city that was off does not spend its first
    /// minute running yesterday, and the ledger says when it woke.
    ///
    /// # Errors
    /// Propagates the schedule's own refusal to parse, and the first
    /// dispatch that fails - a scheduled run that cannot start is not
    /// swallowed just because nobody typed it.
    pub(crate) fn tick(&mut self, now: TimeMs) -> Result<u32, AxError> {
        let schedule = city::Schedule::load(&self.city_root)?;
        let due: Vec<(Address, String, String)> = schedule
            .due(self.last_tick, now)
            .into_iter()
            .map(|entry| {
                (
                    entry.addr().clone(),
                    entry.task().to_owned(),
                    entry.goal().to_owned(),
                )
            })
            .collect();
        self.last_tick = now;
        let mut started: u32 = 0;
        for (addr, task, goal) in due {
            self.dispatch(addr, task, goal)?;
            started = started.saturating_add(1);
        }
        Ok(started)
    }

    /// Records a fork: a new run identity branched from `from` at the
    /// event node `at_seq`. The lineage is the record; driving the new
    /// run is a Dispatch the person (or the interface) sends when ready.
    /// Prefix semantics are the replay layer's (`runtime::fork::prefix`);
    /// this method refuses a node the mother does not own.
    ///
    /// # Errors
    /// Refuses a node `from` does not own, and propagates whatever chain
    /// verification or the prefix bound reports.
    pub fn fork(
        &mut self,
        from: RunId,
        at_seq: kernel::Seq,
        addr: Option<Address>,
    ) -> Result<RunId, AxError> {
        let verified = runtime::replay::verify_ledger_dir(&ledger_dir(&self.city_root))?;
        // Validates the bound the same way a prefix build would.
        let _prefix = runtime::fork::prefix(&verified, at_seq)?;
        let index = usize::try_from(at_seq.value()).map_err(|_| {
            AxError::failure(
                AxCode::InvalidArgs,
                "fork",
                "at_seq does not fit this platform",
            )
        })?;
        let node_owner = verified.lines().get(index).and_then(|line| match line {
            runtime::replay::VerifiedLine::Known { record, .. } => Some(record.run()),
            _ => None,
        });
        if node_owner != Some(from) {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "fork",
                format!("seq {} is not an event of run {from}", at_seq.value()),
            )
            .with_recovery("name an event node of the run you are forking"));
        }
        let fork_addr = match addr {
            Some(addr) => addr,
            None => verified
                .lines()
                .iter()
                .find_map(|line| match line {
                    runtime::replay::VerifiedLine::Known { record, .. }
                        if record.run() == from && record.kind() == EventKind::RunStarted =>
                    {
                        record.addr().cloned()
                    }
                    _ => None,
                })
                .ok_or_else(|| {
                    AxError::failure(
                        AxCode::InvalidArgs,
                        "fork",
                        format!("{from} has no run_started in this ledger"),
                    )
                    .with_recovery("name the address to fork into")
                })?,
        };
        let now = now_ms()?;
        let new_run = run_id_for(
            &Locator::parse(&format!(
                "cas:b3-{}",
                kernel::B3Hash::digest(from.as_bytes())
            ))?,
            &fork_addr,
            now,
        );
        let draft = runtime::fork::fork_draft(from, at_seq, new_run, now, "owner".to_owned())?;
        self.ledger.append(draft)?;
        Ok(new_run)
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
    use crate::serving::{CommandDesk, DeskWait};
    use crate::views::Views;

    #[test]
    fn an_outside_editor_asks_for_work_and_a_stranger_learns_one_bit() {
        let desk = CommandDesk::new();
        let body = |addr: &str| channels::AcpBody {
            token: "pair-me".to_owned(),
            addr: addr.to_owned(),
            task: "read the plan".to_owned(),
            goal: "one answer".to_owned(),
        };

        let err = acp_dispatch(&desk, body("lab/room1"), false).unwrap_err();
        assert_eq!(err.code(), &AxCode::GateDenied);
        assert!(
            !err.subject().contains("lab"),
            "an unauthenticated caller learns one bit and not whether the address exists: {}",
            err.subject()
        );
        assert!(desk.take().is_none(), "and nothing was queued for it");

        // The city's own subtree is not a room, with or without a token.
        let err = acp_dispatch(&desk, body(".sprawling/ledger"), true).unwrap_err();
        assert_eq!(err.code(), &AxCode::OutsideWriteDomain);
        assert!(desk.take().is_none());

        let progress = acp_dispatch(&desk, body("lab/room1"), true).unwrap();
        assert!(!progress.finished);
        assert_eq!(progress.turns, 0);
        let Some(channels::Command::Dispatch { addr, task, .. }) = desk.take() else {
            panic!("an admitted request becomes the dispatch a person would have sent");
        };
        assert_eq!(addr.as_str(), "lab/room1");
        assert_eq!(task, "read the plan");
    }

    /// The defect this card exists for: a person presses a button, the
    /// city refuses, and the page says nothing at all. The refusal has
    /// to arrive at whoever caused it - found by driving a real city
    /// over its own wire, where two refused commands appeared in the
    /// server's log file and nowhere else.
    #[test]
    fn a_refused_command_reaches_the_peer_that_sent_it() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        let mut worker = RunWorker::new(
            dir.path(),
            gateway::Custodian::in_memory(),
            runtime::diagnostics::Diagnostics::off(),
        )
        .unwrap();

        let heard: Arc<std::sync::Mutex<Vec<AxError>>> =
            Arc::new(std::sync::Mutex::new(Vec::new()));
        let peer = Arc::clone(&heard);
        let reply = channels::Reply::to(move |error| {
            let Ok(mut heard) = peer.lock() else {
                return channels::Delivered::PeerGone;
            };
            heard.push(error);
            channels::Delivered::ToThePeer
        });

        // A model chosen on an endpoint that was never attached: the
        // same shape as the real failure, where a base URL missing its
        // `/v1` made the attach fail and the model selection fail after
        // it, and the page reported neither.
        worker.serve_one(Posted {
            command: channels::Command::SelectModel {
                endpoint: channels::ProviderName::parse("nowhere").unwrap(),
                model: "a-model".to_owned(),
                tag: kernel::ModelTag::Main,
                context_tokens: 200_000,
                max_output_tokens: 8_192,
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"select"),
            },
            reply,
        });

        let heard = heard.lock().unwrap();
        assert_eq!(heard.len(), 1, "the peer is told once, and told at all");
        let told = heard.first().unwrap();
        assert!(
            !told.recovery().is_empty(),
            "a refusal that reaches a person carries the way out: {told}"
        );
    }

    /// A command nobody sent - the schedule's own - is refused into a
    /// reply address that names the absence, rather than into a peer
    /// that would have to be invented for it.
    #[test]
    fn a_refusal_with_no_one_behind_it_says_so_rather_than_failing() {
        let nobody = channels::Reply::nowhere();
        let outcome = nobody.refuse(AxError::failure(
            AxCode::ConfigInvalid,
            "read the schedule",
            "the file is not a schedule",
        ));
        assert_eq!(outcome, channels::Delivered::NobodyAsked);
    }

    #[test]
    fn a_scheduled_job_starts_by_itself_and_only_once_per_firing() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        std::fs::write(
            city::schedule_path(dir.path()),
            "[[job]]\nname = \"sweep\"\naddr = \"lab/room1\"\n\
             task = \"sweep the roadmap\"\ngoal = \"every row has a status\"\n\
             every = \"15m\"\n",
        )
        .unwrap();
        let (base_url, _provider) = fake_openai(&["m-local"], vec![completion("done", None)]);
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();

        // Time arrives as a parameter, so an hour passes in two calls.
        let minute = 60_000;
        worker.last_tick = kernel::TimeMs::new(14 * minute);
        assert_eq!(worker.tick(kernel::TimeMs::new(15 * minute)).unwrap(), 1);
        assert_eq!(
            worker.tick(kernel::TimeMs::new(16 * minute)).unwrap(),
            0,
            "a firing already served does not come round again inside its period"
        );
        assert_eq!(
            worker.tick(kernel::TimeMs::new(90 * minute)).unwrap(),
            1,
            "an hour of downtime owes one run, not four"
        );

        let verified = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
        let started = verified
            .raw_lines()
            .iter()
            .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
            .filter(|value| value["kind"] == "run_started")
            .count();
        assert_eq!(started, 2, "two firings, two runs, and both in the history");
    }

    #[test]
    fn an_answer_lands_in_the_history_and_a_delegate_cannot_answer_its_own_action() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let mut worker = RunWorker::new(
            dir.path(),
            gateway::Custodian::in_memory(),
            runtime::diagnostics::Diagnostics::off(),
        )
        .unwrap();

        let item = kernel::ApprovalItem {
            id: kernel::ApprovalId::new("item-1").unwrap(),
            source: kernel::ApprovalSource::Agent,
            actor: "lab/room1".to_owned(),
            action_desc: "send the release mail".to_owned(),
            artifact: Locator::parse(&format!("cas:b3-{}", "ab".repeat(32))).unwrap(),
            cluster_key: kernel::ClusterKey {
                class: kernel::ApprovalClass::AgentQuestion,
                detail: "mail:release".to_owned(),
            },
            created: kernel::TimeMs::new(1_000),
            tainted: false,
        };
        let payload = Payload::new(
            serde_json::to_value(&item)
                .unwrap()
                .as_object()
                .cloned()
                .unwrap(),
        )
        .unwrap();
        worker
            .record(EventKind::ApprovalRequested, payload)
            .unwrap();

        // The appointed delegate is the actor of this item, so the one
        // resident allowed to answer at all is barred from this one.
        worker
            .handle(channels::Command::SetAutonomy {
                scope: channels::HaltScope::City,
                autonomy: kernel::Autonomy::Delegate(kernel::ResidentId::new("lab/room1").unwrap()),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"autonomy"),
            })
            .unwrap();
        let err = worker
            .answer_approval(
                &kernel::ApprovalId::new("item-1").unwrap(),
                kernel::PolicyVerdict::Allow,
                &kernel::Answerer::Resident(kernel::ResidentId::new("lab/room1").unwrap()),
            )
            .unwrap_err();
        assert_eq!(err.code(), &AxCode::ApprovalDenied);
        assert!(err.subject().contains("SelfApprovalBarred"));

        // The person answers it, and the history says so.
        worker
            .handle(channels::Command::Approve {
                item: kernel::ApprovalId::new("item-1").unwrap(),
                verdict: kernel::PolicyVerdict::Allow,
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"approve"),
            })
            .unwrap();

        let verified = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
        let history: String = verified
            .raw_lines()
            .iter()
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .collect::<Vec<String>>()
            .join("\n");
        assert!(history.contains("autonomy_changed"));
        assert!(history.contains("approval_resolved"));

        // And a second answer finds nothing waiting: the queue drains
        // from the ledger, not from a mirror of it.
        let err = worker
            .handle(channels::Command::Approve {
                item: kernel::ApprovalId::new("item-1").unwrap(),
                verdict: kernel::PolicyVerdict::Allow,
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"approve"),
            })
            .unwrap_err();
        assert!(err.recovery().contains("not waiting"));

        // A worker that restarts reads the same answer out of the ledger.
        let restarted = RunWorker::new(
            dir.path(),
            gateway::Custodian::in_memory(),
            runtime::diagnostics::Diagnostics::off(),
        )
        .unwrap();
        assert!(restarted.governance.pending.is_empty());
        assert_eq!(
            restarted.governance.autonomy,
            kernel::Autonomy::Delegate(kernel::ResidentId::new("lab/room1").unwrap())
        );
    }

    /// The wire promises that "double-clicking twice opens two Runs" is
    /// not reachable, and every client mints its key from what the
    /// person entered, so the same request twice is the same key twice.
    /// Nothing read that key until this desk did.
    #[test]
    fn a_repeat_of_a_command_already_underway_is_not_a_second_piece_of_work() {
        let desk = CommandDesk::new();
        let moment = std::time::Duration::from_millis(1);
        let asked = || channels::Command::Dispatch {
            addr: Address::parse("lab/room1").unwrap(),
            task: "read the plan".to_owned(),
            goal: "one answer".to_owned(),
            mode: channels::ModeTag::parse("plan").unwrap(),
            budget: kernel::BudgetCap::default(),
            idem: kernel::IdemKey::derive(
                &RunId::CITY,
                kernel::Seq::FIRST,
                b"lab/room1|read the plan",
            ),
            session: None,
            effort: None,
        };

        desk.post(asked(), channels::Reply::nowhere());
        desk.post(asked(), channels::Reply::nowhere());
        let carrying = desk.wait(moment);
        assert!(
            matches!(carrying, DeskWait::Command(..)),
            "the first ask is taken off the desk"
        );
        assert!(
            matches!(desk.wait(moment), DeskWait::Idle),
            "a second frame of the same ask is a second bill, not a second piece of work"
        );

        // A repeat that arrives while the work is going is the same ask
        // once more: the run it wants is already running.
        desk.post(asked(), channels::Reply::nowhere());
        assert!(
            matches!(desk.wait(moment), DeskWait::Idle),
            "the ask is still being carried out; a repeat adds nothing"
        );

        // ...and once it is over, asking again is asking for a second
        // run, which is a thing a person is allowed to want.
        drop(carrying);
        desk.post(asked(), channels::Reply::nowhere());
        assert!(
            matches!(desk.wait(moment), DeskWait::Command(..)),
            "the same work asked for again after it finished is work"
        );
    }

    /// Work already accepted finishes first: a close that dropped a
    /// queued command would make "stopped" and "lost" the same thing in
    /// the record.
    #[test]
    fn a_close_lands_between_commands_and_never_inside_one() {
        let desk = CommandDesk::new();
        desk.post(
            channels::Command::CreateBuilding {
                addr: Address::parse("lab").unwrap(),
                template: channels::TemplateName::parse("minimal").unwrap(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"create"),
            },
            channels::Reply::nowhere(),
        );
        desk.close();

        assert!(
            matches!(
                desk.wait(std::time::Duration::from_millis(1)),
                DeskWait::Command(..)
            ),
            "the queued command was dropped by the close"
        );
        assert!(matches!(
            desk.wait(std::time::Duration::from_millis(1)),
            DeskWait::Close
        ));
    }

    #[test]
    fn the_approval_queue_holds_what_was_asked_and_drops_what_was_answered() {
        let dir = tempfile::tempdir().unwrap();
        let mut views = Views::new(dir.path());
        // The payload is built the way the dispatch path builds it -
        // by serialising the item - rather than out of field names a
        // test invented. The invented shape was the defect: the view
        // read a `summary` key nobody ever wrote, so every waiting item
        // rendered as "(no summary recorded)" and no test noticed.
        let item = kernel::ApprovalItem {
            id: kernel::ApprovalId::new("a-1".to_owned()).unwrap(),
            source: kernel::ApprovalSource::Gate,
            actor: "urbanite-2".to_owned(),
            action_desc: "delete the archive".to_owned(),
            artifact: kernel::Locator::parse(
                "file:lab/room1@0000000000000000000000000000000000000000",
            )
            .unwrap(),
            cluster_key: kernel::ClusterKey {
                class: kernel::ApprovalClass::AgentQuestion,
                detail: "lab".to_owned(),
            },
            created: TimeMs::new(1),
            tainted: true,
        };
        let asked = serde_json::to_value(&item)
            .unwrap()
            .as_object()
            .unwrap()
            .clone();
        let requested = EventRecord::from_draft(
            EventDraft {
                run: RunId::CITY,
                t: TimeMs::new(1),
                who: "gate".to_owned(),
                addr: None,
                kind: EventKind::ApprovalRequested,
                data: Payload::new(asked).unwrap(),
                ig: false,
            },
            kernel::Seq::FIRST,
            kernel::GENESIS_PREV,
        );
        views.apply(&requested).unwrap();
        let channels::Answer::Approvals(queue) = views.answer(&channels::Query::ApprovalQueue)
        else {
            panic!("the approval queue answers with items");
        };
        assert_eq!(queue.items.len(), 1);
        assert!(queue.items[0].tainted);
        assert_eq!(
            queue.items[0], item,
            "what the queue answers is the item the ledger recorded, field for field"
        );
        assert_eq!(
            queue.items[0].cluster_key.detail, "lab",
            "the cluster key survives, or the inbox cannot group anything"
        );

        let mut answered = serde_json::Map::new();
        answered.insert("id".to_owned(), serde_json::Value::String("a-1".to_owned()));
        let resolved = EventRecord::from_draft(
            EventDraft {
                run: RunId::CITY,
                t: TimeMs::new(2),
                who: "owner".to_owned(),
                addr: None,
                kind: EventKind::ApprovalResolved,
                data: Payload::new(answered).unwrap(),
                ig: false,
            },
            kernel::Seq::FIRST.next().unwrap(),
            kernel::GENESIS_PREV,
        );
        views.apply(&resolved).unwrap();
        let channels::Answer::Approvals(queue) = views.answer(&channels::Query::ApprovalQueue)
        else {
            panic!("the approval queue answers with items");
        };
        assert!(queue.items.is_empty());
    }

    #[test]
    fn a_command_with_no_executor_is_refused_by_name_and_not_by_stage() {
        // What this replaced: one catch-all whose recovery read "this
        // stage runs Dispatch; the rest land with their cards", which is
        // a sentence about a build stage that ended. Eight commands
        // reached it, three of them from buttons this client used to
        // draw, and a person pressing one learned nothing they could act
        // on. The match is exhaustive now, so a command added without an
        // executor stops the build rather than reaching a person.
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        let mut worker = RunWorker::new(
            dir.path(),
            gateway::Custodian::in_memory(),
            runtime::diagnostics::Diagnostics::off(),
        )
        .unwrap();

        // Cancel has a second door - the desk lifts it off the queue at
        // the next safe point of the run it names - so arriving here
        // means no run answered, and the refusal says that rather than
        // naming the verb.
        let missing = worker
            .handle(channels::Command::Cancel {
                run: RunId::CITY,
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"cancel"),
            })
            .unwrap_err();
        assert_eq!(*missing.code(), AxCode::InvalidArgs);
        assert!(
            missing.recovery().contains("no run in flight"),
            "the refusal names the run, not the build stage: {}",
            missing.recovery()
        );
        assert!(!missing.recovery().contains("cards"));

        // A verb the wire spells and this city cannot perform. It says
        // so, and says what to do instead.
        let unbuilt = worker
            .handle(channels::Command::Takeover {
                run: RunId::CITY,
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"takeover"),
            })
            .unwrap_err();
        assert_eq!(*unbuilt.code(), AxCode::WireMismatch);
        assert!(
            unbuilt.recovery().contains("not built"),
            "an unbuilt verb says so: {}",
            unbuilt.recovery()
        );
        assert!(
            unbuilt.recovery().contains("Steer") || unbuilt.recovery().contains("steer"),
            "and names what to do instead: {}",
            unbuilt.recovery()
        );
    }
}
