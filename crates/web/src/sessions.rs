// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The first screen: the box that starts work, and the table of what it
//! started (web-SPEC.md sections 8-53 B3 and 8-56).
//!
//! **Starting work is the empty state of this table.** It used to be a
//! bar pinned to the bottom of every page, asking four questions before
//! anything could be sent. It stands here instead because this is the
//! table its rows land in, which is the one place a person can see what
//! the action does.
//!
//! **At rest it is one box.** Everything else the city needs is
//! inferred and written out as one sentence under the box, one word per
//! decision, and every word is a control that opens in place. Nothing is
//! hidden and nothing is asked: a person who agrees with the sentence
//! presses send, and a person who does not clicks the word they disagree
//! with.
//!
//! **A guess and a decision may not look alike.** A word the city
//! inferred is underlined dotted in a quiet grey; a word the person set
//! is underlined solid in the accent. An interface that draws its own
//! guesses like decisions is answering on the reader's behalf.
//!
//! The markup is `screens/sessions.html` after `dx translate`, with text
//! nodes replaced and control flow added. Tags and class names are the
//! translator's, not this file's.

use channels::Address;
use dioxus::prelude::*;

use crate::app::{Snapshot, View};
use crate::lang::{Lang, Msg, around, fill, say};
use crate::phase::Phase;

/// How many finished sessions the second table holds.
///
/// Eight rather than all of them: what ended is context for what is
/// happening, and a list that grows without bound turns the page a
/// person opens to act into a page they have to scroll.
pub const ENDED_ROWS: usize = 8;

/// The mode a piece of work runs in when nobody has said otherwise.
///
/// `runtime::Mode` is the authority for the set; this is the authority
/// for which one a person who says nothing gets. Build, because it is
/// the mode that produces something, and the one a person who has not
/// yet learned the others meant.
pub const DEFAULT_MODE: &str = "build";

/// One decision in the sentence under the box.
///
/// Three, and they are the three a Run cannot start without. Money is
/// not among them: this city has no budget lock, the receiver of a cost
/// figure is an agent rather than a brake, and a person typing here has
/// no way to know the number.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Field {
    /// Which room the work goes to.
    Room,
    /// Which mode it runs in.
    Mode,
    /// How hard the model is asked to think.
    Effort,
}

impl Field {
    /// Every field, in the order the sentence reads them.
    pub const ALL: [Field; 3] = [Field::Room, Field::Mode, Field::Effort];

    /// The phrase this field's word sits inside.
    #[must_use]
    pub fn sentence(self) -> Msg {
        match self {
            Self::Room => Msg::ComposerSendTo,
            Self::Mode => Msg::ComposerAs,
            Self::Effort => Msg::ComposerThink,
        }
    }

    /// The slot inside that phrase which this field fills.
    #[must_use]
    pub fn slot(self) -> &'static str {
        match self {
            Self::Room => "room",
            Self::Mode => "mode",
            Self::Effort => "effort",
        }
    }

    /// What the field's own control is labelled when it opens.
    #[must_use]
    pub fn label(self) -> Msg {
        match self {
            Self::Room => Msg::ComposerRoomFor,
            Self::Mode => Msg::ComposerModeFor,
            Self::Effort => Msg::ComposerEffortFor,
        }
    }
}

/// What the composer will send, and which parts of it a person chose.
///
/// The distinction is the point: `chosen` is what makes a decision draw
/// differently from a guess, and it is a fact about how the value got
/// here rather than about the value.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Plan {
    pub room: String,
    pub mode: String,
    pub effort: String,
    /// The fields a person set for themselves.
    chosen: Vec<Field>,
}

