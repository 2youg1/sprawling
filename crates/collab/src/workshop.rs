// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! One creation split into nodes, and the order they run in.
//!
//! A node contract is the whole task authority for the agent that takes
//! the node: what to reach, what it may read at which version, where it
//! may write, how done is decided, and when to stop. Written to disk it
//! is that node's `JOB.md`, so the mechanism costs nothing in the prefix.
//!
//! Scheduling is deterministic because replay is: the same graph and the
//! same ledger must produce the same order, or a failure cannot be
//! reproduced. Ties are broken by node id, and every collection here is
//! ordered for that reason.
//!
//! The graph's authority is the building's `Roadmap.md`. This module
//! keeps no second store of what the plan is; it is handed contracts and
//! answers what may run.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use kernel::{Address, AxCode, AxError, BudgetCap, Locator, Version};

/// A node's name within one workshop.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct NodeId(String);

impl NodeId {
    /// # Errors
    /// Refuses an empty id and one carrying whitespace: the id names a
    /// directory and a ledger field.
    pub fn parse(raw: &str) -> Result<NodeId, AxError> {
        if raw.is_empty() || raw.chars().any(char::is_whitespace) {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "read a node id",
                format!("{raw:?}"),
            )
            .with_recovery("use a non-empty id with no whitespace"));
        }
        Ok(NodeId(raw.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One node's contract: the whole of what its agent is answerable for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NodeContract {
    id: NodeId,
    goal: String,
    depends_on: BTreeSet<NodeId>,
    reads: Vec<(Locator, Version)>,
    write_domain: Address,
    owner: String,
    done_check: String,
    budget: BudgetCap,
    stop: String,
}

impl NodeContract {
    /// Sole constructor.
    ///
    /// # Errors
    /// Refuses a contract with no owner, no goal, no stop condition or
    /// no done check. Each of the four is what somebody would otherwise
    /// have to guess, and a guessed stop condition is a run that does
    /// not stop.
    #[allow(
        clippy::too_many_arguments,
        reason = "the contract is nine fields by design; a builder would hide which are required"
    )]
    pub fn new(
        id: NodeId,
        goal: String,
        depends_on: BTreeSet<NodeId>,
        reads: Vec<(Locator, Version)>,
        write_domain: Address,
        owner: String,
        done_check: String,
        budget: BudgetCap,
        stop: String,
    ) -> Result<NodeContract, AxError> {
        for (field, value) in [
            ("goal", &goal),
            ("owner", &owner),
            ("done_check", &done_check),
            ("stop", &stop),
        ] {
            if value.trim().is_empty() {
                return Err(AxError::failure(
                    AxCode::InvalidArgs,
                    "build a node contract",
                    format!("{}: {field} is empty", id.as_str()),
                )
                .with_recovery(
                    "state it; a node whose stop condition or done check is unwritten is one \
                     somebody has to guess at, and they will guess differently",
                ));
            }
        }
        Ok(NodeContract {
            id,
            goal,
            depends_on,
            reads,
            write_domain,
            owner,
            done_check,
            budget,
            stop,
        })
    }

    #[must_use]
    pub fn id(&self) -> &NodeId {
        &self.id
    }

    #[must_use]
    pub fn goal(&self) -> &str {
        &self.goal
    }

    pub fn depends_on(&self) -> impl Iterator<Item = &NodeId> {
        self.depends_on.iter()
    }

    /// What this node may read, each pinned to a version.
    #[must_use]
    pub fn reads(&self) -> &[(Locator, Version)] {
        &self.reads
    }

    #[must_use]
    pub fn write_domain(&self) -> &Address {
        &self.write_domain
    }

    #[must_use]
    pub fn owner(&self) -> &str {
        &self.owner
    }

    #[must_use]
    pub fn done_check(&self) -> &str {
        &self.done_check
    }

    #[must_use]
    pub fn budget(&self) -> BudgetCap {
        self.budget
    }

    #[must_use]
    pub fn stop(&self) -> &str {
        &self.stop
    }

    /// The body of this node's `JOB.md`: the contract itself, because
    /// the agent's task authority and this record are the same thing.
    #[must_use]
    pub fn job_text(&self) -> String {
        let mut text = format!(
            "## Task\n\n{}\n\n## Goal\n\n{}\n\n## Done check\n\n{}\n\n## Stop\n\n{}\n",
            self.goal, self.goal, self.done_check, self.stop
        );
        if !self.reads.is_empty() {
            text.push_str("\n## Reads\n\n");
            for (locator, version) in &self.reads {
                text.push_str(&format!("- {locator} @ v{}\n", version.value()));
            }
        }
        text.push_str(&format!(
            "\n## Write domain\n\n- {}\n",
            self.write_domain.as_str()
        ));
        text
    }
}

