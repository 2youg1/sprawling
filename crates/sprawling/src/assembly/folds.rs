// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Everything a worker inherits from a history it did not write.

use std::path::Path;

use kernel::{Address, AxError, EventKind};
use kernel::{EventRecord, Locator, Payload, RunId};

use crate::effect;
use crate::views::{Views, pursuit_from};

use super::{building_of, plan_node_of, read_autonomy};

/// Rebuilds what the worker answers approvals from. Same disposability
/// as every other view: delete it, replay, get the same answers.
///
/// # Errors
/// Propagates chain verification failures.
/// The two projections the collaboration tools read: what is waiting in
/// each room, and what ground is already claimed.
///
/// Signals are rebuilt by sieve rather than by replaying queue
/// operations: collect what was enqueued, collect what was consumed,
/// and deliver only the difference, in the order it first arrived. That
/// keeps the queue free of a "remove by id" it has no business having,
/// and it is the same answer a fresh city would reach from the same
/// lines.
/// The work an answered item was holding up.
///
/// The ceiling travels with it because the answer resumes the same piece
/// of work: a run that stopped to ask and was told yes is not a new run
/// that happens to be at the same address.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) struct BlockedJob {
    pub(super) addr: Address,
    pub(super) task: String,
    pub(super) goal: String,
    pub(super) budget: kernel::BudgetCap,
}

/// The three registers a run's collaboration tools read from.
pub(super) struct Collaboration {
    pub(super) inboxes: std::collections::BTreeMap<Address, collab::Inbox>,
    /// What each room's earlier runs got back from work they handed
    /// down, verified. Folded from the same handback signals the inboxes
    /// are folded from, because a join outlives one run: a child starts
    /// after its parent froze.
    pub(super) joins: std::collections::BTreeMap<Address, collab::FanIn>,
    pub(super) goals: Vec<kernel::GoalEntry>,
    pub(super) requests: Vec<collab::OpenRequest>,
    /// What each building was last told to work towards.
    pursuits: std::collections::BTreeMap<Address, (String, kernel::PursuitState)>,
    /// Which room holds each node of each building's plan.
    pub(super) plan_holders:
        std::collections::BTreeMap<Address, std::collections::BTreeMap<kernel::NodeId, String>>,
}

impl Collaboration {
    /// The pursuits as values, minted through the depth-zero position.
    ///
    /// The fold reads text and state out of the records; only a holder
    /// of a `Delegator` turns those back into something that can take
    /// work, which is why this takes one rather than doing it inline.
    pub(super) fn pursuits(
        &self,
        at: &kernel::Delegator,
    ) -> std::collections::BTreeMap<Address, kernel::Pursuit> {
        let mut out = std::collections::BTreeMap::new();
        for (addr, (goal, state)) in &self.pursuits {
            let Ok(mut held) = kernel::Pursuit::declare(at, goal.clone()) else {
                continue;
            };
            if *state == kernel::PursuitState::Paused {
                held.pause();
            }
            out.insert(addr.clone(), held);
        }
        out
    }
}

/// `Collaboration` while it is still being read out of a history.
///
/// Signals are held aside until the last line has been seen, because a
/// queue is `enqueued` minus `consumed` and the two arrive in whatever
/// order the work happened in. Nothing else here needs a second look, so
/// nothing else is staged.
#[derive(Default)]
pub(super) struct CollaborationFold {
    pub(super) goals: Vec<kernel::GoalEntry>,
    /// What each building was last told to work towards. Text and
    /// state, because a `Pursuit` is minted through the depth-zero
    /// position and a fold has none.
    pursuits: std::collections::BTreeMap<Address, (String, kernel::PursuitState)>,
    /// Which room holds each node of each building's plan. The map a
    /// red node's neighbours are found through, and the one copy of it:
    /// the record that claims a node carries the room in its `addr`,
    /// so nothing here derives what somebody else already wrote down.
    plan_holders:
        std::collections::BTreeMap<Address, std::collections::BTreeMap<kernel::NodeId, String>>,
    pub(super) requests: Vec<collab::OpenRequest>,
    enqueued: Vec<collab::Signal>,
    consumed: std::collections::BTreeSet<String>,
}