impl Plan {
    /// The plan this city would guess, from what it has already seen.
    ///
    /// Rules, and named as rules: the room a person last sent work to,
    /// the mode that produces something, and the depth this city was
    /// configured with. When a model does the inferring instead
    /// (`bin::assembly`), it answers the same three and the city refuses
    /// rather than guessing when it cannot.
    ///
    /// A city with no history guesses nothing for the room and says so
    /// by leaving it empty, which opens that one control. Inventing a
    /// building name would be the interface answering for the person.
    #[must_use]
    pub fn guessed(snapshot: &Snapshot, effort: &str) -> Self {
        let room = latest_room(snapshot).unwrap_or_default();
        Self {
            room,
            mode: DEFAULT_MODE.to_owned(),
            effort: effort.to_owned(),
            chosen: Vec::new(),
        }
    }

    /// The value in one field.
    #[must_use]
    pub fn value(&self, field: Field) -> &str {
        match field {
            Field::Room => &self.room,
            Field::Mode => &self.mode,
            Field::Effort => &self.effort,
        }
    }

    /// Records a person's own choice, which is what stops the word being
    /// drawn as a guess. Setting a field to what it already guessed is
    /// still a decision: they read it and agreed.
    pub fn choose(&mut self, field: Field, value: String) {
        match field {
            Field::Room => self.room = value,
            Field::Mode => self.mode = value,
            Field::Effort => self.effort = value,
        }
        if !self.chosen.contains(&field) {
            self.chosen.push(field);
        }
    }

    /// Whether this word came from the person rather than from the city.
    #[must_use]
    pub fn is_chosen(&self, field: Field) -> bool {
        self.chosen.contains(&field)
    }

    /// The class the word is drawn with. One producer, so the two
    /// underlines cannot come apart from the two meanings.
    #[must_use]
    pub fn ink(&self, field: Field) -> &'static str {
        if self.is_chosen(field) {
            "chosen"
        } else {
            "guess"
        }
    }
}

/// Which rung of the readiness ladder this city is on.
///
/// **Not wizard steps.** Each one is a fact that is true right now, so a
/// person who attaches a provider in another window finds the first rung
/// gone when they come back. A wizard would have stored its own progress
/// — a second authority on how far along this city is — and the stored
/// copy is what goes stale.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Rung {
    /// No model answers for the tag a run thinks with.
    NoModel,
    /// There is a model and nowhere to put work.
    NoBuilding,
    /// It can send work out and never has.
    NeverSent,
    /// It has sent work out. The ladder is behind this city.
    Working,
}

/// Where this city stands, from what it holds right now.
///
/// Ordered by what blocks what: a city with no model cannot be helped by
/// being told it has no buildings, and a person given two problems at
/// once solves neither. One rung, one action.
#[must_use]
pub fn rung(
    endpoints: Option<&channels::EndpointsAnswer>,
    city: Option<&channels::CityAnswer>,
    snapshot: &Snapshot,
) -> Rung {
    // `None` is "not asked yet", which is not the same as "none
    // attached". A page that showed the first rung while the answer was
    // in flight would tell a working city it cannot work.
    let answered = match (endpoints, city) {
        (Some(endpoints), Some(city)) => (endpoints, city),
        _ => return Rung::Working,
    };
    let (endpoints, city) = answered;
    if !crate::settings::can_dispatch(endpoints) {
        return Rung::NoModel;
    }
    if city.buildings.is_empty() {
        return Rung::NoBuilding;
    }
    if snapshot.runs().next().is_none() {
        return Rung::NeverSent;
    }
    Rung::Working
}

/// The room work was most recently sent to.
///
/// The strongest signal this city has about where the next piece goes,
/// and it costs nothing: a person working on one thing sends several
/// pieces of work to the same place.
#[must_use]
pub fn latest_room(snapshot: &Snapshot) -> Option<String> {
    snapshot
        .runs()
        .filter_map(|(_, row)| row.addr.as_ref().map(|addr| (row.started_at_seq, addr)))
        .max_by_key(|(seq, _)| *seq)
        .map(|(_, addr)| addr.as_str().to_owned())
}

/// One row of the table.
///
/// Flattened out of the snapshot rather than passed as a `RunRow`, so
/// the row holds exactly what it draws and a change to what a row shows
/// is visible here rather than spread through the markup.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SeatRow {
    pub addr: Address,
    pub phase: Phase,
    /// The last thing the model said, or nothing when it has not spoken.
    pub said: Option<String>,
    pub turns: u32,
    /// What it has cost, where any call settled an amount.
    pub spent: Option<channels::UsdMicros>,
}

