// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! What a run offers a building it may not write in, and what merging it costs.

use kernel::{AxError, EventKind, Payload};

use crate::effect;

use super::{Assignment, RunWorker, Site, now_ms};

impl RunWorker {
    /// Settles what a run asked of the request register.
    ///
    /// Opening commits the run's own tree first, because the record names
    /// the commit a verifier will be judging. Checking merges, because
    /// that is what a passed check means, and a verified request nobody
    /// merged would be a third state for a person to chase. Both put the
    /// line before the change, the merge by way of `memory::PlannedMerge`
    /// (section 8-30).
    ///
    /// # Errors
    /// Propagates a worktree that will not open, a fence that will not
    /// commit, a merge the trunk has moved past, and any line the ledger
    /// refuses.
    pub(super) fn settle_requests(
        &mut self,
        site: &Site,
        at: &Assignment,
        pr: &std::rc::Rc<std::cell::RefCell<collab::PrDesk>>,
        produced: &runtime::Produced,
    ) -> Result<(), AxError> {
        let (addr, who, run_id, mode) = (&at.addr, site.who.as_str(), site.run_id, at.mode);
        let write_root = site.write_root.as_path();
        let fence_scope = site.fence_scope(addr);
        let fence_scope = fence_scope.as_str();
        // What the run asked of the request register. Opening commits
        // the run's own tree first, because the record names the commit
        // a verifier will be judging; checking merges, because that is
        // what a passed check means and a verified request nobody merged
        // would be a third state for a person to chase.
        let pr_effects = pr.borrow_mut().take_effects();
        if !pr_effects.is_empty() {
            let trees =
                memory::Worktrees::open(&self.city_root).map_err(memory::MemoryError::into_ax)?;
            for effect in pr_effects {
                match effect {
                    collab::PrEffect::Opened { branch } => {
                        let commit = memory::Checkpoint::open(write_root)
                            .map_err(memory::MemoryError::into_ax)?
                            .wave_pre(fence_scope, now_ms()?, who)
                            .map_err(memory::MemoryError::into_ax)?;
                        let at = commit
                            .as_map()
                            .get("oid")
                            .and_then(serde_json::Value::as_str)
                            .unwrap_or_default()
                            .to_owned();
                        let request = collab::OpenRequest {
                            node: collab::NodeId::parse(&branch)?,
                            implementer: who.to_owned(),
                            branch,
                            commit: at,
                        };
                        self.record_for(
                            run_id,
                            effect::Line {
                                who: who.to_owned(),
                                addr: addr.clone(),
                                kind: EventKind::PrOpened,
                                data: request.payload()?,
                            },
                        )?;
                        self.requests.push(request);
                    }
                    collab::PrEffect::Merged { request, by } => {
                        // The last gate before work becomes the
                        // building's. Verification says a person other
                        // than the author looked; admission says the
                        // evidence this mode demands is present. They
                        // are different questions, and the second one is
                        // the only place a mode means anything.
                        if let runtime::Admission::Refused {
                            because,
                            alternative,
                        } = runtime::admits(mode, produced)
                        {
                            let mut data = request.payload()?.as_map().clone();
                            data.insert("by".to_owned(), serde_json::Value::String(by));
                            data.insert(
                                "why".to_owned(),
                                serde_json::Value::String(format!("{because}; {alternative}")),
                            );
                            self.record_for(
                                run_id,
                                effect::Line {
                                    who: who.to_owned(),
                                    addr: addr.clone(),
                                    kind: EventKind::PrRejected,
                                    data: Payload::new(data)?,
                                },
                            )?;
                            self.requests.retain(|held| held.branch != request.branch);
                            continue;
                        }
                        let name = memory::WorktreeName::parse(&request.branch)
                            .map_err(memory::MemoryError::into_ax)?;
                        // Decided first, announced second, made third.
                        // The refusal this merge can carry - a trunk that
                        // moved after the node branched - happens inside
                        // `plan_merge`, so no line is ever written for a
                        // merge that was going to be refused; and the
                        // trunk cannot move before the line, because
                        // moving it needs a value only that call returns.
                        let planned = trees
                            .plan_merge(&name)
                            .map_err(memory::MemoryError::into_ax)?;
                        let mut data = request.payload()?.as_map().clone();
                        data.insert("verified_by".to_owned(), serde_json::Value::String(by));
                        data.insert(
                            "commit".to_owned(),
                            serde_json::Value::String(planned.commit()),
                        );
                        self.record_for(
                            run_id,
                            effect::Line {
                                who: who.to_owned(),
                                addr: addr.clone(),
                                kind: EventKind::PrMerged,
                                data: Payload::new(data)?,
                            },
                        )?;
                        planned.apply().map_err(memory::MemoryError::into_ax)?;
                        self.requests.retain(|held| held.branch != request.branch);
                    }
                    collab::PrEffect::Rejected { request, by, why } => {
                        let mut data = request.payload()?.as_map().clone();
                        data.insert("by".to_owned(), serde_json::Value::String(by));
                        data.insert("why".to_owned(), serde_json::Value::String(why));
                        self.record_for(
                            run_id,
                            effect::Line {
                                who: who.to_owned(),
                                addr: addr.clone(),
                                kind: EventKind::PrRejected,
                                data: Payload::new(data)?,
                            },
                        )?;
                        self.requests.retain(|held| held.branch != request.branch);
                    }
                }
            }
        }
        Ok(())
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

    /// A building under review lends every run its own tree so that
    /// nothing it produces is the building's until somebody else checks
    /// it. This asks whether the shelf is inside that fence.
    #[test]
    fn a_run_under_review_puts_nothing_on_the_shelf_before_it_is_checked() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let building = dir.path().join("lab");
        std::fs::create_dir_all(building.join("room1")).unwrap();
        std::fs::create_dir_all(building.join("room2")).unwrap();
        lay_rules(
            dir.path(),
            "lab",
            "# BUILDING.md\n\n`confidential: false`\n\n`review: true`\n",
        );

        let (base_url, _provider) = fake_openai(
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
                task: "decide and remember".to_owned(),
                goal: "one decision".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"remember"),
                session: None,
                effort: None,
            })
            .unwrap();

        let lab = Address::parse("lab").unwrap();
        let before = city::archive_index(dir.path(), &lab).unwrap();
        assert!(
            before.is_empty(),
            "a run under review reached the building's shelf without being checked: {before:?}"
        );

        // The other half of the same rule: fencing it must not lose it.
        // A shelf entry nobody can ever reach is a worse answer than one
        // that arrived too early.
        let branch = branch_opened(&report.ledger_dir);
        let (base_url, _second) = fake_openai(
            &["m-local"],
            vec![
                tool_completion(
                    "checking",
                    "tu_3",
                    "pr",
                    serde_json::json!({ "action": "check", "branch": branch, "passed": true }),
                ),
                completion("checked", None),
            ],
        );
        let mut checker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        checker
            .handle(channels::Command::Dispatch {
                addr: Address::parse("lab/room2").unwrap(),
                task: "check the decision".to_owned(),
                goal: "one check".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"check"),
                session: None,
                effort: None,
            })
            .unwrap();

        let after = city::archive_index(dir.path(), &lab).unwrap();
        assert_eq!(
            after.len(),
            1,
            "once it was checked the decision is the building's: {after:?}"
        );
    }

    /// The branch a request was opened on, read back from the history.
    fn branch_opened(ledger_dir: &Path) -> String {
        let verified = runtime::replay::verify_ledger_dir(ledger_dir).unwrap();
        let mut found = None;
        for line in verified.raw_lines() {
            let record = EventRecord::parse_line(line).unwrap();
            if record.kind() == EventKind::PrOpened {
                found = record
                    .data()
                    .as_map()
                    .get("branch")
                    .and_then(|v| v.as_str())
                    .map(str::to_owned);
            }
        }
        found.expect("opening a request leaves a record naming the branch")
    }

    #[test]
    fn work_in_a_review_building_reaches_it_only_after_someone_else_checks_it() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        // The building asks for review, so every run works in its own
        // tree and nothing lands until a second resident says so.
        let building = dir.path().join("lab");
        std::fs::create_dir_all(building.join("room1")).unwrap();
        std::fs::create_dir_all(building.join("room2")).unwrap();
        lay_rules(
            dir.path(),
            "lab",
            "# BUILDING.md\n\n`confidential: false`\n\n`review: true`\n",
        );
        let note = building.join("room1").join("notes.md");
        std::fs::create_dir_all(note.parent().unwrap()).unwrap();
        std::fs::write(&note, b"before\n").unwrap();
        let version = runtime::version_of(b"before\n");

        let (base_url, _provider) = fake_openai(
            &["m-local"],
            vec![
                tool_completion(
                    "editing",
                    "tu_1",
                    "edit",
                    serde_json::json!({
                        "path": "lab/room1/notes.md",
                        "base_version": version,
                        "old": "before",
                        "new": "after",
                    }),
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
                task: "fix the notes".to_owned(),
                goal: "one edit, then offer it".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"one"),
                session: None,
                effort: None,
            })
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(&note).unwrap(),
            "before\n",
            "the implementer's work is invisible to the building until it is merged"
        );

        // A second resident checks it. The same tool, a different run.
        let branch = {
            let verified = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
            let mut found = None;
            for line in verified.raw_lines() {
                let record = EventRecord::parse_line(line).unwrap();
                if record.kind() == EventKind::PrOpened {
                    found = record
                        .data()
                        .as_map()
                        .get("branch")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned);
                }
            }
            found.expect("opening a request leaves a record naming the branch")
        };
        let (base_url, _second) = fake_openai(
            &["m-local"],
            vec![
                tool_completion(
                    "checking",
                    "tu_3",
                    "pr",
                    serde_json::json!({ "action": "check", "branch": branch, "passed": true }),
                ),
                completion("checked", None),
            ],
        );
        let mut checker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        checker
            .handle(channels::Command::Dispatch {
                addr: Address::parse("lab/room2").unwrap(),
                task: "check the notes".to_owned(),
                goal: "one check, then stop".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"two"),
                session: None,
                effort: None,
            })
            .unwrap();

        assert_eq!(
            std::fs::read_to_string(&note).unwrap(),
            "after\n",
            "once someone else has checked it, the building stands on the work"
        );
        let verified = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
        let history: String = verified
            .raw_lines()
            .iter()
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .collect::<Vec<String>>()
            .join("\n");
        assert!(history.contains("worktree_opened"));
        assert!(history.contains("pr_merged"));
    }

    /// The merge is the last change in the city that still outran the
    /// line announcing it. `trees.merge` moved the trunk and only then
    /// wrote `pr_merged`, so a refused line left the building standing on
    /// work its own history says was never brought in.
    #[test]
    fn a_merge_the_history_refused_leaves_the_building_where_it_was() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let building = dir.path().join("lab");
        std::fs::create_dir_all(building.join("room1")).unwrap();
        std::fs::create_dir_all(building.join("room2")).unwrap();
        lay_rules(
            dir.path(),
            "lab",
            "# BUILDING.md

`confidential: false`

`review: true`
",
        );
        let note = building.join("room1").join("notes.md");
        std::fs::write(
            &note, b"before
",
        )
        .unwrap();
        let version = runtime::version_of(
            b"before
",
        );

        let (base_url, _provider) = fake_openai(
            &["m-local"],
            vec![
                tool_completion(
                    "editing",
                    "tu_1",
                    "edit",
                    serde_json::json!({
                        "path": "lab/room1/notes.md",
                        "base_version": version,
                        "old": "before",
                        "new": "after",
                    }),
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
                task: "fix the notes".to_owned(),
                goal: "one edit, then offer it".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"one"),
                session: None,
                effort: None,
            })
            .unwrap();

        let branch = {
            let verified = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
            let mut found = None;
            for line in verified.raw_lines() {
                let record = EventRecord::parse_line(line).unwrap();
                if record.kind() == EventKind::PrOpened {
                    found = record
                        .data()
                        .as_map()
                        .get("branch")
                        .and_then(|v| v.as_str())
                        .map(str::to_owned);
                }
            }
            found.expect("opening a request leaves a record naming the branch")
        };

        // The checking run works over a ledger that will lose exactly the
        // line saying the work was merged.
        let (base_url, _second) = fake_openai(
            &["m-local"],
            vec![
                tool_completion(
                    "checking",
                    "tu_3",
                    "pr",
                    serde_json::json!({ "action": "check", "branch": branch, "passed": true }),
                ),
                completion("checked", None),
            ],
        );
        let fs = memory::FaultFs::new(memory::FaultPlan {
            cut_at_op: None,
            cut_on_write: Some("pr_merged"),
            torn_tail: memory::TornTail::None,
        });
        let (ledger, _report) =
            memory::JsonlLedger::open_faulty(fs, &ledger_dir(dir.path()), now_ms().unwrap())
                .unwrap();
        let mut checker = RunWorker::over(
            dir.path(),
            gateway::Custodian::in_memory(),
            runtime::diagnostics::Diagnostics::off(),
            ledger,
        )
        .unwrap();
        checker
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
        checker
            .handle(channels::Command::SelectModel {
                endpoint: channels::ProviderName::parse("house").unwrap(),
                model: "m-local".to_owned(),
                tag: kernel::ModelTag::Main,
                context_tokens: 32_768,
                max_output_tokens: 4_096,
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"select"),
            })
            .unwrap();
        let outcome = checker.handle(channels::Command::Dispatch {
            addr: Address::parse("lab/room2").unwrap(),
            task: "check the notes".to_owned(),
            goal: "one check, then stop".to_owned(),
            mode: channels::ModeTag::parse("plan").unwrap(),
            budget: kernel::BudgetCap::default(),
            idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"two"),
            session: None,
            effort: None,
        });

        assert_eq!(
            std::fs::read_to_string(&note).unwrap(),
            "before
",
            "the line saying the work was merged never landed, so the building must still stand              on what it had"
        );
        assert!(
            outcome.is_err(),
            "a merge whose line the history refused must not report success"
        );
    }
}
