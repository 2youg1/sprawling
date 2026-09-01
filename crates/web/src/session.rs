// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! One session: a work line in one room (web-SPEC.md section 8-53 B4).
//!
//! **The object page this product never had.** Every other page is a
//! list of sessions, a history of sessions, or a setting; the wire's own
//! shape says so — five queries answer "about one piece of work" and all
//! five verbs act on one. What was missing was a page for the thing
//! itself, and the address of one: `#/live/<uuid>` named a session by a
//! number nobody had chosen, so "open yesterday's session" stayed
//! unanswerable even after the query behind it was built.
//!
//! **The head answers the four questions a person actually arrives
//! with**: what has it cost, what is holding it, how much room is left
//! to think in, and whether the handoff still describes where it is.
//! Three of them are answerable today. The fourth is not on the wire,
//! and it is drawn as an em rule with the reason beside it rather than
//! as a number this page made up.
//!
//! Reopening the page restates where you are, because moving between
//! pages costs a reader the context they were holding.

use channels::Address;
use dioxus::prelude::*;

use crate::app::{Snapshot, View};
use crate::lang::{Lang, Msg, fill, say};

/// One of the four things the head says, and the four are fixed.
///
/// A page that reports whatever it happens to know reports a different
/// set every time it is opened. These four were chosen because they are
/// the questions a person asks before deciding what to do next, and the
/// set does not grow when a new field appears on the wire.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    /// What the fact says.
    pub said: String,
    /// Whether the city could answer it at all. A fact it cannot answer
    /// is drawn quiet and explained in the scope line, never omitted:
    /// a missing row reads as "nothing to report", which is the one
    /// thing that is not true.
    pub known: bool,
}

/// What this client can say about a session, in the fixed order the head
/// reads them.
///
/// Pure, so the assertion that the third one is never invented can be
/// written against a value rather than against markup.
#[must_use]
pub fn head_facts(lang: Lang, row: &crate::app::RunRow) -> [Fact; 4] {
    let spent = match row.spent {
        Some(amount) => Fact {
            said: fill(
                say(lang, Msg::SessionSpentIs),
                &[("amount", &crate::app::render_usd(amount))],
            ),
            known: true,
        },
        None => Fact {
            said: fill(
                say(lang, Msg::SessionSpentIs),
                &[("amount", say(lang, Msg::SessionsUnpriced))],
            ),
            known: false,
        },
    };
    let gate = match row.gate.as_deref() {
        Some(named) => Fact {
            said: fill(say(lang, Msg::SessionAtGate), &[("gate", named)]),
            known: true,
        },
        None => Fact {
            said: say(lang, Msg::SessionNoGate).to_owned(),
            known: true,
        },
    };
    // The one the wire cannot carry. `runtime::tools::status` measures
    // it and thirteen fields beside it, and not one of them is on the
    // wire — so this city knows the number and no query returns it.
    // Drawn as a rule with the reason in the scope line: a plausible
    // figure here would be the interface inventing the single fact a
    // person is most likely to act on.
    let context = Fact {
        said: say(lang, Msg::SessionContextUnknown).to_owned(),
        known: false,
    };
    let handoff = match row.handoff_at_turn {
        None => Fact {
            said: say(lang, Msg::SessionHandoffNone).to_owned(),
            known: true,
        },
        Some(at) => {
            let ago = row.turns.saturating_sub(at);
            Fact {
                said: if ago == 0 {
                    say(lang, Msg::SessionHandoffJust).to_owned()
                } else {
                    fill(say(lang, Msg::SessionHandoffAt), &[("n", &ago.to_string())])
                },
                known: true,
            }
        }
    };
    [spent, gate, context, handoff]
}

/// Which part of a session is being read.
///
/// Tabs rather than four panels down one page: they are four readings of
/// one session and only one is wanted at a time, so stacking them makes
/// a person scroll past three answers to reach the one they came for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Tab {
    #[default]
    Turns,
    Changes,
    Cost,
    Docs,
}

impl Tab {
    /// Every tab, in reading order: what it did, what that changed, what
    /// it cost, and what it wrote down.
    pub const ALL: [Tab; 4] = [Tab::Turns, Tab::Changes, Tab::Cost, Tab::Docs];

    /// What the tab is called.
    #[must_use]
    pub fn word(self) -> Msg {
        match self {
            Self::Turns => Msg::SessionTabTurns,
            Self::Changes => Msg::SessionTabChanges,
            Self::Cost => Msg::SessionTabCost,
            Self::Docs => Msg::SessionTabDocs,
        }
    }
}