/// Every session this client knows of, in two lists: what is still
/// moving, and what has ended.
///
/// Sorted by phase first, so what waits on a person is at the top of the
/// first list. Inside a phase, newest first: a person scanning for the
/// thing they just sent finds it where they are already looking.
#[must_use]
pub fn listing(snapshot: &Snapshot) -> (Vec<SeatRow>, Vec<SeatRow>) {
    let mut in_flight = Vec::new();
    let mut ended = Vec::new();
    for (_, row) in snapshot.runs() {
        let Some(addr) = row.addr.clone() else {
            // A run with no address is a run this client saw start
            // somewhere it cannot name, and a row that cannot be opened
            // is a row that wastes the reader's click.
            continue;
        };
        let seat = SeatRow {
            addr,
            phase: row.phase,
            said: row.said.clone(),
            turns: row.turns,
            spent: row.spent,
        };
        if row.phase.in_flight() {
            in_flight.push((row.started_at_seq, seat));
        } else {
            ended.push((row.started_at_seq, seat));
        }
    }
    in_flight.sort_by_key(|(seq, row)| {
        (
            crate::phase::READING_ORDER
                .iter()
                .position(|held| *held == row.phase)
                .unwrap_or(usize::MAX),
            std::cmp::Reverse(*seq),
        )
    });
    ended.sort_by_key(|(seq, _)| std::cmp::Reverse(*seq));
    ended.truncate(ENDED_ROWS);
    (
        in_flight.into_iter().map(|(_, row)| row).collect(),
        ended.into_iter().map(|(_, row)| row).collect(),
    )
}

/// What a row says it has cost. Absent is not zero: a subscription
/// settles no amount, and printing `$0.00` would be this page inventing
/// the one fact nobody sent it.
#[must_use]
pub fn spent_of(lang: Lang, spent: Option<channels::UsdMicros>) -> String {
    match spent {
        Some(amount) => crate::app::render_usd(amount),
        None => say(lang, Msg::SessionsUnpriced).to_owned(),
    }
}

/// The three counts the top bar carries, said.
#[must_use]
pub fn counts_said(lang: Lang, snapshot: &Snapshot) -> [String; 3] {
    let (running, waiting, buildings) = snapshot.counts();
    [
        (Msg::CountRunning, running),
        (Msg::CountWaiting, waiting),
        (Msg::CountBuildings, buildings),
    ]
    .map(|(msg, count)| fill(say(lang, msg), &[("n", &count.to_string())]))
}

