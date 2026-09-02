// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! What a run is frozen with: its plan, and the handoff that resumes it.

use std::path::Path;

use kernel::Locator;
use kernel::{Address, AxCode, AxError};
use runtime::prefix::{FrozenPrefix, FrozenSegment, SegmentSlot};
use runtime::run::RunPlan;

use super::{
    Assignment, DISPATCH_TURN_BUDGET, Given, RunWorker, Site, Workbench, city_segment, name_of,
};

/// One line ending, named once. The prefix joins documents with it, and
/// a literal `10` at four call sites is four chances to mean something
/// else.
pub(super) const NEWLINE: u8 = 10;

/// The building slot: where this run stands, then the rules it stands
/// under.
///
/// `BUILDING.md` is here rather than left for the agent to open because
/// it is exactly as stable as the resident's own file — a person writes
/// it, no run may write it, and it does not move for the length of a
/// session. A rule an agent has to fetch before it can obey it is a rule
/// that gets obeyed one turn late, or not at all.
pub(super) fn building_segment(city_root: &Path, addr: &Address, building: &Address) -> Vec<u8> {
    let mut out = addr.as_str().as_bytes().to_vec();
    if let Ok(rules) = std::fs::read(city::building_path(city_root, building)) {
        out.push(NEWLINE);
        out.push(NEWLINE);
        out.extend_from_slice(&rules);
    }
    out
}

/// The run slot: what the last session left behind, then what this one
/// was asked for.
///
/// In that order, because the brief is what the agent acts on and the
/// last thing in a prompt is the thing that is read. A handoff that is
/// still its blank form contributes nothing and is left out.
/// # Errors
/// Propagates a handoff that exists and cannot be read: a prefix that
/// left it out would tell the next session there was none.
pub(super) fn run_segment(
    city_root: &Path,
    building: &Address,
    brief: &city::RunBrief,
) -> Result<Vec<u8>, AxError> {
    let mut out = Vec::new();
    if let Some(handoff) = city::handoff(city_root, building)? {
        out.extend_from_slice(handoff.as_bytes());
        out.push(NEWLINE);
        out.push(NEWLINE);
    }
    out.extend_from_slice(brief.segment_text().as_bytes());
    Ok(out)
}

pub(super) fn task_line(plan: &RunPlan) -> String {
    format!("{} at {}", plan.task, plan.addr.as_str())
}

