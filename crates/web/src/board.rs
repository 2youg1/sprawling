// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The plan tree, laid out by state (v0.0.3 card V3.23).
//!
//! **The board holds no state of its own.** Every column is read from
//! `BuildingAnswer.plan` on each render, and there is nothing here that
//! could move a node. A face that could drag a card into `Done` would be
//! a second writer of the one table every progress figure in this city
//! is divided by, and the moment there are two writers there are two
//! denominators. What a person can do from here is send somebody to a
//! node that is ready, which is the ordinary dispatch and the authority
//! it has always had.
//!
//! **Only leaves are drawn.** A branch's work is its children, and a
//! board that showed both would show the same work twice — which is the
//! same rule `kernel::plan` counts progress by, made visible.
//!
//! **Five columns, because they are five different things to do.**
//! Ready is work to hand out; waiting is work whose turn has not come;
//! working is somebody's; blocked needs a person or another branch; done
//! is finished with evidence. Folding waiting into ready would offer
//! somebody a door that is locked.

use channels::{BuildingAnswer, PlanRow, RoadmapStatus};
use dioxus::prelude::*;

use crate::lang::{Lang, Msg, fill, say};

/// Which column a node stands in.
///
/// Exhaustive and derived, never stored: two boards reading one plan
/// draw the same thing, and there is no state here to disagree with the
/// table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Column {
    Ready,
    Waiting,
    Working,
    Blocked,
    Done,
}

impl Column {
    /// The five, left to right: what can start, what cannot yet, what is
    /// under way, what has stopped, what is finished.
    pub const ALL: [Column; 5] = [
        Column::Ready,
        Column::Waiting,
        Column::Working,
        Column::Blocked,
        Column::Done,
    ];

    /// The class this column's cards carry, which is also what the
    /// settled screen calls it.
    #[must_use]
    pub fn token(self) -> &'static str {
        match self {
            Column::Ready => "ready",
            Column::Waiting => "quiet",
            Column::Working => "running",
            Column::Blocked => "alert",
            Column::Done => "done",
        }
    }

    #[must_use]
    pub fn heading(self) -> Msg {
        match self {
            Column::Ready => Msg::BoardReady,
            Column::Waiting => Msg::BoardWaiting,
            Column::Working => Msg::BoardWorking,
            Column::Blocked => Msg::BoardBlocked,
            Column::Done => Msg::BoardDone,
        }
    }
}

/// Where one node stands.
///
/// `ready` is the server's answer rather than this page's: whether a
/// node can be started is `kernel::plan`'s decision, and a client that
/// worked it out from the dependency list would be the second place that
/// rule lived.
#[must_use]
pub fn column_of(row: &PlanRow) -> Column {
    match row.status {
        RoadmapStatus::Done => Column::Done,
        RoadmapStatus::Blocked | RoadmapStatus::AwaitingApproval => Column::Blocked,
        RoadmapStatus::InProgress => Column::Working,
        RoadmapStatus::NotStarted if row.ready => Column::Ready,
        RoadmapStatus::NotStarted => Column::Waiting,
    }
}

/// The leaves of one column, in the plan's own order.
#[must_use]
pub fn column(plan: &[PlanRow], which: Column) -> Vec<&PlanRow> {
    plan.iter()
        .filter(|row| row.leaf && column_of(row) == which)
        .collect()
}

/// A node's share of the whole plan, as whole percent.
///
/// Whole percent because a share is an estimate somebody made, and a
/// second decimal place on an estimate reads as a measurement. Integer
/// arithmetic throughout: the share arrives in billionths for exactly
/// this reason.
#[must_use]
pub fn percent(share_ppb: u64) -> u64 {
    share_ppb
        .saturating_mul(100)
        .checked_div(channels::WHOLE_PPB)
        .unwrap_or(0)
}

/// What a node is waiting for, as one clause.
#[must_use]
pub fn waits_for(row: &PlanRow) -> String {
    row.needs
        .iter()
        .map(channels::NodeId::to_string)
        .collect::<Vec<String>>()
        .join(", ")
}

