// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! One building, and what it has written down.
//!
//! A building's memory is files, not a database: `Roadmap.md` is the only
//! task list, `Memo.md` is where decisions and corrections go, `Handoff.md`
//! is what the next session needs, `BUILDING.md` states the rules, and the
//! archive holds what somebody decided was worth keeping. Agents write all
//! of them, and until this page existed a person could only read them by
//! leaving the interface and opening a text editor.
//!
//! **The page reads; it does not edit.** Editing here would be a second way
//! to change a building - one that leaves no run, no ledger line and no
//! checkpoint. What a person can do here is read, and then dispatch.

use channels::{Address, BuildingAnswer, ClientFrame, InboxAnswer, Query};
use dioxus::prelude::*;

/// Which of a building's faces is showing. The documents are named by the
/// files themselves, so the only variants this type spells are the ones
/// that are not a file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Leaf {
    /// One document, by file name.
    Doc(String),
    /// The archive index.
    Archive,
    /// One room, and what waits in it.
    Room(String),
}

/// The address of one room of this building.
///
/// The directory tree is the space (glossary, "Floor / Room"), so a room's
/// address is its building's plus the directory's own name. The city keeps
/// the authority on what an address may contain; this composes and lets
/// `Address::parse` refuse, rather than deciding for itself what a legal
/// room name is.
#[must_use]
pub fn room_addr(building: &Address, room: &str) -> Option<Address> {
    Address::parse(&format!("{}/{room}", building.as_str())).ok()
}

/// The first thing to show for a building: its plan, unless it has none.
#[must_use]
pub fn opening_leaf(answer: &BuildingAnswer) -> Leaf {
    answer
        .docs
        .first()
        .map_or(Leaf::Archive, |doc| Leaf::Doc(doc.name.clone()))
}

/// What this page knows about one room's queue.
///
/// `Unasked` and `Empty` are different answers and must stay different:
/// an answer that belongs to another room, or has not arrived, would
/// otherwise be rendered as "nothing waits here" - which is a claim this
/// page has no basis for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RoomQueue {
    Unasked,
    Empty,
    Waiting(Vec<channels::SignalLine>),
}

/// Reads the held answer as this room's queue.
///
/// Looking is not taking: the city folds the queue from the Ledger, so a
/// view of it consumes nothing. That is why this page can show a mailbox
/// at all - `Inbox::pull` would empty what it reported.
#[must_use]
pub fn waiting_in(inbox: Option<&InboxAnswer>, building: &Address, room: &str) -> RoomQueue {
    let Some(held) = inbox else {
        return RoomQueue::Unasked;
    };
    if room_addr(building, room).is_none_or(|at| held.addr != at) {
        return RoomQueue::Unasked;
    }
    if held.waiting.is_empty() {
        RoomQueue::Empty
    } else {
        RoomQueue::Waiting(held.waiting.clone())
    }
}

/// A day count rendered the way the archive files it: whole days, because
/// a stamp with more precision than the question invites comparisons
/// nobody meant to make.
#[must_use]
pub fn day_label(day: u64) -> String {
    format!("day {day}")
}