/// One session.
#[component]
#[allow(
    clippy::too_many_arguments,
    reason = "one page, one prop per fact it draws"
)]
pub fn SessionView(
    addr: Address,
    snapshot: Snapshot,
    records: Vec<channels::EventRecord>,
    /// What this session changed on disk, once the server has said.
    changes: Option<channels::ChangesAnswer>,
    /// This city's spend, for the one row that belongs to this run.
    cost: Option<channels::CostAnswer>,
    /// The building this room is in, for the documents tab.
    building: Option<channels::BuildingAnswer>,
    /// A line a drop wrote into this session's box, unsent. It stops in
    /// the box: this session is already spending, which is exactly where
    /// a gesture nobody could take back would cost the most.
    steered: Option<String>,
    live: Signal<bool>,
    on_frame: EventHandler<channels::ClientFrame>,
    on_drop: EventHandler<(crate::drop::Target, crate::drop::Dropped)>,
) -> Element {
    let lang = use_context::<Signal<Lang>>();
    let word = move |msg: Msg| say(lang(), msg);
    let mut tab = use_signal(Tab::default);
    // Whether the feed scrolls itself. A fact about reading one session,
    // so it is this page's and does not outlive it.
    let mut following = use_signal(|| true);
    let found = snapshot.session_at(&addr);

    let Some((run, row)) = found else {
        return rsx! {
            crate::panel::Panel {
                title: word(Msg::SessionUnknown).to_owned(),
                scope: None,
                figure: None,
                source: word(Msg::SessionSource).to_owned(),
                crate::panel::Empty {
                    status: word(Msg::SessionUnknown).to_owned(),
                    what: word(Msg::SessionUnknownWhat).to_owned(),
                    a { class: "nav-item", href: "#/", "{word(Msg::SessionAllSessions)}" }
                }
            }
        };
    };
    let facts = head_facts(lang(), row);
    let phase = row.phase;
    let turns = row.turns;
    let mine: Vec<channels::EventRecord> = records
        .iter()
        .filter(|held| held.run() == run)
        .cloned()
        .collect();

    rsx! {
        header { class: "session-head",
            a { class: "session-back", href: "#/", "{word(Msg::SessionAllSessions)}" }
            h1 { class: "address",
                span {
                    class: "phase {phase.token()}",
                    role: "img",
                    "aria-label": "{say(lang(), phase.word())}",
                }
                span { class: "room", "{addr.as_str()}" }
                span { class: "turn",
                    {fill(word(Msg::SessionTurnOrdinal), &[("n", &turns.to_string())])}
                }
            }
            p { class: "session-facts",
                for fact in facts {
                    span {
                        key: "{fact.said}",
                        class: if fact.known { "fact" } else { "fact unknown" },
                        "{fact.said}"
                    }
                }
            }
            p { class: "panel-scope", "{word(Msg::SessionScope)}" }
            p { class: "panel-scope", "{word(Msg::SessionContextScope)}" }
        }

        nav { class: "session-tabs",
            for one in Tab::ALL {
                button {
                    key: "{one:?}",
                    class: "tab",
                    "aria-current": if tab() == one { "page" } else { "false" },
                    onclick: move |_| tab.set(one),
                    "{word(one.word())}"
                }
            }
        }

        match tab() {
            Tab::Turns => rsx! {
                // What the model is saying right now, if it is saying
                // anything. Above the turns because it is the newest
                // thing, and gone the moment `model_returned` lands: the
                // record below then carries the settled text, which is
                // the text a replay produces.
                if let Some(said) = snapshot.saying(&run) {
                    p { class: "said saying", "{said}" }
                }
                crate::live::LiveView {
                    feed: crate::live::Feed::replay(mine.iter(), Some(run), true),
                    turns: crate::turn::turns(mine.iter()),
                    run: Some(run),
                    runs: Vec::new(),
                    following: following(),
                    steered: steered.clone(),
                    changes: None,
                    live,
                    on_frame,
                    on_follow: move |on| following.set(on),
                    on_drop,
                    on_watch: move |_| {},
                }
            },
            Tab::Changes => rsx! {
                crate::live::Changed { changes: changes.clone() }
            },
            Tab::Cost => rsx! {
                crate::dashboard::CostsView {
                    answer: cost.clone(),
                    usage: snapshot.usage(),
                    spent: row.spent.unwrap_or_default(),
                    live,
                    on_frame,
                }
            },
            Tab::Docs => rsx! {
                crate::building_view::BuildingView {
                    addr: building_of(&addr).unwrap_or_else(|| addr.clone()),
                    answer: building.clone(),
                    inbox: None,
                    signals: snapshot.signals_seen(),
                    live,
                    on_frame,
                    on_select: move |_| {},
                    on_drop,
                }
            },
        }
    }
}

/// The building a room is in. A room address is `building/room`, so the
/// building is everything before the last separator; an address with no
/// separator is already a building.
#[must_use]
pub fn building_of(addr: &Address) -> Option<Address> {
    let (building, _) = addr.as_str().rsplit_once('/')?;
    Address::parse(building).ok()
}