/// The first screen.
#[component]
#[allow(
    clippy::too_many_arguments,
    reason = "one page, one prop per fact it draws"
)]
pub fn SessionsView(
    snapshot: Snapshot,
    /// What the city answered about itself, for the buildings a room can
    /// be completed against.
    city: Option<channels::CityAnswer>,
    /// What this machine has attached, for the first rung of the ladder.
    endpoints: Option<channels::EndpointsAnswer>,
    /// The depth this city thinks with when nobody says otherwise.
    effort: String,
    /// A task line a drop wrote. It fills the box a person would have
    /// typed into, so one gesture both aims and describes.
    dropped: Option<String>,
    live: Signal<bool>,
    on_frame: EventHandler<channels::ClientFrame>,
    on_view: EventHandler<View>,
    on_drop: EventHandler<(crate::drop::Target, crate::drop::Dropped)>,
) -> Element {
    let lang = use_context::<Signal<Lang>>();
    let word = move |msg: Msg| say(lang(), msg);
    let mut task = use_signal(String::new);
    let mut plan = use_signal(|| Plan::guessed(&snapshot, &effort));
    let mut opened: Signal<Option<Field>> = use_signal(|| None);

    // A drop writes the task and nothing else. It arrives as a prop
    // rather than as a signal because the gesture is answered once, in
    // `web::drop`, and a second reading of it here would be a second
    // answer to what a drag means.
    use_effect(move || {
        if let Some(written) = dropped.clone() {
            task.set(written);
        }
    });

    let (in_flight, ended) = listing(&snapshot);
    let standing = rung(endpoints.as_ref(), city.as_ref(), &snapshot);
    let rooms: Vec<String> = city
        .as_ref()
        .map(|answer| {
            answer
                .buildings
                .iter()
                .map(|raised| raised.addr.as_str().to_owned())
                .collect()
        })
        .unwrap_or_default();
    // The frame this box would send, built once from the two signals.
    // `dispatch_command` is the only place in the client that builds a
    // Dispatch, so asking it whether one can be built is the same
    // question as asking whether the button may be pressed - and the
    // button cannot go out of step with what pressing it would do.
    let intended = {
        let held = plan.read().clone();
        crate::app::dispatch_command(
            &held.room,
            &task.read(),
            "",
            &held.mode,
            crate::app::effort_named(&held.effort),
        )
    };
    let sendable = intended.is_some();
    let mut send = move || {
        let held = plan.read().clone();
        let written = task.read().clone();
        if let Some(frame) = crate::app::dispatch_command(
            &held.room,
            &written,
            "",
            &held.mode,
            crate::app::effort_named(&held.effort),
        ) {
            on_frame.call(frame);
            task.set(String::new());
            opened.set(None);
        }
    };

    rsx! {
        // The ladder. Nothing here stores progress, so attaching a
        // provider outside this window makes the first rung disappear by
        // itself. Each rung offers one action, because a person handed
        // two problems at once solves neither.
        if standing == Rung::NoModel {
            crate::panel::Panel {
                title: word(Msg::FirstNoModelTitle).to_owned(),
                scope: Some(word(Msg::FirstNoModelScope).to_owned()),
                figure: None,
                source: word(Msg::FirstNoModelSource).to_owned(),
                crate::panel::Empty {
                    status: fill(word(Msg::FirstNoModelStatus), &[("tag", "main")]),
                    what: word(Msg::FirstNoModelWhat).to_owned(),
                    a { class: "nav-item", href: "#/setup", "{word(Msg::FirstNoModelWay)}" }
                    a { class: "nav-item", href: "#/setup",
                        "{word(Msg::FirstNoModelSubscription)}"
                    }
                }
            }
        }
        if standing == Rung::NoBuilding {
            crate::panel::Panel {
                title: word(Msg::FirstNoBuildingTitle).to_owned(),
                scope: Some(word(Msg::FirstNoBuildingScope).to_owned()),
                figure: None,
                source: word(Msg::FirstNoBuildingSource).to_owned(),
                crate::panel::Empty {
                    status: word(Msg::FirstNoBuildingStatus).to_owned(),
                    what: fill(word(Msg::FirstNoBuildingWhat), &[("example", "lab")]),
                    a { class: "nav-item", href: "#/", "{word(Msg::FirstNoBuildingWay)}" }
                }
            }
        }
        form {
            class: "panel composer",
            onsubmit: move |event| {
                event.prevent_default();
                send();
            },
            div { class: "panel-head",
                h2 { class: "panel-title",
                    if standing == Rung::NeverSent {
                        "{word(Msg::FirstDispatchTitle)}"
                    } else {
                        "{word(Msg::ComposerTitle)}"
                    }
                }
            }
            p { class: "panel-scope",
                if standing == Rung::NeverSent {
                    "{word(Msg::FirstDispatchScope)}"
                } else {
                    "{word(Msg::ComposerScope)}"
                }
            }
            div { class: "panel-body",
                textarea {
                    class: "composer-task",
                    placeholder: "{word(Msg::ComposerExample)}",
                    rows: "2",
                    value: "{task}",
                    ondragover: move |event| event.prevent_default(),
                    ondrop: move |event| {
                        event.prevent_default();
                        on_drop
                            .call((
                                crate::drop::Target::Composer,
                                crate::drop::from_event(&event),
                            ));
                    },
                    oninput: move |event| task.set(event.value()),
                    // Enter sends and Shift+Enter makes a line, which is
                    // the shape every message box a person has used takes.
                    // Said beside the button as well: a keystroke nobody
                    // is told about is a keystroke nobody presses.
                    onkeydown: move |event| {
                        if event.key() == Key::Enter && !event.modifiers().shift() {
                            event.prevent_default();
                            send();
                        }
                    },
                }
                p { class: "composer-plan",
                    for field in Field::ALL {
                        Decision {
                            key: "{field:?}",
                            field,
                            plan: plan.read().clone(),
                            opened,
                            rooms: rooms.clone(),
                            on_choose: move |value: String| {
                                plan.write().choose(field, value);
                                opened.set(None);
                            },
                        }
                    }
                    button {
                        class: "send",
                        r#type: "submit",
                        disabled: !sendable,
                        "{word(Msg::DispatchSend)}"
                    }
                }
                p { class: "composer-key",
                    if standing == Rung::NeverSent {
                        "{word(Msg::FirstDispatchKeys)}"
                    } else {
                        "{word(Msg::ComposerKeys)}"
                    }
                }
            }
            p { class: "panel-source",
                if standing == Rung::NeverSent {
                    "{word(Msg::FirstDispatchSource)}"
                } else {
                    "{word(Msg::ComposerSource)}"
                }
            }
        }

        section { class: "panel",
            div { class: "panel-head",
                h2 { class: "panel-title", "{word(Msg::NavSessions)}" }
                span { class: "panel-figure", "{in_flight.len()}" }
            }
            p { class: "panel-scope", "{word(Msg::SessionsScope)}" }
            div { class: "panel-body",
                if in_flight.is_empty() {
                    div { class: "empty",
                        span { class: "empty-status", "{word(Msg::SessionsNothingYet)}" }
                        span { class: "empty-what", "{word(Msg::SessionsNothingWhat)}" }
                    }
                } else {
                    for row in in_flight {
                        SessionRow { key: "{row.addr.as_str()}", row }
                    }
                }
            }
            p { class: "panel-source",
                {
                    fill(
                        word(Msg::SessionsSource),
                        &[
                            (
                                "seq",
                                &snapshot
                                    .applied_through()
                                    .map(|seq| seq.value().to_string())
                                    .unwrap_or_else(|| word(Msg::AskingWhatItHolds).to_owned()),
                            ),
                        ],
                    )
                }
            }
        }

        if !ended.is_empty() {
            section { class: "panel",
                div { class: "panel-head",
                    h2 { class: "panel-title", "{word(Msg::SessionsEnded)}" }
                }
                p { class: "panel-scope", "{word(Msg::SessionsEndedScope)}" }
                div { class: "panel-body",
                    for row in ended {
                        SessionRow { key: "{row.addr.as_str()}", row }
                    }
                }
                p { class: "panel-source", "{word(Msg::SessionsEndedSource)}" }
            }
        }

        // Which buildings are busy, drawn rather than listed. One block
        // on this page rather than a destination of its own: it is a way
        // of picturing the table above it, not a separate question.
        section { class: "panel",
            div { class: "panel-head",
                h2 { class: "panel-title", "{word(Msg::NavCity)}" }
            }
            p { class: "panel-scope", "{word(Msg::SessionsCityScope)}" }
            div { class: "panel-body",
                crate::city_view::CityView {
                    city,
                    busy: crate::app::busy_buildings(&snapshot),
                    selected: None,
                    live,
                    on_frame,
                    on_select: move |_| {},
                    on_open: move |name: String| {
                        if let Some(addr) = crate::app::opened_building(Some(name.as_str())) {
                            on_view.call(View::Building(addr));
                        }
                    },
                }
            }
        }
    }
}

