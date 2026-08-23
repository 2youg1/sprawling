// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The first screen: how much of this city is working, and on what.
//!
//! Every other page answers a question a person already has. This one
//! answers the question they arrive with - *is anything happening, and does
//! any of it need me* - and it has to answer both in one glance or it has
//! not earned the position.
//!
//! **Two counts and no more.** A first screen that reports seven numbers
//! reports none of them: the eye lands on whichever is largest. So the
//! headline is one sentence with two numbers in it, and everything else on
//! the page is a list a person can walk into.
//!
//! **Nothing here is a new question to the city.** The page is folded from
//! the event stream and one city answer that other pages already ask for.
//! An overview that polled would be the most expensive page in the product
//! and would tell a person nothing the fold does not already know.

use channels::{Address, CityAnswer, ClientFrame, Query};
use dioxus::prelude::*;

use crate::app::{RunPhase, Snapshot, View};

/// How much of this city is working, and across how much of it.
///
/// `runs` counts work in flight - a run that is running or waiting on a
/// person. A frozen or halted run is not in flight, and counting it here
/// would make a stopped city look busy, which is the one thing a first
/// screen must never do.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Working {
    pub runs: usize,
    /// Buildings with at least one run in flight.
    pub buildings: usize,
    /// Buildings this city holds, whether or not anything is happening in
    /// them. Carried so the headline can say *two of five* rather than
    /// *two*, which is a different fact.
    pub raised: usize,
}

/// Reads the two counts off the fold.
///
/// A run's address is a room inside a building, so the building is its
/// first segment; two runs in two rooms of one building are one building
/// working, because a person reading this line is counting places, not
/// paths.
#[must_use]
pub fn working(snapshot: &Snapshot, city: Option<&CityAnswer>) -> Working {
    let mut buildings: std::collections::BTreeSet<&str> = std::collections::BTreeSet::new();
    let mut runs = 0usize;
    for (_, row) in snapshot.runs() {
        if !matches!(row.phase, RunPhase::Running | RunPhase::AwaitingApproval) {
            continue;
        }
        runs = runs.saturating_add(1);
        if let Some(addr) = row.addr.as_ref()
            && let Some(building) = addr.as_str().split('/').next()
        {
            buildings.insert(building);
        }
    }
    Working {
        runs,
        buildings: buildings.len(),
        raised: city.map_or(0, |answer| answer.buildings.len()),
    }
}

/// The one sentence the first screen exists to say.
///
/// It states the true case in each of its shapes rather than a template
/// with numbers substituted in: "nothing is running" and "0 runs in 0
/// buildings" are the same fact, and only one of them is a sentence.
#[must_use]
pub fn headline(working: &Working) -> String {
    match (working.runs, working.raised) {
        (0, 0) => "this city has no buildings yet".to_owned(),
        (0, raised) => format!("nothing is running in any of the {raised} building(s) here"),
        (1, _) => format!(
            "1 run in flight, in 1 of the {} building(s) here",
            working.raised
        ),
        (runs, raised) => format!(
            "{runs} runs in flight, across {} of the {raised} building(s) here",
            working.buildings
        ),
    }
}

/// One thing waiting on a person, and where they go to deal with it.
///
/// A count and a destination rather than a sentence with a link buried in
/// it: the row is the button.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Attention {
    pub what: String,
    pub count: u32,
    pub view: View,
}