impl RunWorker {
    /// Puts one desk's effects on the ledger, and only then makes the
    /// change they announce.
    ///
    /// This is the one door for all five of them. The change arrives as
    /// the return value of `Landing::record`, which appends first, so
    /// there is no expression in this file that reaches the city before
    /// the history - and the match below is exhaustive, so a new kind of
    /// change has to say here what it is.
    ///
    /// # Errors
    /// Propagates the first line the ledger refuses, in which case
    /// nothing changes; then whatever delivering, writing the plan or
    /// filing an entry reports.
    /// Freezes what this run is: the plan it drives on, and the handoff
    /// that says what to read to pick it up again.
    ///
    /// One phase because the two are one decision. The prefix is
    /// assembled for this plan and frozen with it, the handoff quotes
    /// the plan's own task line, and the job locator ends up in both -
    /// pinned in the store, so what a resumed run reads is the bytes the
    /// run segment carried rather than a file somebody edited since.
    ///
    /// # Errors
    /// Propagates a city or run segment that will not read, a norm on
    /// the must-read list that will not open, a store that will not take
    /// the bytes, and a handoff the runtime refuses.
    pub(super) fn freeze_plan(
        &mut self,
        site: &Site,
        workbench: &Workbench,
        at: &Assignment,
        given: Given,
    ) -> Result<(RunPlan, runtime::handoff::Handoff), AxError> {
        let addr = &at.addr;
        let Given {
            brief,
            task,
            goal,
            job,
        } = given;
        let tools = workbench.catalog.borrow().tool_defs();
        // The catalog is part of the resident segment, not a fifth slot:
        // what a resident may reach is as much a standing fact about it
        // as who it is, and both are frozen for the whole run so the
        // prefix stays cacheable across the run's life. Assembled here
        // rather than earlier because the catalog does not exist until
        // the tools, the reading room and the mode are known.
        // The name a person typed when they started this session, which
        // is the last segment of the address they started it at. It
        // opens the resident slot rather than the city one: the city
        // segment is identical for every agent in the city and is
        // cached as such, and a name in it would make one copy per
        // agent of the largest stable block in the prompt.
        let mut resident = format!("Your name: {}\n\n", name_of(addr)).into_bytes();
        resident.extend_from_slice(&site.identity.segment_bytes());
        resident.push(NEWLINE);
        resident.extend_from_slice(workbench.catalog.borrow().render().as_bytes());
        let prefix = FrozenPrefix::assemble(
            FrozenSegment::new(SegmentSlot::City, city_segment(&self.city_root)?),
            FrozenSegment::new(
                SegmentSlot::Building,
                building_segment(&self.city_root, addr, site.building.addr()),
            ),
            FrozenSegment::new(SegmentSlot::Resident, resident),
            FrozenSegment::new(
                SegmentSlot::Run,
                run_segment(&self.city_root, site.building.addr(), &brief)?,
            ),
        )?;

        let plan = RunPlan {
            run: site.run_id,
            who: site.who.clone(),
            addr: addr.clone(),
            task,
            goal,
            // The city decided this when it laid the brief down; the
            // window and the run segment read the one decision.
            opening: match &brief {
                city::RunBrief::Job { .. } => runtime::Opening::FromJob,
                city::RunBrief::Principal => runtime::Opening::WithPerson,
            },
            job: job.clone(),
            parent: at.parent,
            budget_turns: DISPATCH_TURN_BUDGET,
            budget: at.budget,
            shape: runtime::turn::CallShape {
                model: site.model.id.clone(),
                // The model's own ceiling, not a number chosen here.
                // With thinking enabled this budget covers reasoning and
                // answer together, so a hand-picked value truncates runs
                // for a reason that appears nowhere in the account.
                max_tokens: site.model.max_output_tokens,
                // Stated in CONFIG.toml, resolved down the three-layer
                // ladder, and frozen with the run.
                effort: site.config.effort,
            },
            prefix,
            policy: site.rules.policy().clone(),
            tools,
            // From the catalog rather than from a second scan of the
            // shelves: the catalog already decided what this run can
            // reach, and reading the shelf again would answer that
            // question a second time at a different instant.
            skills: workbench.catalog.borrow().skill_pins(),
        };

        // The norms are filled by the machine: their addresses are known
        // when the building is laid out, and a model asked to recite the
        // list from memory gets one entry wrong eventually.
        let mut must_read = Vec::new();
        for norm in city::norms(&self.city_root, addr)? {
            let bytes = std::fs::read(&norm).map_err(|err| {
                AxError::failure(
                    AxCode::StorageFatal,
                    "read a norm document for the must-read list",
                    format!("{}: {err}", norm.display()),
                )
                .with_recovery("fix the file's permissions, or remove it from the building")
            })?;
            let hash = self.cas.put(&bytes).map_err(memory::MemoryError::into_ax)?;
            must_read.push(Locator::parse(&format!("cas:b3-{hash}"))?);
        }
        must_read.push(job);
        let handoff = runtime::handoff::Handoff::new(
            must_read,
            task_line(&plan),
            "see the city roadmap".to_owned(),
            "dispatched from the control surface".to_owned(),
            "resume from the job locator".to_owned(),
        )?;
        Ok((plan, handoff))
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

    /// A handoff that exists and cannot be read is not the absence of a
    /// handoff, and the next session is assembled from it.
    #[test]
    fn a_handoff_that_cannot_be_read_is_refused_by_name() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        let building = dir.path().join("lab");
        std::fs::create_dir_all(building.join("room1")).unwrap();
        lay_rules(
            dir.path(),
            "lab",
            "# BUILDING.md\n\n`confidential: false`\n",
        );
        let lab = Address::parse("lab").unwrap();
        let handoff = city::handoff_path(dir.path(), &lab);
        let _ = std::fs::remove_file(&handoff);
        std::fs::create_dir_all(&handoff).unwrap();

        let (base_url, _provider) =
            fake_openai(&["m-local"], vec![completion("nothing to do", None)]);
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        let outcome = worker.handle(channels::Command::Dispatch {
            addr: Address::parse("lab/room1").unwrap(),
            task: "carry on".to_owned(),
            goal: "one turn".to_owned(),
            mode: channels::ModeTag::parse("plan").unwrap(),
            budget: kernel::BudgetCap::default(),
            idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"handoff"),
            session: None,
            effort: None,
        });