/// A graph of contracts that can actually be run.
#[derive(Debug)]
pub struct Workshop {
    nodes: BTreeMap<NodeId, NodeContract>,
}

impl Workshop {
    /// # Errors
    /// Refuses a duplicate id, a dependency on a node that is not in the
    /// graph, and a cycle. All three are refused at construction, so a
    /// workshop that exists is one that can finish.
    pub fn new(contracts: Vec<NodeContract>) -> Result<Workshop, AxError> {
        let mut nodes = BTreeMap::new();
        for contract in contracts {
            if nodes.contains_key(contract.id()) {
                return Err(AxError::failure(
                    AxCode::InvalidArgs,
                    "build a workshop",
                    contract.id().as_str().to_owned(),
                )
                .with_recovery("two contracts claim one node id; rename one"));
            }
            nodes.insert(contract.id().clone(), contract);
        }
        let workshop = Workshop { nodes };
        for contract in workshop.nodes.values() {
            for dependency in contract.depends_on() {
                if !workshop.nodes.contains_key(dependency) {
                    return Err(AxError::failure(
                        AxCode::InvalidArgs,
                        "build a workshop",
                        format!(
                            "{} depends on {}, which is not in the graph",
                            contract.id().as_str(),
                            dependency.as_str()
                        ),
                    )
                    .with_recovery("add that node, or drop the dependency"));
                }
            }
        }
        // A cycle is detected by the schedule failing to cover the graph.
        let order = workshop.order();
        if order.len() != workshop.nodes.len() {
            let stuck: Vec<String> = workshop
                .nodes
                .keys()
                .filter(|id| !order.contains(id))
                .map(|id| id.as_str().to_owned())
                .collect();
            return Err(AxError::failure(
                AxCode::GoalConflict,
                "build a workshop",
                stuck.join(", "),
            )
            .with_recovery(
                "these nodes wait on each other; break the cycle by deciding which one goes first",
            ));
        }
        Ok(workshop)
    }

    /// The order the nodes run in. Deterministic: dependencies first,
    /// ties broken by id, so the same graph schedules the same way on
    /// every machine and every replay.
    #[must_use]
    pub fn schedule(&self) -> Vec<NodeId> {
        self.order()
    }

    /// What may start now, given what has finished. Several nodes can be
    /// ready at once; that is the fan-out.
    #[must_use]
    pub fn ready(&self, done: &BTreeSet<NodeId>) -> Vec<NodeId> {
        self.nodes
            .values()
            .filter(|contract| !done.contains(contract.id()))
            .filter(|contract| contract.depends_on().all(|need| done.contains(need)))
            .map(|contract| contract.id().clone())
            .collect()
    }

    #[must_use]
    pub fn contract(&self, id: &NodeId) -> Option<&NodeContract> {
        self.nodes.get(id)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.nodes.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.nodes.is_empty()
    }

