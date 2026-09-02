// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! One building's plan: who holds what, and how far red reaches.

use kernel::{Address, AxCode, AxError, EventKind};
use kernel::{Payload, RunId};

use crate::effect;

use super::{Assignment, RunWorker, now_ms};

/// The run reporting a change to the plan: which run, of which
/// building, as whom. Three values that always travel together and are
/// never chosen independently, so they travel as one. The room is not
/// among them: it is the address the [`Assignment`] beside this one
/// already names, and two fields for one room are two chances to name
/// different ones.
#[derive(Clone, Copy)]
pub(super) struct Reporter<'a> {
    pub(super) run_id: RunId,
    pub(super) building: &'a Address,
    pub(super) who: &'a str,
}

impl RunWorker {
    /// Tells whoever is standing behind a node that has just gone red.
    ///
    /// **A fact starts this, not a person.** Every other way one
    /// resident reaches another in this city begins with somebody
    /// deciding to speak; this one begins with a node going red, and it
    /// reaches exactly the rooms holding work that cannot now move.
    ///
    /// The signal's id is derived from the building and the node, so a
    /// blockage announced twice is one signal: the inbox already
    /// deduplicates by id, and a room told four times about one problem
    /// is a room that stops reading its inbox.
    pub(super) fn tell_whoever_is_behind(
        &mut self,
        at: &Assignment,
        by: Reporter<'_>,
        effects: &[collab::ClaimEffect],
    ) -> Result<(), AxError> {
        let Reporter {
            run_id,
            building,
            who,
        } = by;
        let room = &at.addr;
        let red: Vec<kernel::RedNode> = effects
            .iter()
            .filter_map(|effect| match effect {
                collab::ClaimEffect::PutDown {
                    exit: kernel::PlanExit::Stopped { id, why },
                    ..
                } if why.is_red() => Some(kernel::RedNode {
                    at: id.clone(),
                    why: why.clone(),
                }),
                _ => None,
            })
            .collect();
        if red.is_empty() {
            return Ok(());
        }
        let Some(tree) = self.plan_of(building) else {
            return Ok(());
        };
        let held = self.holders_in(building);
        let mut sent = Vec::new();
        for notice in kernel::notices(&kernel::spread(&tree, &red), &held) {
            let Ok(to) = Address::parse(&notice.to) else {
                continue;
            };
            if &to == room {
                continue;
            }
            let mut payload = serde_json::Map::new();
            payload.insert(
                "text".to_owned(),
                serde_json::Value::String(notice.line.clone()),
            );
            sent.push(collab::SignalEffect::Enqueued(collab::Signal::new(
                collab::SignalId::parse(&format!(
                    "blocked-{}-{}",
                    building.as_str().replace('/', "-"),
                    notice.about
                ))?,
                collab::SignalKind::Mention,
                who.to_owned(),
                to,
                kernel::Version::FIRST,
                Payload::new(payload)?,
                now_ms()?,
            )?));
        }
        if sent.is_empty() {
            return Ok(());
        }
        let landing = effect::Landing::signals(sent, room, who)?;
        self.settle(at, run_id, landing)
    }

    /// Which room holds each node of a building's plan.
    ///
    /// Read from the plan and the claim records together: the table
    /// says which nodes are being worked on, and the records say from
    /// which room. A node marked `In progress` that no record claims is
    /// left out rather than guessed at.
    fn holders_in(&self, building: &Address) -> std::collections::BTreeMap<kernel::NodeId, String> {
        self.plan_holders.get(building).cloned().unwrap_or_default()
    }

    /// What could be started in one building right now.
    fn ready_in(&self, addr: &Address) -> Vec<kernel::NodeId> {
        self.plan_of(addr)
            .map(|tree| tree.ready())
            .unwrap_or_default()
    }

    /// One node's text, for the task a dispatch carries.
    fn plan_item(&self, addr: &Address, node: &kernel::NodeId) -> Option<String> {
        self.plan_of(addr)?
            .get(node)
            .map(|held| held.row.item.clone())
    }

    /// A building's plan as it stands on disk.
    ///
    /// Read here rather than folded: this runs once per dispatch, not
    /// once per question a page asks, and the worker writes the file it
    /// is reading.
    fn plan_of(&self, addr: &Address) -> Option<kernel::PlanTree> {
        let text = city::roadmap(&self.city_root, addr).ok()?;
        match kernel::check_roadmap_shape(&text) {
            kernel::RoadmapShape::WellFormed { rows } => kernel::PlanTree::build(rows).ok(),
            kernel::RoadmapShape::Malformed { .. } => None,
        }
    }