impl CollaborationFold {
    pub(super) fn absorb(&mut self, record: &EventRecord) -> Result<(), AxError> {
        match record.kind() {
            EventKind::SignalEnqueued => self
                .enqueued
                .push(collab::Signal::from_payload(record.data())?),
            EventKind::SignalConsumed => {
                if let Some(id) = record
                    .data()
                    .as_map()
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                {
                    self.consumed.insert(id.to_owned());
                }
            }
            EventKind::GoalRegistered => self.goals.push(effect::goal_from_payload(record.data())?),
            EventKind::RoadmapClaimed => {
                if let (Some(building), Some(node), Some(room)) = (
                    record.addr().and_then(building_of),
                    plan_node_of(record),
                    record.addr(),
                ) {
                    self.plan_holders
                        .entry(building)
                        .or_default()
                        .insert(node, room.as_str().to_owned());
                }
            }
            EventKind::RoadmapFinished | EventKind::RoadmapReleased | EventKind::RoadmapBlocked => {
                if let (Some(building), Some(node)) =
                    (record.addr().and_then(building_of), plan_node_of(record))
                {
                    self.plan_holders.entry(building).or_default().remove(&node);
                }
            }
            EventKind::PursuitChanged => {
                if let Some((addr, held)) = pursuit_from(record) {
                    match held {
                        Some(entry) => {
                            self.pursuits.insert(addr, entry);
                        }
                        None => {
                            self.pursuits.remove(&addr);
                        }
                    }
                }
            }
            EventKind::PrOpened => self
                .requests
                .push(collab::OpenRequest::from_payload(record.data())?),
            // A request leaves the register whichever way it ended: a
            // merged one is done, and a rejected one goes back to the
            // resident who wrote it rather than sitting in a queue
            // nobody owns.
            EventKind::PrMerged | EventKind::PrRejected => {
                if let Some(branch) = record
                    .data()
                    .as_map()
                    .get("branch")
                    .and_then(serde_json::Value::as_str)
                {
                    self.requests.retain(|held| held.branch != branch);
                }
            }
            _ => {}
        }
        Ok(())
    }

    pub(super) fn settle(self) -> Result<Collaboration, AxError> {
        let mut inboxes = std::collections::BTreeMap::new();
        let mut joins: std::collections::BTreeMap<Address, collab::FanIn> =
            std::collections::BTreeMap::new();
        let (goals, requests, consumed) = (self.goals, self.requests, self.consumed);
        let (pursuits, plan_holders) = (self.pursuits, self.plan_holders);
        for signal in self.enqueued {
            // A join is folded from every handback the room ever
            // received, whether or not the signal announcing it has been
            // read: reading a notice and holding a result are different
            // facts.
            if let Some(artifact) = artifact_of(&signal) {
                joins
                    .entry(signal.room().clone())
                    .or_default()
                    .accept(artifact);
            }
            if consumed.contains(signal.id().as_str()) {
                continue;
            }
            let inbox = inboxes
                .entry(signal.room().clone())
                .or_insert_with(new_inbox);
            inbox.deliver(&signal)?;
        }
        Ok(Collaboration {
            inboxes,
            joins,
            goals,
            requests,
            pursuits,
            plan_holders,
        })
    }
}

/// The verified result a handback signal reports, when it reports one.
///
/// Reads back what `collab::Handback::signal` wrote, and nothing else -
/// an ordinary signal between residents is not a result and returns
/// `None`. The digest is taken from the locator rather than carried
/// beside it: two fields holding one hash are two places for it to
/// disagree.
pub(super) fn artifact_of(signal: &collab::Signal) -> Option<collab::Artifact> {
    let body = signal.payload().as_map();
    if body.get("handback").and_then(serde_json::Value::as_str)? != "finished" {
        return None;
    }
    let node = collab::NodeId::parse(body.get("room").and_then(serde_json::Value::as_str)?).ok()?;
    let at = Locator::parse(body.get("at").and_then(serde_json::Value::as_str)?).ok()?;
    let Locator::Cas { hash, .. } = at else {
        return None;
    };
    let verified_by = body
        .get("verified_by")
        .and_then(serde_json::Value::as_str)?
        .to_owned();
    collab::Claim::new(node, at.clone(), hash, signal.from().to_owned())
        .verified(true, &verified_by)
        .ok()
}

