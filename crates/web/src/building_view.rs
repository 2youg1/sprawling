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
//! **The page reads; it does not edit.** Editing a document here would be
//! a second way to change a building - one that leaves no run, no ledger
//! line and no checkpoint. What a person can do with a document here is
//! read it, and then dispatch.
//!
//! One tab is not a document. What a building's runs may reach lives in
//! its `CONFIG.toml`, inside the reserved subtree that no write domain
//! touches, so no run can set it and a person's form is not a second way
//! to do anything. That form is `web::reach`'s, mounted here rather than
//! written here, which keeps this file a reader.

use crate::lang::{Msg, fill, say};
use channels::{Address, BuildingAnswer, ClientFrame, InboxAnswer, Query};
use dioxus::html::HasFileData;
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
    /// What this building's runs may reach: the only face on this page
    /// a person writes through, and the only one no agent can.
    Reach,
}

/// What a browser will say about a drop, without reading a byte.
///
/// The names and the text are all this build takes: a file dropped on a
/// city formed around somebody's own folder is already inside that city,
/// and staging a copy would be a second authority for one file.
fn dropped_from(event: &Event<DragData>) -> crate::drop::Dropped {
    let names: Vec<String> = event
        .files()
        .iter()
        .map(dioxus::html::FileData::name)
        .collect();
    if names.is_empty() {
        return crate::drop::Dropped::Unreadable;
    }
    crate::drop::Dropped::Files(names)
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
    /// Points the control surface at this building, so the one form
    /// that starts work is the one form that starts work.
    on_select: EventHandler<Option<String>>,
    /// Where a gesture goes. The page reads no drag itself: what one
    /// means is `web::drop`'s answer, and where it goes is the root's.
    on_drop: EventHandler<(crate::drop::Target, crate::drop::Dropped)>,
) -> Element {
    let lang = use_context::<Signal<crate::lang::Lang>>();
    let word = move |msg: Msg| say(lang(), msg);
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
                crate::panel::Empty {
                    status: fill(word(Msg::BuildingAsking), &[("addr", addr.as_str())]),
                    what: word(Msg::BuildingAskingWhat).to_owned(),
                }
            }
        };
    };
    let showing = leaf().unwrap_or_else(|| opening_leaf(&answer));
    let archive_tab = fill(
        word(Msg::BuildingArchiveTab),
        &[("count", &answer.archive.len().to_string())],
    );
    let docs = answer.docs.clone();
    let rooms = answer.rooms.len();
    rsx! {
        section { class: "building",
            crate::panel::Panel {
                title: fill(word(Msg::BuildingTitle), &[("addr", answer.addr.as_str())]),
                figure: (rooms > 0).then(|| rooms.to_string()),
                scope: word(Msg::BuildingScope).to_owned(),
                source: word(Msg::BuildingSource).to_owned(),
            header { class: "building-head",
                // A drop zone rather than a decoration: work aimed by
                // dragging lands on a place, and this header is the
                // building. What the drop means is decided in one
                // function; this only says where it landed.
                h2 {
                    class: "drop-zone",
                    title: "{word(Msg::DropHere)}",
                    ondragover: move |event| event.prevent_default(),
                    ondrop: {
                        let here = answer.addr.clone();
                        move |event: Event<DragData>| {
                            event.prevent_default();
                            on_drop.call((
                                crate::drop::Target::Place(here.clone()),
                                dropped_from(&event),
                            ));
                        }
                    },
                    "{answer.addr.as_str()}"
                }
                // Not a fourth dispatch form: this fills the bar at the
                // bottom of the window, which is where work is started
                // from every page and where a person looks for it next
                // time.
                button {
                    class: "start-here",
                    onclick: {
                        let here = answer.addr.as_str().to_owned();
                        move |_| on_select.call(Some(here.clone()))
                    },
                    "{word(Msg::BuildingStartHere)}"
                }
                crate::progress::ProgressBar {
                    bar: crate::progress::bar(
                        &answer.progress,
                        false,
                        crate::progress::Subject::Plan,
                        lang(),
                    ),
                }
                if answer.rooms.is_empty() {
                    span { class: "rooms", "{word(Msg::BuildingNoRooms)}" }
                }
            }
            for problem in answer.problems.clone() {
                p { key: "{problem}", class: "problems",
                    "{fill(word(Msg::BuildingUnreadableRow), &[(\"problem\", &problem)])}"
                }
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
                    "{archive_tab}"
                }
                button {
                    class: "tab",
                    "aria-current": if showing == Leaf::Reach { "true" } else { "false" },
                    onclick: move |_| leaf.set(Some(Leaf::Reach)),
                    "{word(Msg::BuildingReachTab)}"
                }
                // The rooms are listed here and nowhere else on this page:
                // a second list of them would be a second answer to "what
                // is in this building".
                for room in answer.rooms.clone() {
                    button {
                        key: "room-{room}",
                        class: "tab room drop-zone",
                        title: "{word(Msg::DropHere)}",
                        "aria-current": if showing == Leaf::Room(room.clone()) { "true" } else { "false" },
                        onclick: {
                            let name = room.clone();
                            move |_| leaf.set(Some(Leaf::Room(name.clone())))
                        },
                        ondragover: move |event| event.prevent_default(),
                        ondrop: {
                            let landed = room_addr(&answer.addr, &room);
                            move |event: Event<DragData>| {
                                event.prevent_default();
                                // A room name this city cannot address
                                // is not a place; the same function that
                                // refuses a run refuses it, with its own
                                // words rather than a second wording
                                // here.
                                let target = match landed.clone() {
                                    Some(addr) => crate::drop::Target::Place(addr),
                                    None => crate::drop::Target::Run,
                                };
                                on_drop.call((target, dropped_from(&event)));
                            }
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
                                crate::panel::Empty {
                                    status: fill(word(Msg::BuildingAskingRoom), &[("room", room.as_str())]),
                                    what: "until that answer arrives this page cannot say whether the room is empty, and will not guess"
                                        .to_owned(),
                                }
                            },
                            RoomQueue::Empty => rsx! {
                                crate::panel::Empty {
                                    status: fill(word(Msg::BuildingRoomEmpty), &[("room", room.as_str())]),
                                    what: "another resident can leave a signal here, and a run in this room pulls it at its next safe point. Looking is not taking."
                                        .to_owned(),
                                }
                            },
                            RoomQueue::Waiting(lines) => rsx! {
                                p { class: "note",
                                    "{lines.len()} waiting. Looking is not taking: a signal leaves this queue when a run pulls it."
                                }
                                for line in lines {
                                    div { key: "{line.id}", class: "waiting",
                                        span { class: "kind", "{line.kind}" }
                                        span { class: "from", "{fill(word(Msg::BuildingSignalFrom), &[(\"who\", &line.from)])}" }
                                        span { class: "id", "{line.id}" }
                                    }
                                }
                            },
                        }
                    }
                },
                Leaf::Reach => rsx! {
                    crate::reach::ReachForm {
                        addr: answer.addr.clone(),
                        sandbox: answer.sandbox.clone(),
                        servers: answer.mcp.clone(),
                        on_frame,
                    }
                },
                Leaf::Archive => rsx! {
                    div { class: "archive",
                        if answer.archive.is_empty() {
                            crate::panel::Empty {
                                status: word(Msg::BuildingNothingFiled).to_owned(),
                                what: "a resident files what it settled and does not want to work out twice. What is here is what the next run in this building is told before it starts."
                                    .to_owned(),
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
                                        "{word(Msg::BuildingTruncated)}"
                                    }
                                }
                                pre { class: "doc-text", "{doc.text}" }
                            }
                        },
                        None => rsx! {
                            crate::panel::Empty {
                                status: format!("{name} is not in this building"),
                                what: word(Msg::BuildingNoDocument)
                                    .to_owned(),
                            }
                        },
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
    use super::*;
    use channels::{ArchiveLine, BuildingDoc, PlannedProgress, Progress};

    fn answer(docs: Vec<&str>) -> BuildingAnswer {
        BuildingAnswer {
            sandbox: None,
            mcp: Vec::new(),
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
        let shown = crate::progress::bar(
            &held.progress,
            false,
            crate::progress::Subject::Plan,
            crate::lang::Lang::En,
        );
        assert_eq!(shown.label, "3/7");

        let mut unplanned = held;
        unplanned.progress = channels::Progress::Unplanned(channels::UnplannedProgress {
            steps: 0,
            budget: channels::BudgetUse::default(),
        });
        let said = crate::progress::bar(
            &unplanned.progress,
            false,
            crate::progress::Subject::Plan,
            crate::lang::Lang::En,
        );
        assert_eq!(said.label, "no plan");
        assert!(said.filled.is_none(), "a percentage nobody can compute");
    }
}