/// What is waiting on a person, most blocking first.
///
/// The order is deliberate and is the order these become expensive: an
/// approval stops one run now; a frozen run is already stopped and will
/// stay stopped; a provider that cannot be reached stops everything that
/// has not started. A page that listed them by kind would put the cheapest
/// first as often as not.
#[must_use]
pub fn needs_you(snapshot: &Snapshot) -> Vec<Attention> {
    let mut rows = Vec::new();
    let waiting = snapshot.approvals_pending();
    if waiting > 0 {
        rows.push(Attention {
            what: "waiting for you to allow or refuse".to_owned(),
            count: waiting,
            view: View::Approvals,
        });
    }
    let unreadable = snapshot.unreadable_approvals();
    if unreadable > 0 {
        rows.push(Attention {
            // Counted and named rather than dropped: a queue quietly one
            // item short is wrong about the only thing a person came for.
            what: "approval record(s) this client could not read".to_owned(),
            count: unreadable,
            view: View::Approvals,
        });
    }
    let frozen = snapshot
        .runs()
        .filter(|(_, row)| matches!(row.phase, RunPhase::Frozen))
        .count();
    if let Ok(frozen) = u32::try_from(frozen)
        && frozen > 0
    {
        rows.push(Attention {
            what: "run(s) frozen, each holding a handoff for whoever resumes it".to_owned(),
            count: frozen,
            view: View::Live(None),
        });
    }
    if !matches!(
        snapshot.provider(),
        crate::app::ProviderHealth::Healthy | crate::app::ProviderHealth::Unknown
    ) {
        rows.push(Attention {
            what: format!("the provider is {}", snapshot.provider().as_str()),
            count: 1,
            view: View::Settings,
        });
    }
    rows
}