    /// Sets, pauses, resumes or clears a building's pursuit.
    ///
    /// **Held in the process and not written down.** A standing goal is
    /// a posture rather than a fact about the past, and a city that came
    /// back from a restart already working through the night is the one
    /// failure this must not have. What the goal *does* — every run it
    /// starts — is recorded like anything else.
    ///
    /// A goal is declared through the depth-zero position this Desk
    /// holds, which is the runtime half of the guard the type already
    /// carries: a sub-agent has no `Delegator` and cannot reach this
    /// function either.
    pub(super) fn set_pursuit(
        &mut self,
        addr: &Address,
        step: channels::PursuitStep,
    ) -> Result<(), AxError> {
        let missing = |action: &'static str| {
            AxError::failure(
                AxCode::InvalidArgs,
                action,
                format!("{} is not pursuing anything", addr.as_str()),
            )
            .with_recovery("set a goal first; there is nothing here to change")
        };
        let name = match step {
            channels::PursuitStep::Set { goal } => {
                // Declared through the depth-zero position this worker
                // holds. That is the runtime half of the guard the type
                // already carries: a sub-agent has no `Delegator`, and
                // no path from a tool reaches this function either.
                let declared = kernel::Pursuit::declare(&self.delegator, goal)?;
                self.note(
                    runtime::diagnostics::Level::Effect,
                    "kernel::pursuit",
                    &format!(
                        "{} works towards `{}` until nothing is ready",
                        addr.as_str(),
                        declared.goal()
                    ),
                );
                self.pursuits.insert(addr.clone(), declared);
                "set"
            }
            channels::PursuitStep::Pause => {
                self.pursuits
                    .get_mut(addr)
                    .ok_or_else(|| missing("pause a pursuit"))?
                    .pause();
                "pause"
            }
            channels::PursuitStep::Resume => {
                self.pursuits
                    .get_mut(addr)
                    .ok_or_else(|| missing("resume a pursuit"))?
                    .resume();
                "resume"
            }
            channels::PursuitStep::Clear => {
                self.pursuits
                    .remove(addr)
                    .ok_or_else(|| missing("clear a pursuit"))?;
                "clear"
            }
        };
        let mut map = serde_json::Map::new();
        map.insert(
            "step".to_owned(),
            serde_json::Value::String(name.to_owned()),
        );
        if let Some(held) = self.pursuits.get(addr) {
            map.insert(
                "goal".to_owned(),
                serde_json::Value::String(held.goal().to_owned()),
            );
        }
        self.record_at(EventKind::PursuitChanged, addr.clone(), Payload::new(map)?)?;
        self.pursue(addr)
    }

    /// Takes ready work for as long as a pursuit says to.
    ///
    /// **It terminates because every dispatch takes a node out of the
    /// ready set.** Claiming moves a node to `In progress`, and a run
    /// that ends still holding one leaves it blocked, so the set this
    /// reads from strictly shrinks — except when a run splits a branch,
    /// which is the city finding more work rather than looping.
    ///
    /// The verdict is `kernel::pursuit`'s and is not re-derived here:
    /// what "there is nothing left to do" means has one authority.
    fn pursue(&mut self, addr: &Address) -> Result<(), AxError> {
        loop {
            let Some(state) = self.pursuits.get(addr).map(kernel::Pursuit::state) else {
                return Ok(());
            };
            let ready = self.ready_in(addr);
            let goal = self
                .pursuits
                .get(addr)
                .map(|held| held.goal().to_owned())
                .unwrap_or_default();
            match kernel::observe_pursuit(state, &ready, 0) {
                kernel::PursuitVerdict::Work { next } => {
                    let Some(node) = self.plan_item(addr, &next) else {
                        return Ok(());
                    };
                    self.note(
                        runtime::diagnostics::Level::Effect,
                        "kernel::pursuit",
                        &format!("{} takes {next}: {node}", addr.as_str()),
                    );
                    self.dispatch_in(
                        Assignment {
                            addr: addr.clone(),
                            session: None,
                            effort: None,
                            mode: runtime::Mode::PlanGoal,
                            budget: kernel::BudgetCap::default(),
                            parent: None,
                        },
                        format!("Plan node {next}: {node}"),
                        goal,
                    )?;
                    // A dispatch that did not move the node would loop
                    // for ever, so the check is on the ready set itself
                    // rather than on a counter.
                    if self.ready_in(addr).contains(&next) {
                        self.note(
                            runtime::diagnostics::Level::Refuse,
                            "kernel::pursuit",
                            &format!(
                                "{next} is still ready after a run took it; the pursuit stops \
                                 rather than dispatching it again"
                            ),
                        );
                        return Ok(());
                    }
                }
                kernel::PursuitVerdict::Waiting { .. }
                | kernel::PursuitVerdict::Paused
                | kernel::PursuitVerdict::Finished => return Ok(()),
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
    use crate::assembly::fixture::*;
    use crate::assembly::*;

    /// A plan nobody could read and a plan somebody else changed are two
    /// different facts, and only one of them is the person's to fix.
    ///
    /// The distinction is not cosmetic: the claim path already refuses to
    /// overwrite a row that moved, and it reports that refusal by name.
    /// Reading the file as empty makes every claim look like it lost a
    /// race that never happened, which sends the person to ask a resident
    /// instead of to look at a file.
    #[test]
    fn a_plan_that_cannot_be_read_is_refused_by_name_rather_than_blamed_on_a_neighbour() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        let building = dir.path().join("lab");
        std::fs::create_dir_all(building.join("room1")).unwrap();
        lay_rules(
            dir.path(),
            "lab",
            "# BUILDING.md\n\n`confidential: false`\n",
        );
        // A directory where the plan belongs. `read_to_string` then fails
        // for a reason that is not "it is not there yet" - the one reason
        // an empty plan is the right answer to - without this test having
        // to negotiate file permissions with the host.
        let plan = building.join(city::ROADMAP_FILE);
        let _ = std::fs::remove_file(&plan);
        std::fs::create_dir_all(&plan).unwrap();

        let (base_url, _provider) =
            fake_openai(&["m-local"], vec![completion("nothing to do", None)]);
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        let outcome = worker.handle(channels::Command::Dispatch {
            addr: Address::parse("lab/room1").unwrap(),
            task: "claim a row".to_owned(),
            goal: "one claim".to_owned(),
            mode: channels::ModeTag::parse("plan").unwrap(),
            budget: kernel::BudgetCap::default(),
            idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"unreadable"),
            session: None,
            effort: None,
        });

        let err = outcome.expect_err("a plan nobody can read is not an empty plan");
        assert!(
            err.to_string().contains(city::ROADMAP_FILE),
            "the refusal has to name the file a person must fix: {err}"
        );
    }

    /// R2.10's other half: not only "the line comes before the change",
    /// but "no line, no change".
    ///
    /// The ledger here is the real `JsonlLedger` over the deterministic
    /// power-loss model, told to lose exactly the write carrying
    /// `roadmap_claimed`. Everything else in the city is on a real disk,
    /// which is the point: the plan the run wanted to rewrite is a file
    /// somebody could go and read afterwards.
    #[test]
    fn a_line_the_history_refused_is_a_change_the_city_never_made() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        let plan = dir.path().join("lab").join(city::ROADMAP_FILE);
        std::fs::create_dir_all(dir.path().join("lab")).unwrap();
        std::fs::write(&plan, PLAN_TWO_FREE_ROWS).unwrap();

        let (base_url, provider) = fake_openai(
            &["m-local"],
            vec![
                tool_completion(
                    "taking a row",
                    "tu_1",
                    "plan",
                    serde_json::json!({ "action": "claim", "node": "1" }),
                ),
                completion("took it", None),
            ],
        );

        // The one write that dies. Named by what it carries rather than
        // by an ordinal: how many filesystem operations run before it is
        // not something a caller up here knows, and any number written
        // here would stop meaning this line the moment anything upstream
        // read one more file.
        let fs = memory::FaultFs::new(memory::FaultPlan {
            cut_at_op: None,
            cut_on_write: Some("roadmap_claimed"),
            torn_tail: memory::TornTail::None,
        });
        let (ledger, _report) =
            memory::JsonlLedger::open_faulty(fs, &ledger_dir(dir.path()), now_ms().unwrap())
                .unwrap();
        let mut worker = RunWorker::over(
            dir.path(),
            gateway::Custodian::in_memory(),
            runtime::diagnostics::Diagnostics::off(),
            ledger,
        )
        .unwrap();
        worker
            .handle(channels::Command::AttachEndpoint {
                name: channels::ProviderName::parse("house").unwrap(),
                base_url,
                dialect: kernel::DialectKind::OpenAi,
                secret: None,
                auth_header: None,
                admit: Vec::new(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"attach"),
            })
            .unwrap();
        worker
            .handle(channels::Command::SelectModel {
                endpoint: channels::ProviderName::parse("house").unwrap(),
                model: "m-local".to_owned(),
                tag: kernel::ModelTag::Main,
                context_tokens: 32_768,
                max_output_tokens: 4_096,
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"select"),
            })
            .unwrap();

        let outcome = worker.handle(channels::Command::Dispatch {
            addr: Address::parse("lab/room1").unwrap(),
            task: "take one row".to_owned(),
            goal: "one claim".to_owned(),
            mode: channels::ModeTag::parse("plan").unwrap(),
            budget: kernel::BudgetCap::default(),
            idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"lost"),
            session: None,
            effort: None,
        });
        drop(provider);

        let after = std::fs::read_to_string(&plan).unwrap();
        assert_eq!(
            after, PLAN_TWO_FREE_ROWS,
            "the line never landed, so the plan on disk must stand exactly as it was"
        );
        assert!(
            outcome.is_err(),
            "a dispatch whose line the history refused must not report success"
        );
    }

    #[test]
    fn a_run_takes_a_row_from_the_plan_and_the_next_run_cannot_take_the_same_one() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let plan = dir.path().join("lab").join(city::ROADMAP_FILE);
        std::fs::create_dir_all(dir.path().join("lab")).unwrap();
        std::fs::write(&plan, PLAN_TWO_FREE_ROWS).unwrap();
        let take = serde_json::json!({ "action": "claim", "node": "1" });
        let (base_url, provider) = fake_openai(
            &["m-local"],
            vec![
                tool_completion("taking a row", "tu_1", "plan", take.clone()),
                completion("took it", None),
                tool_completion("taking the same row", "tu_2", "plan", take),
                completion("took another", None),
            ],
        );
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        for (n, room) in ["lab/room1", "lab/room2"].into_iter().enumerate() {
            worker
                .handle(channels::Command::Dispatch {
                    addr: Address::parse(room).unwrap(),
                    task: "take a row from the plan".to_owned(),
                    goal: "claim one row, then stop".to_owned(),
                    mode: channels::ModeTag::parse("plan").unwrap(),
                    budget: kernel::BudgetCap::default(),
                    idem: kernel::IdemKey::derive(
                        &RunId::CITY,
                        kernel::Seq::new(u64::try_from(n).unwrap()),
                        b"dispatch",
                    ),
                    session: None,
                    effort: None,
                })
                .unwrap();
        }
        drop(provider);

        let after = std::fs::read_to_string(&plan).unwrap();
        assert!(
            after.contains("| 1 | wire the kiln | 1 |  | In progress |  |"),
            "the plan on disk carries the claim: {after}"
        );
        assert!(
            after.contains("| 2 | glaze tests | 1 |  | Not started |  |"),
            "a refused claim rewrites nothing, not even the row it was offered: {after}"
        );
        let verified = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
        let history: String = verified
            .raw_lines()
            .iter()
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .collect::<Vec<String>>()
            .join(
                "
",
            );
        assert_eq!(
            history.matches("roadmap_claimed").count(),
            1,
            "one row, one claim, however many runs asked for it"
        );
    }

    #[test]
    fn a_finished_row_carries_evidence_a_reader_can_retrieve() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let plan = dir.path().join("lab").join(city::ROADMAP_FILE);
        std::fs::create_dir_all(dir.path().join("lab")).unwrap();
        std::fs::write(&plan, PLAN_ONE_FREE_ROW).unwrap();
        let evidence = format!("cas:b3-{}", "ab".repeat(32));
        // Two calls, because the plan gate admits no shortcut: a run
        // closes the node it took, and this run has to take it first.
        let (base_url, provider) = fake_openai(
            &["m-local"],
            vec![
                tool_completion(
                    "taking it",
                    "tu_1",
                    "plan",
                    serde_json::json!({ "action": "claim", "node": "1" }),
                ),
                tool_completion(
                    "closing it",
                    "tu_2",
                    "plan",
                    serde_json::json!({ "action": "finish", "node": "1", "evidence": evidence }),
                ),
                completion("closed", None),
            ],
        );
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        worker
            .handle(channels::Command::Dispatch {
                addr: Address::parse("lab/room1").unwrap(),
                task: "close the row".to_owned(),
                goal: "finish row 1 with evidence".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::new(0), b"dispatch"),
                session: None,
                effort: None,
            })
            .unwrap();
        drop(provider);

        let after = std::fs::read_to_string(&plan).unwrap();
        let kernel::RoadmapShape::WellFormed { rows } = kernel::check_roadmap_shape(&after) else {
            panic!("an edited plan still parses");
        };
        let tree = kernel::PlanTree::build(rows).expect("an edited plan is still a tree");
        let kernel::Progress::Planned(planned) = tree.progress() else {
            panic!("a plan's progress is planned")
        };
        assert_eq!(
            (planned.done, planned.total),
            (1, 1),
            "an evidenced Done is what moves the figure the person reads"
        );
        let verified = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
        let history: String = verified
            .raw_lines()
            .iter()
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .collect::<Vec<String>>()
            .join(
                "
",
            );
        assert!(history.contains("roadmap_finished"));
    }

    #[test]
    fn a_goal_that_lands_on_a_claimed_path_is_refused_with_the_level_that_decides_it() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let claim = serde_json::json!({
            "statement": "rewrite the kiln notes",
            "paths": ["lab/room1/notes.md"],
            "standing": true,
        });
        let (base_url, provider) = fake_openai(
            &["m-local"],
            vec![
                tool_completion("claiming", "tu_1", "goal", claim.clone()),
                completion("claimed", None),
                tool_completion("claiming too", "tu_2", "goal", claim),
                completion("gave way", None),
            ],
        );
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        for (n, room) in ["lab/room1", "lab/room2"].into_iter().enumerate() {
            worker
                .handle(channels::Command::Dispatch {
                    addr: Address::parse(room).unwrap(),
                    task: "claim the notes".to_owned(),
                    goal: "register a goal, then stop".to_owned(),
                    mode: channels::ModeTag::parse("plan").unwrap(),
                    budget: kernel::BudgetCap::default(),
                    idem: kernel::IdemKey::derive(
                        &RunId::CITY,
                        kernel::Seq::new(u64::try_from(n).unwrap()),
                        b"dispatch",
                    ),
                    session: None,
                    effort: None,
                })
                .unwrap();
        }

        let verified = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
        let history: String = verified
            .raw_lines()
            .iter()
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .collect::<Vec<String>>()
            .join("\n");
        assert!(
            history.contains("goal_registered"),
            "the first claim stands"
        );
        assert!(
            history.contains("goal_conflict"),
            "the second one is a fact about the city, not only a refusal the model saw"
        );
        let asked = provider.bodies().join("\n");
        assert!(
            asked.contains("E_GOAL_CONFLICT"),
            "the refusal reaches the model that asked"
        );
    }

    /// A graph of nodes runs in dependency order, each in its own room
    /// with its contract as its `JOB.md`, and what comes back verified
    /// joins - so the next run in that room can be asked a question only
    /// somebody who opened the results can answer.
    ///
    /// `collab::workshop` and `collab::fanin` had no callers outside
    /// their own files before this; the whole layer was a set of types
    /// nobody had run.
    #[test]
    fn a_workshop_runs_its_nodes_in_order_and_what_comes_back_joins() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let graph = serde_json::json!({
            "op": "lay_out",
            "nodes": [
                {
                    "room": "lab/writer",
                    "goal": "write it up",
                    "done_check": "the page exists",
                    "stop": "when the page exists",
                    "depends_on": ["lab/reader"],
                },
                {
                    "room": "lab/reader",
                    "goal": "read the meter",
                    "done_check": "a number is written down",
                    "stop": "when the number is written down",
                },
            ],
        });
        let (base_url, _provider) = fake_openai(
            &["m-local"],
            vec![
                completion_with("splitting it up", "workshop", "tu_1", graph.clone()),
                completion("waiting on a person", None),
                completion_with("splitting it up", "workshop", "tu_2", graph),
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
                task: "get it measured and written up".to_owned(),
                goal: "a page with a number in it, then stop".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"dispatch"),
                session: None,
                effort: None,
            })
            .unwrap();
        // Laying out a graph is a spawn like any other: the person is
        // asked once, and their answer carries the whole graph.
        let cluster = allow_the_one_pending_item(&mut worker);
        assert_eq!(cluster.class, kernel::ApprovalClass::Delegation);

        for node in ["lab/reader", "lab/writer"] {
            assert!(
                city::job_path(dir.path(), &Address::parse(node).unwrap()).exists(),
                "{node} was never given a job file"
            );
        }
        let contract = std::fs::read_to_string(city::job_path(
            dir.path(),
            &Address::parse("lab/reader").unwrap(),
        ))
        .unwrap();
        assert!(
            contract.contains("## Done check") && contract.contains("## Stop"),
            "a node's job file is its contract: {contract}"
        );

        let verified = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
        let history: String = verified
            .raw_lines()
            .iter()
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .collect::<Vec<String>>()
            .join("\n");
        let reader = history.find("lab/reader").expect("the first node ran");
        let writer = history.find("lab/writer").expect("the second node ran");
        assert!(
            reader < writer,
            "the node everything waits on has to go first"
        );
        assert_eq!(
            worker
                .joins
                .get(&room)
                .map_or(0, |join| join.artifacts().count()),
            2,
            "both results joined, verified by the city rather than by their own producers"
        );
    }
}