pub(super) fn new_inbox() -> collab::Inbox {
    collab::Inbox::new(INBOX_CAPACITY, SIGNAL_BANDWIDTH)
}

/// What a run was sent out to do, and what it was allowed to spend
/// doing it. Read back from `run_started`, which is the only record
/// that carries all three.
#[derive(Clone, PartialEq, Eq, Debug)]
pub(super) struct Sent {
    pub(super) task: String,
    pub(super) goal: String,
    pub(super) budget: kernel::BudgetCap,
}

/// How many signals one room may hold, and how many one pull takes.
/// Bandwidth belongs to the receiver: a sender cannot push more into a
/// resident's context than the resident agreed to read at once.
pub(super) const INBOX_CAPACITY: u64 = 256;

pub(super) const SIGNAL_BANDWIDTH: u32 = 4;

/// Who may answer, what is waiting, what has already been allowed, and
/// what each waiting item is holding up.
///
/// One fold, two readers. `Standing::fold` shows it every line of a
/// history it did not write; `RunWorker::govern` shows it every line the
/// running city writes. Until R2.11 those were two implementations that
/// happened to agree, and `set_admission` and `answer_approval` each
/// held a third by writing a field directly.
pub(super) struct Governance {
    pub(super) pending: std::collections::BTreeMap<String, kernel::ApprovalItem>,
    pub(super) autonomy: kernel::Autonomy,
    pub(super) granted: Vec<kernel::ClusterKey>,
    /// The scopes a person has shut, by the name `scope_name` gives
    /// them. Folded from the ledger like everything else the panel
    /// shows, so a restarted city is still halted.
    pub(super) halted: std::collections::BTreeSet<String>,
    /// What each run was sent to do, by run.
    ///
    /// Never pruned, and one short entry per run - the same growth class
    /// as `memory::HotView`, which is also one entry per run. It cannot
    /// be pruned on `run_frozen`: `freeze` writes inside the drive while
    /// the assembly records waiting items after it, so the ledger order
    /// is `run_started … run_frozen … approval_requested` and pruning
    /// there would drop the entry one line before it is read.
    sent: std::collections::BTreeMap<RunId, Sent>,
    /// What each waiting item is holding up, by approval id. Pruned when
    /// the item is answered, because an answered item holds nothing up.
    pub(super) origins: std::collections::BTreeMap<String, BlockedJob>,
}

impl Governance {
    /// A city nobody has governed yet.
    fn empty() -> Governance {
        Governance {
            pending: std::collections::BTreeMap::new(),
            autonomy: kernel::consts_policy::AUTONOMY_DEFAULT,
            granted: Vec::new(),
            halted: std::collections::BTreeSet::new(),
            sent: std::collections::BTreeMap::new(),
            origins: std::collections::BTreeMap::new(),
        }
    }

    /// Registers what a run was sent to do.
    ///
    /// Two callers, one shape: the dispatch that is about to build the
    /// `RunPlan` out of these very values, and `absorb` reading them back
    /// out of `run_started`. `what_a_worker_holds_is_what_a_restart_rebuilds`
    /// is what holds the two to the same answer.
    pub(super) fn sent(&mut self, run: RunId, task: &str, goal: &str, budget: kernel::BudgetCap) {
        self.sent.insert(
            run,
            Sent {
                task: task.to_owned(),
                goal: goal.to_owned(),
                budget,
            },
        );
    }