/// One word of the inferred sentence, and the control it opens into.
///
/// Opening replaces the word in place rather than revealing a panel: the
/// sentence keeps its shape, so a person who opens a word by accident
/// has not lost the page they were reading.
#[component]
fn Decision(
    field: Field,
    plan: Plan,
    opened: Signal<Option<Field>>,
    rooms: Vec<String>,
    on_choose: EventHandler<String>,
) -> Element {
    let lang = use_context::<Signal<Lang>>();
    let word = move |msg: Msg| say(lang(), msg);
    let (before, after) = around(word(field.sentence()), field.slot());
    let value = plan.value(field).to_owned();
    let ink = plan.ink(field);
    // A room this city has never seen is the one field a person must be
    // able to type into; the rest are closed sets, and a closed set is
    // offered rather than typed.
    let choices: Vec<(String, String)> = match field {
        Field::Room => Vec::new(),
        Field::Mode => crate::app::MODES
            .iter()
            .map(|tag| ((*tag).to_owned(), (*tag).to_owned()))
            .collect(),
        Field::Effort => crate::app::EFFORTS
            .iter()
            .skip(1)
            .map(|(tag, msg)| ((*tag).to_owned(), say(lang(), *msg).to_owned()))
            .collect(),
    };

    rsx! {
        if opened() == Some(field) {
            label { class: "composer-open",
                span { class: "label", "{word(field.label())}" }
                if field == Field::Room {
                    input {
                        class: "composer-field",
                        list: "composer-rooms",
                        value: "{value}",
                        autofocus: true,
                        onchange: move |event| on_choose.call(event.value()),
                    }
                    datalist { id: "composer-rooms",
                        for room in rooms.clone() {
                            option { key: "{room}", value: "{room}" }
                        }
                    }
                } else {
                    select {
                        class: "composer-field",
                        value: "{value}",
                        onchange: move |event| on_choose.call(event.value()),
                        for (tag , shown) in choices.clone() {
                            option { key: "{tag}", value: "{tag}", "{shown}" }
                        }
                    }
                }
            }
        } else {
            button {
                class: "{ink}",
                r#type: "button",
                onclick: move |_| opened.set(Some(field)),
                "{before}"
                b { "{value}" }
                "{after}"
            }
            if field != Field::Effort {
                span { class: "dot", "·" }
            }
        }
    }
}