/// The plan tree, by state.
#[component]
pub fn BoardView(answer: BuildingAnswer) -> Element {
    let lang = use_context::<Signal<Lang>>();
    let word = move |msg: Msg| say(lang(), msg);
    let leaves = answer.plan.iter().filter(|row| row.leaf).count();

    rsx! {
        if !answer.blocked.is_empty() {
            crate::panel::Panel {
                title: word(Msg::BoardStuck).to_owned(),
                scope: Some(word(Msg::BoardStuckScope).to_owned()),
                figure: Some(answer.blocked.len().to_string()),
                source: word(Msg::BoardStuckSource).to_owned(),
                for line in answer.blocked.clone() {
                    p { key: "{line.source.as_str()}", class: "blocked-line",
                        span {
                            class: "phase alert",
                            role: "img",
                            "aria-label": "{word(Msg::BoardBlocked)}",
                        }
                        span { class: "said", "{line.line}" }
                        span { class: "turn",
                            {
                                fill(
                                    word(Msg::BoardWaitingBehind),
                                    &[("n", &line.waiting.to_string())],
                                )
                            }
                        }
                    }
                }
            }
        }
        crate::panel::Panel {
            title: word(Msg::BoardTitle).to_owned(),
            scope: Some(word(Msg::BoardScope).to_owned()),
            figure: Some(leaves.to_string()),
            source: word(Msg::BoardSource).to_owned(),
            if answer.plan.is_empty() {
                crate::panel::Empty {
                    status: word(Msg::BoardEmpty).to_owned(),
                    what: word(Msg::BoardEmptyWhat).to_owned(),
                }
            } else {
                div { class: "board-columns",
                    for which in Column::ALL {
                        div { key: "{which.token()}", class: "board-column",
                            h3 { class: "board-heading",
                                "{word(which.heading())}"
                                span { class: "badge", "{column(&answer.plan, which).len()}" }
                            }
                            for row in column(&answer.plan, which) {
                                div {
                                    key: "{row.node.as_str()}",
                                    class: "board-card {which.token()}",
                                    span { class: "board-node", "{row.node.as_str()}" }
                                    span { class: "board-item", "{row.item}" }
                                    if which == Column::Waiting {
                                        span { class: "board-waits",
                                            {
                                                fill(
                                                    word(Msg::BoardWaitsFor),
                                                    &[("nodes", &waits_for(row))],
                                                )
                                            }
                                        }
                                    } else {
                                        span { class: "board-share", "{percent(row.share_ppb)}%" }
                                    }
                                }
                            }
                        }
                    }
                }
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
    use super::{Column, column, column_of, percent, waits_for};
    use channels::{NodeId, PlanRow, RoadmapStatus};

    fn row(node: &str, status: RoadmapStatus, ready: bool, leaf: bool) -> PlanRow {
        PlanRow {
            node: NodeId::parse(node).unwrap(),
            item: format!("item {node}"),
            status,
            share_ppb: 125_000_000,
            needs: Vec::new(),
            ready,
            leaf,
            evidence: None,
        }
    }

    /// A node nobody has started is two different things depending on
    /// whether it can be started, and folding them together would offer
    /// somebody a door that is locked.
    #[test]
    fn a_node_that_cannot_start_yet_is_not_in_the_ready_column() {
        assert_eq!(
            column_of(&row("1", RoadmapStatus::NotStarted, true, true)),
            Column::Ready
        );
        assert_eq!(
            column_of(&row("2", RoadmapStatus::NotStarted, false, true)),
            Column::Waiting
        );
    }

    #[test]
    fn every_status_lands_in_exactly_one_column() {
        for (status, expected) in [
            (RoadmapStatus::InProgress, Column::Working),
            (RoadmapStatus::Blocked, Column::Blocked),
            (RoadmapStatus::AwaitingApproval, Column::Blocked),
            (RoadmapStatus::Done, Column::Done),
        ] {
            assert_eq!(column_of(&row("1", status, false, true)), expected);
        }
    }

    /// A branch's work is its children. Drawing both would show the same
    /// work twice, which is the rule progress is counted by.
    #[test]
    fn a_branch_is_not_a_card() {
        let plan = vec![
            row("1", RoadmapStatus::NotStarted, false, false),
            row("1.1", RoadmapStatus::NotStarted, true, true),
        ];
        let ready = column(&plan, Column::Ready);
        assert_eq!(ready.len(), 1);
        assert_eq!(ready[0].node.as_str(), "1.1");
        assert!(column(&plan, Column::Waiting).is_empty());
    }

    #[test]
    fn a_share_is_drawn_as_whole_percent() {
        assert_eq!(percent(125_000_000), 12);
        assert_eq!(percent(channels::WHOLE_PPB), 100);
        assert_eq!(percent(0), 0);
    }

    #[test]
    fn what_a_node_waits_for_is_named_rather_than_counted() {
        let mut held = row("3", RoadmapStatus::NotStarted, false, true);
        held.needs = vec![NodeId::parse("1").unwrap(), NodeId::parse("2.1").unwrap()];
        assert_eq!(waits_for(&held), "1, 2.1");
    }
}