    /// Folds one line in, from whichever of the two directions it came.
    ///
    /// The envelope arrives beside the payload because two of these arms
    /// need it: a run is named by the record it started, and a waiting
    /// item is held against the room that raised it.
    pub(super) fn absorb(
        &mut self,
        kind: EventKind,
        run: RunId,
        addr: Option<&Address>,
        payload: &Payload,
    ) {
        let data = payload.as_map();
        let text = |key: &str| {
            data.get(key)
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_owned()
        };
        let count = |key: &str| {
            data.get(key)
                .and_then(serde_json::Value::as_u64)
                .unwrap_or_default()
        };
        match kind {
            EventKind::RunStarted => {
                self.sent(
                    run,
                    &text("task"),
                    &text("goal"),
                    kernel::BudgetCap {
                        usd: kernel::UsdMicros::new(count("usd_micros")),
                        tokens: kernel::Tokens::new(count("tokens")),
                    },
                );
            }
            EventKind::ApprovalRequested => {
                let Ok(item) = serde_json::from_value::<kernel::ApprovalItem>(
                    serde_json::Value::Object(data.clone()),
                ) else {
                    return;
                };
                // What this item is holding up, joined here rather than
                // hunted for later. Both halves come from the history:
                // the room from this record's envelope, the work from
                // the `run_started` of the run that raised it.
                if let (Some(addr), Some(sent)) = (addr, self.sent.get(&run)) {
                    self.origins.insert(
                        item.id.as_str().to_owned(),
                        BlockedJob {
                            addr: addr.clone(),
                            task: sent.task.clone(),
                            goal: sent.goal.clone(),
                            budget: sent.budget,
                        },
                    );
                }
                self.pending.insert(item.id.as_str().to_owned(), item);
            }
            EventKind::ApprovalResolved => {
                if let Some(id) = data.get("id").and_then(serde_json::Value::as_str) {
                    self.pending.remove(id);
                    self.origins.remove(id);
                }
                let allowed =
                    data.get("verdict").and_then(serde_json::Value::as_str) == Some("allow");
                if allowed
                    && let Some(cluster) = data.get("cluster")
                    && let Ok(key) = serde_json::from_value::<kernel::ClusterKey>(cluster.clone())
                {
                    self.granted.push(key);
                }
            }
            EventKind::AutonomyChanged => {
                if let Some(name) = data.get("autonomy").and_then(serde_json::Value::as_str) {
                    self.autonomy = read_autonomy(name);
                }
            }
            // One kind for both directions: halting and releasing are
            // one fact changing value, and a second kind would let a
            // reader see a release with no halt before it.
            EventKind::CityHalted => {
                let Some(scope) = data.get("scope").and_then(serde_json::Value::as_str) else {
                    return;
                };
                if data.get("state").and_then(serde_json::Value::as_str) == Some(HALTED) {
                    self.halted.insert(scope.to_owned());
                } else {
                    self.halted.remove(scope);
                }
            }
            _ => {}
        }
    }
}

/// The value of a halt record's `state` field when the scope is shut.
pub(super) const HALTED: &str = "halted";

/// And when it is open again.
pub(super) const RELEASED: &str = "released";

/// Everything a worker inherits from a history it did not write.
pub(crate) struct Standing {
    pub(crate) book: gateway::EndpointBook,
    pub(super) governance: Governance,
    pub(super) collaboration: Collaboration,
}

impl Standing {
    /// One verified pass, three folds.
    ///
    /// Until this existed the three were three functions, and opening a
    /// worker read, parsed and chain-verified the same bytes three times
    /// over to answer three questions about them. The answers never
    /// disagreed, which `what_a_worker_holds_is_what_a_restart_rebuilds`
    /// is what now holds, so the two extra passes bought nothing but the
    /// time and the memory of reading a whole history twice more.
    ///
    /// A line is parsed once here and shown to each fold. Verification
    /// stays where it was: a history that does not verify is not one any
    /// of these three views may be built from.
    ///
    /// # Errors
    /// Propagates chain verification and whatever a fold says about a
    /// payload it cannot read.
    pub(crate) fn fold(ledger_dir: &Path) -> Result<Standing, AxError> {
        let mut book = gateway::EndpointBook::new();
        let mut governance = Governance::empty();
        let mut collaboration = CollaborationFold::default();
        if ledger_dir.exists() {
            let verified = runtime::replay::verify_ledger_dir(ledger_dir)?;
            for line in verified.raw_lines() {
                let record = EventRecord::parse_line(line)?;
                book.apply(&record)?;
                governance.absorb(record.kind(), record.run(), record.addr(), record.data());
                collaboration.absorb(&record)?;
            }
        }
        Ok(Standing {
            book,
            governance,
            collaboration: collaboration.settle()?,
        })
    }
}