/// One session, as a row that is itself the link to it.
///
/// An anchor and not a button: writing the fragment is the only way a
/// view changes, and an `<a href>` already does that. It arrives with
/// the keyboard, the middle click, "copy link address" and the link role
/// a screen reader announces — all of which a button with an `onclick`
/// would have had to be given back one at a time.
#[component]
fn SessionRow(row: SeatRow) -> Element {
    let lang = use_context::<Signal<Lang>>();
    let mark = row.phase;
    let said = row.said.clone().unwrap_or_default();
    rsx! {
        a {
            class: "session-row",
            href: "{crate::route::to_fragment(&View::Session(row.addr.clone()))}",
            span {
                class: "phase {mark.token()}",
                role: "img",
                "aria-label": "{say(lang(), mark.word())}",
            }
            span { class: "room", "{row.addr.as_str()}" }
            span { class: "said", "{said}" }
            span { class: "turn",
                {fill(say(lang(), Msg::SessionsTurnCount), &[("n", &row.turns.to_string())])}
            }
            span { class: "spent", "{spent_of(lang(), row.spent)}" }
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
    use super::{DEFAULT_MODE, ENDED_ROWS, Field, Plan, listing, spent_of};
    use crate::app::Snapshot;
    use crate::lang::Lang;
    use crate::phase::Phase;

    /// The one visual rule this page is built on: a word the city
    /// guessed and a word the person set may not be drawn alike.
    #[test]
    fn a_guess_and_a_decision_are_drawn_differently() {
        let mut plan = Plan {
            room: "lab/parser".to_owned(),
            mode: DEFAULT_MODE.to_owned(),
            effort: "medium".to_owned(),
            chosen: Vec::new(),
        };
        for field in Field::ALL {
            assert_eq!(plan.ink(field), "guess");
        }
        plan.choose(Field::Mode, "sc".to_owned());
        assert_eq!(plan.ink(Field::Mode), "chosen");
        assert_eq!(plan.ink(Field::Room), "guess");
        assert_eq!(plan.ink(Field::Effort), "guess");
    }

    /// Agreeing with a guess is a decision. Without this, a person who
    /// opened the word, read it and kept it would still be shown the
    /// city's own dotted guess.
    #[test]
    fn keeping_the_guessed_value_still_counts_as_choosing_it() {
        let mut plan = Plan {
            mode: DEFAULT_MODE.to_owned(),
            ..Plan::default()
        };
        plan.choose(Field::Mode, DEFAULT_MODE.to_owned());
        assert_eq!(plan.mode, DEFAULT_MODE);
        assert_eq!(plan.ink(Field::Mode), "chosen");
    }

    /// A city with nothing in it guesses no room, which opens that one
    /// control instead of inventing a building name.
    #[test]
    fn an_empty_city_guesses_no_room() {
        let plan = Plan::guessed(&Snapshot::new(), "medium");
        assert!(plan.room.is_empty());
        assert_eq!(plan.mode, DEFAULT_MODE);
        assert_eq!(plan.effort, "medium");
    }

    /// Absent money is not zero money. A subscription settles no amount,
    /// and `$0.00` would be a figure nobody sent.
    #[test]
    fn an_unpriced_session_does_not_read_as_a_free_one() {
        assert_eq!(spent_of(Lang::En, None), "not priced");
        assert_ne!(spent_of(Lang::En, None), "$0.00");
        assert_eq!(
            spent_of(Lang::En, Some(channels::UsdMicros::new(420_000))),
            "$0.42"
        );
    }

    /// What waits on a person is at the top of the first list, which is
    /// the reason the page was opened.
    #[test]
    fn what_waits_on_a_person_leads_the_list() {
        let snapshot = crate::app::seated(&[
            (Some("lab/one"), Phase::Running, 1),
            (Some("lab/two"), Phase::Waiting, 3),
            (Some("lab/three"), Phase::Running, 5),
            (Some("lab/gone"), Phase::Frozen, 7),
        ]);
        let (in_flight, ended) = listing(&snapshot);
        assert_eq!(in_flight.len(), 3);
        assert_eq!(in_flight[0].addr.as_str(), "lab/two");
        assert_eq!(in_flight[0].phase, Phase::Waiting);
        // Newest first inside a phase.
        assert_eq!(in_flight[1].addr.as_str(), "lab/three");
        assert_eq!(ended.len(), 1);
        assert_eq!(ended[0].addr.as_str(), "lab/gone");
    }

    /// A page opened to act on does not become a page to scroll.
    #[test]
    fn the_finished_list_stops_at_eight() {
        let rooms: Vec<String> = (0..20).map(|index| format!("lab/room{index}")).collect();
        let seats: Vec<(Option<&str>, Phase, u64)> = rooms
            .iter()
            .enumerate()
            .map(|(index, name)| {
                (
                    Some(name.as_str()),
                    Phase::Frozen,
                    u64::try_from(index)
                        .unwrap_or_default()
                        .saturating_mul(2)
                        .saturating_add(1),
                )
            })
            .collect();
        let snapshot = crate::app::seated(&seats);
        let (in_flight, ended) = listing(&snapshot);
        assert!(in_flight.is_empty());
        assert_eq!(ended.len(), ENDED_ROWS);
        // The newest eight, not the first eight.
        assert_eq!(ended[0].addr.as_str(), "lab/room19");
    }

    /// A run this client cannot name has no row, because a row that
    /// cannot be opened spends a reader's click on nothing.
    #[test]
    fn a_run_with_no_address_gets_no_row() {
        let snapshot = crate::app::seated(&[
            (Some("lab/named"), Phase::Running, 1),
            (None, Phase::Running, 3),
        ]);
        assert_eq!(snapshot.runs().count(), 2, "both runs are folded");
        let (in_flight, _) = listing(&snapshot);
        assert_eq!(in_flight.len(), 1);
        assert_eq!(in_flight[0].addr.as_str(), "lab/named");
    }
}