        let err = outcome.expect_err("an unreadable handoff is not an absent one");
        assert!(
            err.to_string().contains(city::HANDOFF_FILE),
            "the refusal has to name the file a person must fix: {err}"
        );
    }

    #[test]
    fn work_offered_in_up_mode_without_a_test_does_not_land() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let note = dir.path().join("lab").join("room1").join("note.md");
        std::fs::create_dir_all(note.parent().unwrap()).unwrap();
        lay_rules(
            dir.path(),
            "lab",
            "# BUILDING.md\n\n`confidential: false`\n\n`review: true`\n",
        );
        std::fs::write(&note, "before\n").unwrap();
        let (base_url, provider) = fake_openai(
            &["m-local"],
            vec![
                tool_completion(
                    "writing",
                    "tu_1",
                    "edit",
                    serde_json::json!({ "path": "lab/room1/note.md", "content": "after
" }),
                ),
                tool_completion(
                    "offering",
                    "tu_2",
                    "pr",
                    serde_json::json!({ "action": "open" }),
                ),
                completion("offered", None),
            ],
        );
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        worker
            .handle(channels::Command::Dispatch {
                addr: Address::parse("lab/room1").unwrap(),
                task: "change the note".to_owned(),
                goal: "the note reads after".to_owned(),
                mode: channels::ModeTag::parse("up").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::new(0), b"dispatch"),
                session: None,
                effort: None,
            })
            .unwrap();
        drop(provider);

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
        assert!(
            history.contains("pr_opened"),
            "the work was offered: {history}"
        );
        assert_eq!(
            std::fs::read_to_string(&note).unwrap(),
            "before
",
            "an improvement with no test of its own does not become the building's"
        );
    }

    #[test]
    fn the_job_lands_in_the_room_and_the_history_carries_the_same_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let room = Address::parse("lab/room1").unwrap();
        let (base_url, _provider) = fake_openai(&["m-local"], vec![completion("done", None)]);
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
                addr: room.clone(),
                task: "measure the thing".to_owned(),
                goal: "a number with a unit, then stop".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"dispatch"),
                session: None,
                effort: None,
            })
            .unwrap();

        let on_disk = std::fs::read_to_string(city::job_path(dir.path(), &room)).unwrap();
        assert!(on_disk.contains("measure the thing"));
        let stored = kernel::B3Hash::digest(on_disk.as_bytes()).to_string();

        let verified = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
        let history: String = verified
            .raw_lines()
            .iter()
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .collect::<Vec<String>>()
            .join("\n");
        assert!(
            history.contains(&stored),
            "the run's job locator addresses the same bytes the room holds"
        );

        // The must-read list is filled from the norms rather than recited:
        // the city's own instructions, this building's rules, and the job.
        let handoff: serde_json::Value = verified
            .raw_lines()
            .iter()
            .filter_map(|line| serde_json::from_slice::<serde_json::Value>(line).ok())
            .find(|value| value["kind"] == "handoff_written")
            .expect("a run that freezes writes its handoff first");
        let must_read = handoff["data"]["must_read"]
            .as_array()
            .expect("the handoff carries its must-read list");
        assert_eq!(must_read.len(), 3, "city, building, job: {must_read:?}");
    }

    /// The prefix carries what it tells the agent to read.
    ///
    /// Before this card the building slot held twelve bytes of address
    /// and the run slot held a `cas:` hash no tool in the city can
    /// resolve, so an agent was told to read its building's rules and
    /// its own task and had no way to reach either.
    #[test]
    fn the_prefix_carries_the_rules_and_the_task_rather_than_pointing_at_them() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        let room = Address::parse("lab/room1").unwrap();
        let (base_url, provider) = fake_openai(&["m-local"], vec![completion("done", None)]);
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        worker
            .handle(channels::Command::CreateBuilding {
                addr: Address::parse("lab").unwrap(),
                template: channels::TemplateName::parse("minimal").unwrap(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"create"),
            })
            .unwrap();
        // A handoff the last session actually wrote, as against the blank
        // form a new building starts with.
        std::fs::write(
            dir.path().join("lab").join("Handoff.md"),
            "# Handoff \u{2014} lab\n\nThe meter reads in millivolts.\n",
        )
        .unwrap();
        worker
            .handle(channels::Command::Dispatch {
                addr: room.clone(),
                task: "measure the thing".to_owned(),
                goal: "a number with a unit, then stop".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"dispatch"),
                session: None,
                effort: None,
            })
            .unwrap();

        let asked = provider.bodies().join("\n");
        assert!(
            asked.contains("confidential: false"),
            "the building's own rules reach the model: {asked}"
        );
        assert!(
            asked.contains("The meter reads in millivolts."),
            "what the last session left reaches the next one"
        );
        assert!(
            asked.contains("a number with a unit, then stop"),
            "this session's brief is in the prompt, not addressed by it"
        );
        assert!(
            !asked.contains("FULL READ"),
            "nothing sends the agent after what it already holds"
        );
        assert!(
            !asked.contains("cas:b3-"),
            "no content hash reaches a model that cannot resolve one"
        );
    }

    /// Work handed down is the same work, so it is done under the same
    /// ceiling.
    ///
    /// `knock` already carries the speaker's `BudgetCap` and says why in
    /// its own comment; delegation is the stronger case of the same
    /// thing and was the one path that zeroed it. What that costs today
    /// is not overspending - nothing enforces a cap yet, and `StatusTool`
    /// is its only reader - but a delegate telling a model its budget is
    /// zero while its parent was told the truth. That is the defect
    /// section 8-12 already named: a model that calls `status` and gets a
    /// row of noughts learns not to ask again.
    ///
    /// Both halves are asserted, so the test cannot pass by carrying
    /// nothing anywhere.
    #[test]
    fn work_handed_down_is_done_under_the_ceiling_that_sent_it() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        let hand_down = |id: &str| {
            completion_with(
                "handing it down",
                "delegate",
                id,
                serde_json::json!({
                    "room": "lab/helper",
                    "task": "measure the thing",
                    "goal": "a number, then stop",
                }),
            )
        };
        let read_situation =
            |id: &str| completion_with("what am I under", "status", id, serde_json::json!({}));
        let (base_url, provider) = fake_openai(
            &["m-local"],
            vec![
                // First dispatch, on the default ceiling. Its only job is
                // to get the person's answer on record, which settles the
                // Policy that lets the second dispatch delegate without
                // stopping - so the run under test never goes near the
                // approval resumption path.
                hand_down("tu_1"),
                completion("waiting on a person", None),
                hand_down("tu_2"),
                completion("parent done", None),
                completion("child done", None),
                // Second dispatch, carrying a real ceiling. The parent
                // reads its situation first - that reading is the control
                // arm, green before this card and after it - then hands
                // the work down.
                read_situation("tu_3"),
                hand_down("tu_4"),
                completion("parent done", None),
                // The child reads its own. This is the arm the card is
                // about, and it said zero.
                read_situation("tu_5"),
                completion("child done", None),
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
        let ceiling = kernel::BudgetCap {
            usd: kernel::UsdMicros::new(250_000),
            tokens: kernel::Tokens::new(4_000),
        };
        fn send(worker: &mut RunWorker, budget: kernel::BudgetCap, tag: &[u8]) {
            worker
                .handle(channels::Command::Dispatch {
                    addr: Address::parse("lab/room1").unwrap(),
                    task: "get it measured".to_owned(),
                    goal: "the number is written down, then stop".to_owned(),
                    mode: channels::ModeTag::parse("plan").unwrap(),
                    budget,
                    idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, tag),
                    session: None,
                    effort: None,
                })
                .unwrap();
        }
        send(
            &mut worker,
            kernel::BudgetCap::default(),
            b"settle-the-policy",
        );
        allow_the_one_pending_item(&mut worker);
        send(&mut worker, ceiling, b"under-a-real-ceiling");

        let stated = "budget: 250000 usd_micros, 4000 tokens".to_owned();
        let asked = provider.bodies();

        // The control arm: the run the person dispatched was already told
        // the truth, so this stays green on both sides of the fix and a
        // regression here means the test stopped reaching the code.
        assert_eq!(
            ceiling_read_by(&asked, "lab/room1"),
            vec![stated.clone()],
            "the dispatched run reads the ceiling it was sent with"
        );
        // The arm this card is about. Before the fix it read
        // "budget: 0 usd_micros, 0 tokens".
        assert_eq!(
            ceiling_read_by(&asked, "lab/helper"),
            vec![stated],
            "work handed down is done under the ceiling that sent it"
        );
    }

    /// Whose reading is whose. A conversation carries its earlier turns,
    /// so one run's status answer appears in every later request of that
    /// same run - counting bodies counts one run many times and passes
    /// without a fix. What tells readings apart is the address inside
    /// the status block itself, and the answer is the set of distinct
    /// ceilings read at that address rather than how many times.
    fn ceiling_read_by(bodies: &[String], room: &str) -> Vec<String> {
        let mut readings = Vec::new();
        for body in bodies {
            // The status text is a JSON string inside a JSON string, so
            // one newline arrives as two backslashes and an `n`.
            for (at, _) in body.match_indices(&format!("addr: {room}\\\\n")) {
                let window = body
                    .get(at..body.len().min(at.saturating_add(240)))
                    .unwrap_or_default();
                let Some(from) = window.find("budget: ") else {
                    continue;
                };
                let tail = window.get(from..).unwrap_or_default();
                let line = tail.split("\\\\n").next().unwrap_or_default();
                readings.push(line.to_owned());
            }
        }
        readings.sort_unstable();
        readings.dedup();
        readings
    }

    /// An answer carries the work on, and the work it carries on is the
    /// same work - so it runs under the ceiling that sent it.
    ///
    /// The resumption path went through `fn dispatch`, which writes
    /// `BudgetCap::default()`, and no word could fix that: `BlockedJob`
    /// was rebuilt out of `run_started`, and that record did not carry a
    /// ceiling for it to be rebuilt from. Both runs here stand at the
    /// same address, so the discriminator is not `addr:` but the set of
    /// distinct ceilings read there: two values before this card, one
    /// after.
    #[test]
    fn work_resumed_by_an_answer_is_done_under_the_ceiling_that_sent_it() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let read_situation =
            |id: &str| completion_with("what am I under", "status", id, serde_json::json!({}));
        let (base_url, provider) = fake_openai(
            &["m-local"],
            vec![
                read_situation("tu_1"),
                completion("stopping here", None),
                // The run the answer carries on: it reads its own
                // situation, which is the arm this card is about.
                read_situation("tu_2"),
                completion("carried on", None),
            ],
        );
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        let addr = Address::parse("lab/room1").unwrap();
        let ceiling = kernel::BudgetCap {
            usd: kernel::UsdMicros::new(250_000),
            tokens: kernel::Tokens::new(4_000),
        };
        worker
            .handle(channels::Command::Dispatch {
                addr: addr.clone(),
                task: "empty the archive".to_owned(),
                goal: "one sweep, then stop".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: ceiling,
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"under-a-cap"),
                session: None,
                effort: None,
            })
            .unwrap();

        // The item that run left waiting, recorded the way the bench
        // records one: against the run and the address, because
        // answering it later has to find the work it was holding up.
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
                    addr,
                    kind: EventKind::ApprovalRequested,
                    data: Payload::new(value.as_object().cloned().unwrap()).unwrap(),
                },
            )
            .unwrap();

        worker
            .handle(channels::Command::Approve {
                item: kernel::ApprovalId::new("item-1").unwrap(),
                verdict: kernel::PolicyVerdict::Allow,
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"approve"),
            })
            .unwrap();

        assert_eq!(
            ceiling_read_by(&provider.bodies(), "lab/room1"),
            vec!["budget: 250000 usd_micros, 4000 tokens".to_owned()],
            "both the run the person sent and the run their answer carried on read one ceiling, \
             the one that was sent"
        );
    }

    #[test]
    fn the_effort_a_config_layer_states_is_what_goes_out_on_the_wire() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        let room = Address::parse("lab/room1").unwrap();
        let city_layer = city::config_path(dir.path(), &room, city::Layer::City).unwrap();
        std::fs::create_dir_all(city_layer.parent().unwrap()).unwrap();
        std::fs::write(&city_layer, "[model]\neffort = \"low\"\n").unwrap();
        let building_layer = city::config_path(dir.path(), &room, city::Layer::Building).unwrap();
        std::fs::create_dir_all(building_layer.parent().unwrap()).unwrap();
        std::fs::write(&building_layer, "[model]\neffort = \"xhigh\"\n").unwrap();

        let (base_url, provider) = fake_openai(&["m-local"], vec![completion("done", None)]);
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        worker
            .handle(channels::Command::Dispatch {
                addr: room,
                task: "think about it".to_owned(),
                goal: "answer".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"dispatch"),
                session: None,
                effort: None,
            })
            .unwrap();

        let asked = provider.bodies().join("\n");
        assert!(
            asked.contains("\"effort\":\"xhigh\""),
            "the building layer overrides the city layer, and the resolved level is what the \
             provider was asked for: {asked}"
        );
        assert!(!asked.contains("\"effort\":\"low\""));
    }

    #[test]
    fn a_fork_records_lineage_and_refuses_a_node_the_mother_does_not_own() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        let (base_url, _provider) = fake_openai(&["m-local"], vec![completion("done", None)]);
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        worker
            .handle(channels::Command::Dispatch {
                addr: Address::parse("lab/room1").unwrap(),
                task: "mother work".to_owned(),
                goal: "a lineage".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"dispatch"),
                session: None,
                effort: None,
            })
            .unwrap();
        // Find the mother's run_started node in the verified chain.
        let verified = runtime::replay::verify_ledger_dir(&ledger_dir(dir.path())).unwrap();
        let (mother, node) = verified
            .lines()
            .iter()
            .find_map(|line| match line {
                runtime::replay::VerifiedLine::Known { record, .. }
                    if record.kind() == EventKind::RunStarted =>
                {
                    Some((record.run(), record.seq()))
                }
                _ => None,
            })
            .expect("a dispatch writes run_started");
        let new_run = worker.fork(mother, node, None).unwrap();
        assert_ne!(new_run, mother);
        let after = runtime::replay::verify_ledger_dir(&ledger_dir(dir.path())).unwrap();
        let forked = after
            .lines()
            .iter()
            .filter_map(|line| match line {
                runtime::replay::VerifiedLine::Known { record, .. }
                    if record.kind() == EventKind::RunForked =>
                {
                    Some(record.clone())
                }
                _ => None,
            })
            .next()
            .expect("the fork is a ledger fact");
        assert_eq!(forked.run(), new_run);
        // Seq 0 is the genesis, a city event: not the mother's node.
        let err = worker.fork(mother, kernel::Seq::FIRST, None).unwrap_err();
        assert!(err.subject().contains("not an event of run"), "{err}");
    }
}