/// Rebuilds the views from the ledger on disk. This is the disposability
/// of a projection exercised on every start: nothing is persisted, and
/// the answer is the same as if the process had been running all along.
///
/// # Errors
/// Propagates chain verification failures; a city whose history does not
/// verify is not one whose views should be served.
pub(crate) fn rebuild_views(ledger_dir: &Path) -> Result<Views, AxError> {
    let verified = runtime::replay::verify_ledger_dir(ledger_dir)?;
    let city_root = ledger_dir
        .parent()
        .and_then(Path::parent)
        .unwrap_or(ledger_dir);
    let mut views = Views::new(city_root);
    for line in verified.raw_lines() {
        let record = EventRecord::parse_line(line)?;
        views.apply(&record)?;
    }
    Ok(views)
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

    /// What a working worker holds and what a restarted one rebuilds are
    /// one thing folded by one rule, or they are two things that happen
    /// to agree. This asks which.
    #[test]
    fn what_a_worker_holds_is_what_a_restart_rebuilds() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        std::fs::create_dir_all(dir.path().join("market").join("ito")).unwrap();
        std::fs::create_dir_all(dir.path().join("market").join("hana")).unwrap();
        lay_rules(
            dir.path(),
            "market",
            "# BUILDING.md\n\n`confidential: false`\n",
        );

        let (base_url, _provider) = fake_openai(
            &["m-local"],
            vec![
                tool_completion(
                    "asking hana",
                    "tu_1",
                    "signal",
                    serde_json::json!({
                        "action": "send",
                        "to": "market/hana",
                        "text": "what is your rate?",
                    }),
                ),
                completion("asked", None),
                completion("nothing to add", None),
            ],
        );
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        worker
            .handle(channels::Command::Dispatch {
                addr: Address::parse("market/ito").unwrap(),
                task: "ask hana what she charges".to_owned(),
                goal: "a price".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"dispatch"),
                session: None,
                effort: None,
            })
            .unwrap();

        let rebuilt = Standing::fold(&report.ledger_dir).unwrap().collaboration;
        let live_queues: std::collections::BTreeMap<String, u32> = worker
            .inboxes
            .iter()
            .filter(|(_, queue)| queue.pending() > 0)
            .map(|(room, queue)| (room.as_str().to_owned(), queue.pending()))
            .collect();
        let rebuilt_queues: std::collections::BTreeMap<String, u32> = rebuilt
            .inboxes
            .iter()
            .filter(|(_, queue)| queue.pending() > 0)
            .map(|(room, queue)| (room.as_str().to_owned(), queue.pending()))
            .collect();
        assert_eq!(
            live_queues, rebuilt_queues,
            "a queue the working city holds is a queue a restart finds"
        );
        assert_eq!(
            worker.goals.len(),
            rebuilt.goals.len(),
            "the ground claimed is folded from one rule"
        );
        assert_eq!(
            worker.requests.len(),
            rebuilt.requests.len(),
            "the register of open requests is folded from one rule"
        );

        // What each waiting item is holding up. This is the half the
        // handoff left open: the origin is process memory, and a
        // restarted worker rebuilds from the ledger, so an item raised
        // before a restart and answered after it has to find the same
        // work. It does, because the origin is folded by `absorb` out of
        // the same two lines a restart reads.
        let item = kernel::ApprovalItem {
            id: kernel::ApprovalId::new("item-held").unwrap(),
            source: kernel::ApprovalSource::Gate,
            actor: "market/ito".to_owned(),
            action_desc: "ask hana what she charges".to_owned(),
            artifact: Locator::parse("file:market/ito@0000000000000000000000000000000000000000")
                .unwrap(),
            cluster_key: kernel::ClusterKey {
                class: kernel::ApprovalClass::DiscardEscalate,
                detail: "market/ito".to_owned(),
            },
            created: TimeMs::new(1),
            tainted: false,
        };
        let started = {
            let verified = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
            let mut first = None;
            for line in verified.raw_lines() {
                let record = EventRecord::parse_line(line).unwrap();
                if record.kind() == EventKind::RunStarted && first.is_none() {
                    first = Some(record.run());
                }
            }
            first.expect("a dispatch starts a run")
        };
        worker
            .record_for(
                started,
                effect::Line {
                    who: "market/ito".to_owned(),
                    addr: Address::parse("market/ito").unwrap(),
                    kind: EventKind::ApprovalRequested,
                    data: Payload::new(
                        serde_json::to_value(&item)
                            .unwrap()
                            .as_object()
                            .cloned()
                            .unwrap(),
                    )
                    .unwrap(),
                },
            )
            .unwrap();
        let refolded = Standing::fold(&report.ledger_dir).unwrap().governance;
        assert_eq!(
            worker.governance.origins, refolded.origins,
            "what a waiting item is holding up is folded from one rule, so an answer after a \
             restart resumes the work an answer before it would have"
        );
        assert_eq!(
            worker.governance.origins.get("item-held"),
            Some(&BlockedJob {
                addr: Address::parse("market/ito").unwrap(),
                task: "ask hana what she charges".to_owned(),
                goal: "a price".to_owned(),
                budget: kernel::BudgetCap::default(),
            }),
            "and the comparison above is not two empty maps agreeing"
        );
    }

    /// The live page could see nothing from before it opened, because
    /// the server broadcasts and never backfills.
    #[test]
    fn a_page_can_ask_for_the_history_that_happened_before_it_opened() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let mut worker = RunWorker::new(
            dir.path(),
            gateway::Custodian::in_memory(),
            runtime::diagnostics::Diagnostics::off(),
        )
        .unwrap();
        for n in 0..6u8 {
            worker
                .handle(channels::Command::CreateBuilding {
                    addr: Address::parse(&format!("lab{n}")).unwrap(),
                    template: channels::TemplateName::parse("minimal").unwrap(),
                    idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, &[n]),
                })
                .unwrap();
        }
        let mut views = rebuild_views(&report.ledger_dir).unwrap();

        let channels::Answer::History(tail) = views.answer(&channels::Query::History {
            before: None,
            limit: 3,
        }) else {
            panic!("the history query has an answer");
        };
        assert_eq!(tail.records.len(), 3);
        let seqs: Vec<u64> = tail.records.iter().map(|r| r.seq().value()).collect();
        let mut ascending = seqs.clone();
        ascending.sort_unstable();
        assert_eq!(seqs, ascending, "oldest first: that is the fold's order");
        let earlier = tail.earlier.expect("there is more behind this slice");

        // Paging back reaches the genesis record and then says there is
        // nothing behind it, rather than answering an empty slice
        // forever.
        let channels::Answer::History(older) = views.answer(&channels::Query::History {
            before: Some(earlier),
            limit: channels::HISTORY_MAX,
        }) else {
            panic!("the history query has an answer");
        };
        assert_eq!(
            older.records[0].seq().value(),
            kernel::Seq::FIRST.value(),
            "paging back reaches the genesis line, which is sequence zero"
        );
        assert!(
            older.earlier.is_none(),
            "the first record has nothing behind it"
        );
        assert!(
            older.records.last().map(|r| r.seq().value()) < seqs.first().copied(),
            "the two slices do not overlap"
        );
    }

    /// Opening a session that started before this tab did.
    ///
    /// `Query::History` carries no run, so four sessions divided one
    /// bounded slice between them and a session older than the slice was
    /// not in it at all - the page for it was blank. This asks for one
    /// session and gets that session.
    #[test]
    fn one_session_can_be_asked_for_by_itself_rather_than_filtered_out_of_the_city() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let mut worker = RunWorker::new(
            dir.path(),
            gateway::Custodian::in_memory(),
            runtime::diagnostics::Diagnostics::off(),
        )
        .unwrap();
        for n in 0..6u8 {
            worker
                .handle(channels::Command::CreateBuilding {
                    addr: Address::parse(&format!("lab{n}")).unwrap(),
                    template: channels::TemplateName::parse("minimal").unwrap(),
                    idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, &[n]),
                })
                .unwrap();
        }
        let mut views = rebuild_views(&report.ledger_dir).unwrap();

        // Everything a fresh city writes belongs to the city's own run,
        // so asking for it gets those records and asking for a session
        // nobody ever opened gets none of them.
        let channels::Answer::History(mine) = views.answer(&channels::Query::RunHistory {
            run: RunId::CITY,
            before: None,
            limit: channels::HISTORY_MAX,
        }) else {
            panic!("the run history query has an answer");
        };
        assert!(!mine.records.is_empty(), "the city's own run wrote these");
        assert!(
            mine.records.iter().all(|held| held.run() == RunId::CITY),
            "a session's history holds only that session"
        );
        let seqs: Vec<u64> = mine.records.iter().map(|r| r.seq().value()).collect();
        let mut ascending = seqs.clone();
        ascending.sort_unstable();
        assert_eq!(seqs, ascending, "oldest first, as the fold expects");

        let channels::Answer::History(stranger) = views.answer(&channels::Query::RunHistory {
            run: RunId::from_bytes([3u8; 16]),
            before: None,
            limit: channels::HISTORY_MAX,
        }) else {
            panic!("the run history query has an answer");
        };
        assert!(
            stranger.records.is_empty(),
            "a session this city never ran has no history in it"
        );
    }

    /// Stopping on the limit has to be told apart from reaching the
    /// beginning of the session, or a busy city reads as a session with
    /// nothing in it.
    #[test]
    fn a_run_history_that_stopped_early_says_where_to_resume_rather_than_that_it_ended() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let mut worker = RunWorker::new(
            dir.path(),
            gateway::Custodian::in_memory(),
            runtime::diagnostics::Diagnostics::off(),
        )
        .unwrap();
        for n in 0..6u8 {
            worker
                .handle(channels::Command::CreateBuilding {
                    addr: Address::parse(&format!("lab{n}")).unwrap(),
                    template: channels::TemplateName::parse("minimal").unwrap(),
                    idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, &[n]),
                })
                .unwrap();
        }
        let mut views = rebuild_views(&report.ledger_dir).unwrap();

        // One record at a time, so the walk stops on the limit well
        // before it reaches the genesis line.
        let channels::Answer::History(page) = views.answer(&channels::Query::RunHistory {
            run: RunId::CITY,
            before: None,
            limit: 1,
        }) else {
            panic!("the run history query has an answer");
        };
        assert_eq!(page.records.len(), 1);
        let resume = page
            .earlier
            .expect("stopping on the limit is not reaching the beginning");
        assert!(
            resume.value() <= page.records[0].seq().value(),
            "resuming must not skip the records between"
        );

        // And paging with it does reach the beginning, which is the
        // other statement `earlier` has to be able to make.
        let mut before = Some(resume);
        let mut guard = 0;
        while let Some(at) = before {
            let channels::Answer::History(page) = views.answer(&channels::Query::RunHistory {
                run: RunId::CITY,
                before: Some(at),
                limit: 1,
            }) else {
                panic!("the run history query has an answer");
            };
            before = page.earlier;
            guard += 1;
            assert!(guard < 1_000, "paging back does not terminate");
        }
    }

    /// A person could not see what a key bought until they had already
    /// registered it, and could not register part of what it bought.
    #[test]
    fn a_provider_can_be_asked_what_it_serves_and_only_part_of_it_admitted() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let (base_url, _provider) = fake_openai(&["m-small", "m-large"], Vec::new());
        let mut worker = RunWorker::new(
            dir.path(),
            gateway::Custodian::in_memory(),
            runtime::diagnostics::Diagnostics::off(),
        )
        .unwrap();

        worker
            .handle(channels::Command::ProbeEndpoint {
                name: channels::ProviderName::parse("house").unwrap(),
                base_url: base_url.clone(),
                dialect: kernel::DialectKind::OpenAi,
                secret: None,
                auth_header: None,
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"probe"),
            })
            .unwrap();
        let after_probe = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
        let probed: String = after_probe
            .raw_lines()
            .iter()
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .collect::<Vec<String>>()
            .join("\n");
        assert!(probed.contains("endpoint_probed"), "{probed}");
        assert!(probed.contains("m-small") && probed.contains("m-large"));
        assert!(
            !probed.contains("endpoint_attached"),
            "asking what a provider serves attached it anyway"
        );

        worker
            .handle(channels::Command::AttachEndpoint {
                name: channels::ProviderName::parse("house").unwrap(),
                base_url,
                dialect: kernel::DialectKind::OpenAi,
                secret: None,
                auth_header: None,
                admit: vec!["m-large".to_owned()],
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"attach"),
            })
            .unwrap();
        let channels::Answer::Endpoints(book) = rebuild_views(&report.ledger_dir)
            .unwrap()
            .answer(&channels::Query::EndpointView)
        else {
            panic!("the settings page reads the endpoint book");
        };
        assert_eq!(
            book.endpoints[0].models,
            vec!["m-large".to_owned()],
            "a subset was ticked and the whole list was registered anyway"
        );
    }

    /// Halting is admission control: it refuses new work and says which
    /// scope refused, and a release opens the same scope again. Both
    /// survive a restart, because the worker folds them from the ledger
    /// rather than holding them only in memory.
    #[test]
    fn a_halted_scope_refuses_new_work_and_a_release_takes_it_again() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        let room = Address::parse("lab/room1").unwrap();
        let (base_url, _provider) = fake_openai(&["m-local"], vec![completion("done", None)]);
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        let work = |tag: &[u8]| channels::Command::Dispatch {
            addr: room.clone(),
            task: "measure the thing".to_owned(),
            goal: "a number, then stop".to_owned(),
            mode: channels::ModeTag::parse("plan").unwrap(),
            budget: kernel::BudgetCap::default(),
            idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, tag),
            session: None,
            effort: None,
        };
        let halt = |scope| channels::Command::Halt {
            scope,
            idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"halt"),
        };

        worker
            .handle(halt(channels::HaltScope::Building(
                Address::parse("lab").unwrap(),
            )))
            .unwrap();
        let refused = worker.handle(work(b"one")).unwrap_err();
        assert_eq!(refused.code(), &AxCode::GateDenied);
        assert!(
            refused.recovery().contains("release"),
            "a refusal says how to undo the thing that caused it: {}",
            refused.recovery()
        );
        assert!(
            !city::job_path(dir.path(), &room).exists(),
            "a refused dispatch leaves no task in a room no run opened"
        );

        // A different building is not covered by that halt.
        worker
            .handle(channels::Command::CreateBuilding {
                addr: Address::parse("shop").unwrap(),
                template: channels::TemplateName::parse("minimal").unwrap(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"create"),
            })
            .unwrap();
        let mut elsewhere = work(b"two");
        if let channels::Command::Dispatch { addr, .. } = &mut elsewhere {
            *addr = Address::parse("shop/room1").unwrap();
        }
        worker.handle(elsewhere).unwrap();

        worker
            .handle(channels::Command::Release {
                scope: channels::HaltScope::Building(Address::parse("lab").unwrap()),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"release"),
            })
            .unwrap();
        worker.handle(work(b"three")).unwrap();

        // The posture is history, not a field: a second worker over the
        // same ledger knows the building is open again.
        let restarted = Standing::fold(&ledger_dir(dir.path())).unwrap().governance;
        assert!(restarted.halted.is_empty());
    }
}