/// The first screen.
#[component]
pub fn OverviewView(
    snapshot: Snapshot,
    city: Option<CityAnswer>,
    /// Whether the socket is live; see `app::Root`.
    live: Signal<bool>,
    on_frame: EventHandler<ClientFrame>,
    on_view: EventHandler<View>,
    /// The way into one building's own pages, which the nav cannot carry.
    on_open: EventHandler<String>,
) -> Element {
    let asked = use_signal(|| false);
    use_effect(move || {
        let mut asked = asked;
        if live() && !asked() {
            asked.set(true);
            on_frame.call(ClientFrame::Query(Query::CityView));
        }
    });
    let working = working(&snapshot, city.as_ref());
    let attention = needs_you(&snapshot);
    let halted = snapshot.is_halted();
    let mut in_flight: Vec<(String, String, u32, Option<u32>)> = snapshot
        .runs()
        .filter(|(_, row)| !matches!(row.phase, RunPhase::Halted))
        .map(|(id, row)| {
            (
                row.addr.as_ref().map_or_else(
                    || crate::live::short_run(*id),
                    |addr| addr.as_str().to_owned(),
                ),
                row.phase.as_str().to_owned(),
                row.steps_done,
                row.steps_planned,
            )
        })
        .collect();
    in_flight.sort();
    rsx! {
        section { class: "overview",
            crate::panel::Panel {
                title: headline(&working),
                scope: "work in flight only: a frozen or halted run is listed below but is not counted here, because a stopped city must not read as a busy one"
                    .to_owned(),
                source: "folded from the event stream this page is already receiving, plus one city query asked when it opened. Nothing on this page is polled."
                    .to_owned(),
                if halted {
                    p { class: "problems",
                        "This city is halted: nothing new will start until it is released."
                    }
                }
                if attention.is_empty() {
                    crate::panel::Empty {
                        status: "nothing is waiting for you".to_owned(),
                        what: "a run reaches a person only when a gate refuses to decide by itself, or when it freezes with work left. Neither has happened."
                            .to_owned(),
                    }
                } else {
                    div { class: "attention",
                        for row in attention.clone() {
                            button {
                                key: "{row.what}",
                                class: "attention-row",
                                onclick: {
                                    let view = row.view.clone();
                                    move |_| on_view.call(view.clone())
                                },
                                span { class: "count", "{row.count}" }
                                span { class: "what", "{row.what}" }
                            }
                        }
                    }
                }
            }
            crate::panel::Panel {
                title: if in_flight.is_empty() { "no run has started in this city".to_owned() }
                    else { "what is being worked on".to_owned() },
                scope: "one row per run this client knows of, by where it is working; a halted run is left out because halting is a decision, not a state to watch"
                    .to_owned(),
                source: "the run's own events - started, each turn, frozen - as they arrived"
                    .to_owned(),
                if in_flight.is_empty() {
                    crate::panel::Empty {
                        status: "no work has been sent yet".to_owned(),
                        what: "send some from the bar at the bottom of the window: a room to work in, what to produce, and what counts as done. A run appears here the moment it starts."
                            .to_owned(),
                    }
                } else {
                    table { class: "in-flight",
                        thead {
                            tr {
                                th { "where" }
                                th { "phase" }
                                th { "steps" }
                            }
                        }
                        tbody {
                            for (place , phase , done , planned) in in_flight.clone() {
                                tr { key: "{place}-{phase}",
                                    td { class: "addr", "{place}" }
                                    td { "{phase}" }
                                    td { class: "num",
                                        match planned {
                                            Some(planned) => rsx! { "{done} of {planned}" },
                                            // No denominator, no ratio: the type
                                            // refuses to invent one and so does this.
                                            None => rsx! { "{done} so far" },
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            crate::panel::Panel {
                title: if working.raised == 0 { "no building has been raised".to_owned() }
                    else { format!("the {} building(s) this city holds", working.raised) },
                scope: "each with what its own plan says about it; a building with no readable plan says so rather than showing a zero"
                    .to_owned(),
                source: "one city query, asked when this page opened. Buildings appear when somebody raises one, so this is not re-asked on every event."
                    .to_owned(),
                match city.as_ref() {
                    None => rsx! {
                        crate::panel::Empty {
                            status: "asking the city what it holds".to_owned(),
                            what: "its buildings and their plans".to_owned(),
                        }
                    },
                    Some(answer) if answer.buildings.is_empty() => rsx! {
                        crate::panel::Empty {
                            status: "this city has no buildings yet".to_owned(),
                            what: "a building is one line of business, with its own rules, plan and archive. Raise one on the city page and work can be sent to it."
                                .to_owned(),
                            button {
                                onclick: move |_| on_view.call(View::City),
                                "go to the city"
                            }
                        }
                    },
                    Some(answer) => rsx! {
                        div { class: "index",
                            for building in answer.buildings.clone() {
                                div { key: "{building.addr.as_str()}", class: "index-row",
                                    button {
                                        class: "pick",
                                        onclick: {
                                            let name = building.addr.as_str().to_owned();
                                            move |_| on_open.call(name.clone())
                                        },
                                        "{building.addr.as_str()}"
                                    }
                                    span { class: "note",
                                        "{crate::progress::bar(&building.progress, false, crate::progress::Subject::Plan).label}"
                                    }
                                    span { class: "note",
                                        if working_here(&snapshot, &building.addr) {
                                            "working"
                                        } else {
                                            "idle"
                                        }
                                    }
                                }
                            }
                        }
                    },
                }
            }
        }
    }
}

/// Whether any run in flight is working inside this building.
fn working_here(snapshot: &Snapshot, building: &Address) -> bool {
    snapshot.runs().any(|(_, row)| {
        matches!(row.phase, RunPhase::Running | RunPhase::AwaitingApproval)
            && row.addr.as_ref().is_some_and(|addr| {
                addr.as_str()
                    .split('/')
                    .next()
                    .is_some_and(|first| first == building.as_str())
            })
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code"
)]
mod tests {
    use super::*;

    #[test]
    fn an_empty_city_says_so_rather_than_reporting_zeroes() {
        let working = Working::default();
        assert_eq!(headline(&working), "this city has no buildings yet");
        // "0 runs in 0 buildings" is the same fact and is not a sentence.
        assert!(!headline(&working).starts_with('0'));
    }

    #[test]
    fn a_city_with_buildings_and_no_work_says_which_of_the_two_it_is() {
        let working = Working {
            runs: 0,
            buildings: 0,
            raised: 5,
        };
        assert!(headline(&working).contains("nothing is running"));
        assert!(
            headline(&working).contains('5'),
            "a reader has to be able to tell an idle city from an empty one"
        );
    }

    #[test]
    fn the_headline_counts_places_rather_than_paths() {
        // Two runs in two rooms of one building are one building working:
        // the line is read as "where is work happening", not "how many
        // addresses are there".
        let working = Working {
            runs: 2,
            buildings: 1,
            raised: 3,
        };
        let said = headline(&working);
        assert!(said.contains("2 runs"), "{said}");
        assert!(said.contains("across 1 of the 3"), "{said}");
    }
}