/// Which run an old `#/live/<uuid>` link means, once the client knows
/// which room it was in.
///
/// The router is pure and the room is a fact only the snapshot holds, so
/// the redirect happens here rather than in `route`. `None` means this
/// client has not folded that run's start yet, and the page says it is
/// still asking rather than that the link is broken.
#[must_use]
pub fn room_for_link(snapshot: &Snapshot, view: &View) -> Option<View> {
    let View::Run(run) = view else {
        return None;
    };
    snapshot.room_of(run).cloned().map(View::Session)
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
    use super::{Tab, building_of, head_facts};
    use crate::app::RunRow;
    use crate::lang::Lang;
    use crate::phase::Phase;

    fn row() -> RunRow {
        RunRow {
            addr: channels::Address::parse("lab/parser").ok(),
            session: Some("parser".to_owned()),
            parent: None,
            phase: Phase::Running,
            steps_done: 0,
            steps_planned: None,
            started_at_seq: channels::Seq::FIRST,
            last_seq: channels::Seq::FIRST,
            turns: 7,
            handoff_at_turn: None,
            gate: None,
            spent: None,
            said: None,
        }
    }

    /// The head says four things and only four. A page that reported
    /// whatever it happened to know would report a different set each
    /// time it was opened.
    #[test]
    fn the_head_answers_four_questions_and_no_others() {
        assert_eq!(head_facts(Lang::En, &row()).len(), 4);
        assert_eq!(Tab::ALL.len(), 4);
    }

    /// The whole of the fourth question's answer: the city is not asked
    /// to guess. This is the assertion the card closes on.
    #[test]
    fn the_context_left_is_a_rule_and_never_a_number() {
        let mut row = row();
        row.turns = 40;
        row.spent = Some(channels::UsdMicros::new(1_360_000));
        let facts = head_facts(Lang::En, &row);
        let context = &facts[2];
        assert!(!context.known, "the wire does not carry this one");
        assert!(
            context.said.contains("——"),
            "an unanswerable fact is drawn as a rule, not as a figure: {}",
            context.said
        );
        assert!(
            !context.said.chars().any(|glyph| glyph.is_ascii_digit()),
            "a number here would be invented: {}",
            context.said
        );
    }

    /// A session nobody has priced does not read as a free one.
    #[test]
    fn an_unpriced_session_says_so_rather_than_saying_zero() {
        let facts = head_facts(Lang::En, &row());
        assert!(!facts[0].known);
        assert!(!facts[0].said.contains("0.00"));
    }

    /// The rule the streaming half closes on: an increment is something
    /// to watch, and the record is what is true.
    ///
    /// A provider that revises, or a stream cut halfway, leaves text in
    /// the buffer that no `model_returned` ever confirmed. Dropping the
    /// buffer when the record lands is the whole of the rule, and it is
    /// asserted on the fold rather than on the markup because the fold is
    /// where it is decided.
    #[test]
    fn where_the_increments_and_the_record_disagree_the_record_wins() {
        // Seated first, because a run this client never saw start has no
        // row to carry the settled text - which is a different defect
        // from the one under test.
        let mut snapshot = crate::app::seated(&[(Some("lab/parser"), Phase::Running, 1)]);
        let run = channels::RunId::from_bytes([0u8; 16]);
        snapshot.is_saying(&channels::Delta {
            run,
            text: "half a sen".to_owned(),
        });
        assert_eq!(snapshot.saying(&run), Some("half a sen"));
        snapshot.is_saying(&channels::Delta {
            run,
            text: "tence".to_owned(),
        });
        assert_eq!(
            snapshot.saying(&run),
            Some("half a sentence"),
            "increments join; keeping only the last would show every fortieth word"
        );

        snapshot.apply(&crate::app::returned_for_test(run, "the whole sentence", 9));
        assert_eq!(
            snapshot.saying(&run),
            None,
            "the call settled, so nothing is still being said"
        );
        let Some(row) = snapshot.run(&run) else {
            panic!("the record seats the run");
        };
        assert_eq!(
            row.said.as_deref(),
            Some("the whole sentence"),
            "the page draws what the record says, not what arrived before it"
        );
    }

    /// The three answerable ones are answered from records this client
    /// already folds, which is what makes them free.
    #[test]
    fn the_three_answerable_facts_come_from_the_stream() {
        let mut row = row();
        row.spent = Some(channels::UsdMicros::new(420_000));
        row.gate = Some("exec".to_owned());
        row.handoff_at_turn = Some(4);
        let facts = head_facts(Lang::En, &row);
        assert!(facts[0].known && facts[0].said.contains("$0.42"));
        assert!(facts[1].known && facts[1].said.contains("exec"));
        assert!(
            facts[3].known && facts[3].said.contains('3'),
            "7 - 4 turns ago"
        );
    }

    /// A handoff written this turn is current, and saying "0 turns ago"
    /// makes a reader work out that it means now.
    #[test]
    fn a_handoff_written_this_turn_says_this_turn() {
        let mut row = row();
        row.handoff_at_turn = Some(7);
        let facts = head_facts(Lang::En, &row);
        assert_eq!(facts[3].said, "handoff written this turn");
    }

    #[test]
    fn a_room_knows_which_building_it_is_in() {
        let addr = channels::Address::parse("lab/parser").unwrap();
        assert_eq!(building_of(&addr).unwrap().as_str(), "lab");
        let bare = channels::Address::parse("lab").unwrap();
        assert_eq!(building_of(&bare), None);
    }
}