/// The building page.
#[component]
pub fn BuildingView(
    addr: Address,
    answer: Option<BuildingAnswer>,
    /// What waits in the room this page last asked about.
    inbox: Option<InboxAnswer>,
    /// How many signal events the stream has carried. A change means the
    /// open room's queue may have moved, so the page asks again.
    signals: u64,
    /// Whether the socket is live; see `app::Root`.
    live: Signal<bool>,
    on_frame: EventHandler<ClientFrame>,
) -> Element {
    let asked = use_signal(|| None::<String>);
    let wanted = addr.as_str().to_owned();
    use_effect(use_reactive!(|(wanted, live)| {
        let mut asked = asked;
        if live()
            && asked().as_deref() != Some(wanted.as_str())
            && let Ok(addr) = Address::parse(&wanted)
        {
            asked.set(Some(wanted.clone()));
            on_frame.call(ClientFrame::Query(Query::BuildingView { addr }));
        }
    }));
    let mut leaf = use_signal(|| None::<Leaf>);

    // The room's mailbox is asked for when a room is opened, and again
    // when the stream says a signal moved. `signals` is a count of those
    // events, not their content: the queue itself is the city's answer,
    // and folding one here would be a second place that claims to know
    // what waits in a room.
    let showing_room = match leaf() {
        Some(Leaf::Room(ref name)) => Some(name.clone()),
        _ => None,
    };
    let of_building = addr.clone();
    use_effect(use_reactive!(|(
        showing_room,
        signals,
        live,
        of_building,
    )| {
        let _ = signals;
        if live()
            && let Some(ref room) = showing_room
            && let Some(room_at) = room_addr(&of_building, room)
        {
            on_frame.call(ClientFrame::Query(Query::InboxView { addr: room_at }));
        }
    }));
    let Some(answer) = answer else {
        return rsx! {
            section { class: "building",
                p { class: "empty", "asking {addr.as_str()} what it has written down" }
            }
        };
    };
    let showing = leaf().unwrap_or_else(|| opening_leaf(&answer));
    let docs = answer.docs.clone();
    rsx! {
        section { class: "building",
            header { class: "building-head",
                h2 { "{answer.addr.as_str()}" }
                crate::progress::ProgressBar {
                    bar: crate::progress::bar(
                        &answer.progress,
                        false,
                        crate::progress::Subject::Plan,
                    ),
                }
                if answer.rooms.is_empty() {
                    span { class: "rooms", "no rooms yet - work here has not been given one" }
                }
            }
            for problem in answer.problems.clone() {
                p { key: "{problem}", class: "problems", "this plan row could not be read - {problem}" }
            }
            div { class: "tabs",
                for doc in docs.clone() {
                    button {
                        key: "{doc.name}",
                        class: "tab",
                        "aria-current": if showing == Leaf::Doc(doc.name.clone()) { "true" } else { "false" },
                        onclick: {
                            let name = doc.name.clone();
                            move |_| leaf.set(Some(Leaf::Doc(name.clone())))
                        },
                        "{doc.name}"
                    }
                }
                button {
                    class: "tab",
                    "aria-current": if showing == Leaf::Archive { "true" } else { "false" },
                    onclick: move |_| leaf.set(Some(Leaf::Archive)),
                    "archive ({answer.archive.len()})"
                }
                // The rooms are listed here and nowhere else on this page:
                // a second list of them would be a second answer to "what
                // is in this building".
                for room in answer.rooms.clone() {
                    button {
                        key: "room-{room}",
                        class: "tab room",
                        "aria-current": if showing == Leaf::Room(room.clone()) { "true" } else { "false" },
                        onclick: {
                            let name = room.clone();
                            move |_| leaf.set(Some(Leaf::Room(name.clone())))
                        },
                        "{room}/"
                    }
                }
            }
            match showing {
                Leaf::Room(ref room) => rsx! {
                    div { class: "mailbox",
                        match waiting_in(inbox.as_ref(), &answer.addr, room) {
                            RoomQueue::Unasked => rsx! {
                                p { class: "empty", "asking what waits in {room}" }
                            },
                            RoomQueue::Empty => rsx! {
                                p { class: "empty", "nothing waits in {room}" }
                            },
                            RoomQueue::Waiting(lines) => rsx! {
                                p { class: "note",
                                    "{lines.len()} waiting. Looking is not taking: a signal leaves this queue when a run pulls it."
                                }
                                for line in lines {
                                    div { key: "{line.id}", class: "waiting",
                                        span { class: "kind", "{line.kind}" }
                                        span { class: "from", "from {line.from}" }
                                        span { class: "id", "{line.id}" }
                                    }
                                }
                            },
                        }
                    }
                },
                Leaf::Archive => rsx! {
                    div { class: "archive",
                        if answer.archive.is_empty() {
                            p { class: "empty",
                                "nothing filed yet. An agent files what it decided was worth keeping."
                            }
                        }
                        for line in answer.archive.clone() {
                            div { key: "{line.day}-{line.subject}", class: "filed",
                                span { class: "day", "{day_label(line.day)}" }
                                span { class: "kind", "{line.kind}" }
                                span { class: "subject", "{line.subject}" }
                            }
                        }
                    }
                },
                Leaf::Doc(ref name) => {
                    let held = docs.iter().find(|doc| &doc.name == name);
                    match held {
                        Some(doc) => rsx! {
                            article { class: "doc",
                                p { class: "doc-note",
                                    "{doc.name} - {doc.bytes} bytes"
                                    if doc.truncated {
                                        " - shown up to the page's limit; the file on disk is longer"
                                    }
                                }
                                pre { class: "doc-text", "{doc.text}" }
                            }
                        },
                        None => rsx! {
                            p { class: "empty", "{name} is not in this building" }
                        },
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
    use super::*;
    use channels::{ArchiveLine, BuildingDoc, PlannedProgress, Progress};

    fn answer(docs: Vec<&str>) -> BuildingAnswer {
        BuildingAnswer {
            addr: Address::parse("lab").unwrap(),
            progress: Progress::Planned(PlannedProgress {
                done: 3,
                blocked: 0,
                total: 7,
            }),
            problems: Vec::new(),
            rooms: vec!["room1".to_owned()],
            docs: docs
                .into_iter()
                .map(|name| BuildingDoc {
                    name: name.to_owned(),
                    text: "body".to_owned(),
                    bytes: 4,
                    truncated: false,
                })
                .collect(),
            archive: vec![ArchiveLine {
                kind: "decision".to_owned(),
                day: 20_000,
                subject: "we build without dx".to_owned(),
            }],
        }
    }

    #[test]
    fn a_room_address_is_the_building_plus_the_directory_name() {
        let lab = Address::parse("lab").unwrap();
        assert_eq!(
            room_addr(&lab, "room1").map(|at| at.as_str().to_owned()),
            Some("lab/room1".to_owned())
        );
        // The city keeps the authority on what an address may hold; this
        // composes and lets the parser refuse.
        assert!(room_addr(&lab, "..").is_none());
        assert!(room_addr(&lab, "").is_none());
    }

    #[test]
    fn a_building_opens_on_its_plan() {
        let held = answer(vec!["Roadmap.md", "Memo.md"]);
        assert_eq!(opening_leaf(&held), Leaf::Doc("Roadmap.md".to_owned()));
    }

    #[test]
    fn a_building_with_no_documents_opens_on_what_it_has_filed() {
        assert_eq!(opening_leaf(&answer(Vec::new())), Leaf::Archive);
    }

    fn queue_of(room: &str, ids: &[&str]) -> InboxAnswer {
        InboxAnswer {
            addr: Address::parse(&format!("lab/{room}")).unwrap(),
            waiting: ids
                .iter()
                .map(|id| channels::SignalLine {
                    id: (*id).to_owned(),
                    kind: "question".to_owned(),
                    from: "lab/room2".to_owned(),
                    at: channels::TimeMs::new(10),
                })
                .collect(),
        }
    }

    #[test]
    fn another_rooms_answer_is_not_this_rooms_silence() {
        let lab = Address::parse("lab").unwrap();
        // The defect this refuses: showing "nothing waits here" on the
        // strength of an answer about a different room.
        assert_eq!(
            waiting_in(Some(&queue_of("room2", &[])), &lab, "room1"),
            RoomQueue::Unasked
        );
        assert_eq!(waiting_in(None, &lab, "room1"), RoomQueue::Unasked);
        assert_eq!(
            waiting_in(Some(&queue_of("room1", &[])), &lab, "room1"),
            RoomQueue::Empty
        );
        let waiting = waiting_in(Some(&queue_of("room1", &["sig-1", "sig-2"])), &lab, "room1");
        assert_eq!(
            waiting,
            RoomQueue::Waiting(queue_of("room1", &["sig-1", "sig-2"]).waiting)
        );
    }

    #[test]
    fn progress_is_the_plans_own_numbers_or_an_admission() {
        // The words come from `web::progress`, which is the one place a
        // progress reading is written. This page used
        // to phrase its own, which is how the city page and this one came
        // to say the same thing two ways.
        let held = answer(vec!["Roadmap.md"]);
        let shown = crate::progress::bar(&held.progress, false, crate::progress::Subject::Plan);
        assert_eq!(shown.label, "3/7");

        let mut unplanned = held;
        unplanned.progress = channels::Progress::Unplanned(channels::UnplannedProgress {
            steps: 0,
            budget: channels::BudgetUse::default(),
        });
        let said = crate::progress::bar(&unplanned.progress, false, crate::progress::Subject::Plan);
        assert_eq!(said.label, "no plan");
        assert!(said.filled.is_none(), "a percentage nobody can compute");
    }
}