    /// Kahn's algorithm over ordered collections: the queue is drained in
    /// id order, so the result is a function of the graph alone.
    fn order(&self) -> Vec<NodeId> {
        let mut remaining: BTreeMap<NodeId, usize> = self
            .nodes
            .values()
            .map(|contract| (contract.id().clone(), contract.depends_on().count()))
            .collect();
        let mut queue: VecDeque<NodeId> = remaining
            .iter()
            .filter(|(_, count)| **count == 0)
            .map(|(id, _)| id.clone())
            .collect();
        let mut out = Vec::new();
        while let Some(id) = queue.pop_front() {
            remaining.remove(&id);
            out.push(id.clone());
            let mut freed: Vec<NodeId> = Vec::new();
            for (candidate, count) in &mut remaining {
                if self
                    .nodes
                    .get(candidate)
                    .is_some_and(|contract| contract.depends_on().any(|need| *need == id))
                {
                    *count = count.saturating_sub(1);
                    if *count == 0 {
                        freed.push(candidate.clone());
                    }
                }
            }
            for id in freed {
                queue.push_back(id);
            }
        }
        out
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

    fn node(id: &str, deps: &[&str]) -> NodeContract {
        NodeContract::new(
            NodeId::parse(id).unwrap(),
            format!("produce {id}"),
            deps.iter()
                .map(|d| NodeId::parse(d).unwrap())
                .collect::<BTreeSet<NodeId>>(),
            Vec::new(),
            Address::parse(&format!("lab/{id}")).unwrap(),
            "lab/room1".to_owned(),
            "the test suite passes".to_owned(),
            BudgetCap::default(),
            "stop when the check passes".to_owned(),
        )
        .unwrap()
    }

    #[test]
    fn the_same_graph_schedules_the_same_way_every_time() {
        let first = Workshop::new(vec![
            node("c", &["a", "b"]),
            node("a", &[]),
            node("b", &["a"]),
        ])
        .unwrap();
        // The same nodes, handed over in a different order.
        let second = Workshop::new(vec![
            node("b", &["a"]),
            node("c", &["a", "b"]),
            node("a", &[]),
        ])
        .unwrap();

        let order: Vec<String> = first
            .schedule()
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect();
        assert_eq!(order, ["a", "b", "c"]);
        assert_eq!(first.schedule(), second.schedule());
    }

    #[test]
    fn independent_nodes_are_ready_together_and_that_is_the_fan_out() {
        let workshop =
            Workshop::new(vec![node("a", &[]), node("b", &[]), node("c", &["a", "b"])]).unwrap();
        let ready: Vec<String> = workshop
            .ready(&BTreeSet::new())
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect();
        assert_eq!(ready, ["a", "b"]);

        let done: BTreeSet<NodeId> = ["a", "b"]
            .iter()
            .map(|d| NodeId::parse(d).unwrap())
            .collect();
        let ready: Vec<String> = workshop
            .ready(&done)
            .iter()
            .map(|id| id.as_str().to_owned())
            .collect();
        assert_eq!(ready, ["c"]);
    }

    #[test]
    fn a_cycle_is_refused_at_construction_and_names_the_nodes_in_it() {
        let err = Workshop::new(vec![node("a", &["b"]), node("b", &["a"])]).unwrap_err();
        assert_eq!(err.code(), &AxCode::GoalConflict);
        assert!(err.subject().contains('a') && err.subject().contains('b'));
    }

    #[test]
    fn a_dependency_on_a_node_nobody_wrote_is_refused() {
        let err = Workshop::new(vec![node("a", &["ghost"])]).unwrap_err();
        assert!(err.subject().contains("ghost"));
        assert!(err.recovery().contains("drop the dependency"));
    }

    #[test]
    fn two_contracts_cannot_claim_one_node() {
        let err = Workshop::new(vec![node("a", &[]), node("a", &[])]).unwrap_err();
        assert!(err.recovery().contains("rename one"));
    }

    #[test]
    fn a_contract_that_leaves_the_stop_condition_to_the_reader_is_refused() {
        let err = NodeContract::new(
            NodeId::parse("a").unwrap(),
            "produce a".to_owned(),
            BTreeSet::new(),
            Vec::new(),
            Address::parse("lab/a").unwrap(),
            "lab/room1".to_owned(),
            "the suite passes".to_owned(),
            BudgetCap::default(),
            "   ".to_owned(),
        )
        .unwrap_err();
        assert!(err.subject().contains("stop"));
    }

    #[test]
    fn the_contract_is_the_job_file_so_the_agent_reads_one_authority() {
        let contract = node("a", &[]);
        let text = contract.job_text();
        assert!(text.contains("## Goal"));
        assert!(text.contains("## Done check"));
        assert!(text.contains("the test suite passes"));
        assert!(text.contains("## Stop"));
        assert!(text.contains("lab/a"), "the node writes where it may write");
    }
}
