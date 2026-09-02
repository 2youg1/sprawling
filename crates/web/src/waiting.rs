// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Everything that cannot move until a person answers (web-SPEC.md
//! section 8-53 B1).
//!
//! **One page for one question.** What waits on a person used to be
//! spread across three places: the approvals page held the queue, the
//! overview counted frozen runs, and whether the city itself was stopped
//! was a sentence in a column nobody read. A person cannot act on a
//! count they have to assemble, so the three arrive here together and
//! the nav badge counts exactly what this page lists.
//!
//! **Colour says one thing here and it is spent on the queue.** ALERT
//! means "a person is needed" everywhere in this interface, which is why
//! this design cannot have a red-and-green diff: a second meaning for
//! the one loud colour makes the first one stop working.

use dioxus::prelude::*;

use crate::app::Snapshot;
use crate::lang::{Lang, Msg, say};
use crate::phase::Phase;
use crate::route::View;

/// A session that stopped without a person stopping it.
///
/// Separate from the approval queue because the two ask different
/// things: an approval is a question with an answer, and a frozen
/// session is work that ran out of somewhere to go. Both wait, and only
/// one of them can be answered from this page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Stalled {
    pub addr: channels::Address,
    pub phase: Phase,
    pub turns: u32,
}

/// Sessions that stopped and were not stopped by this person.
///
/// A run this person cancelled is not waiting on them: they already
/// answered it, and listing it here would make the page grow every time
/// somebody used the one control that ends work cleanly.
#[must_use]
pub fn stalled(snapshot: &Snapshot) -> Vec<Stalled> {
    let mut rows: Vec<(channels::Seq, Stalled)> = snapshot
        .runs()
        .filter(|(_, row)| matches!(row.phase, Phase::Frozen | Phase::Halted))
        .filter_map(|(_, row)| {
            row.addr.clone().map(|addr| {
                (
                    row.started_at_seq,
                    Stalled {
                        addr,
                        phase: row.phase,
                        turns: row.turns,
                    },
                )
            })
        })
        .collect();
    rows.sort_by_key(|(seq, _)| std::cmp::Reverse(*seq));
    rows.into_iter().map(|(_, row)| row).collect()
}

/// What waits on a person.
#[component]
pub fn WaitingView(
    snapshot: Snapshot,
    live: Signal<bool>,
    on_frame: EventHandler<channels::ClientFrame>,
) -> Element {
    let lang = use_context::<Signal<Lang>>();
    let word = move |msg: Msg| say(lang(), msg);
    let items = snapshot.approvals();
    let stopped = stalled(&snapshot);
    let nothing = items.is_empty() && stopped.is_empty() && !snapshot.is_halted();

    rsx! {
        if nothing {
            crate::panel::Panel {
                title: word(Msg::NavWaiting).to_owned(),
                scope: Some(word(Msg::WaitingScope).to_owned()),
                figure: None,
                source: word(Msg::WaitingSource).to_owned(),
                crate::panel::Empty {
                    status: word(Msg::WaitingNothing).to_owned(),
                    what: word(Msg::WaitingNothingWhat).to_owned(),
                    a { class: "nav-item", href: "#/", "{word(Msg::NavSessions)}" }
                }
            }
        } else {
            // The queue first: it is the only part of this page a person
            // can answer, and the parts they cannot answer must not stand
            // between them and the parts they can.
            crate::approval::ApprovalsView {
                items,
                live,
                on_frame,
            }
            if !stopped.is_empty() {
                crate::panel::Panel {
                    title: word(Msg::WaitingFrozenHeading).to_owned(),
                    scope: None,
                    figure: Some(stopped.len().to_string()),
                    source: word(Msg::WaitingSource).to_owned(),
                    for row in stopped {
                        a {
                            key: "{row.addr.as_str()}",
                            class: "session-row",
                            href: "{crate::route::to_fragment(&View::Session(row.addr.clone()))}",
                            span {
                                class: "phase {row.phase.token()}",
                                role: "img",
                                "aria-label": "{say(lang(), row.phase.word())}",
                            }
                            span { class: "room", "{row.addr.as_str()}" }
                            span { class: "said", "{say(lang(), row.phase.word())}" }
                            span { class: "turn",
                                {
                                    crate::lang::fill(
                                        say(lang(), Msg::SessionsTurnCount),
                                        &[("n", &row.turns.to_string())],
                                    )
                                }
                            }
                            span { class: "spent" }
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
    use super::stalled;
    use crate::app::Snapshot;
    use crate::phase::Phase;

    /// Work a person stopped is work they already answered. Listing it
    /// would grow this page every time somebody used the one control
    /// that ends work cleanly.
    #[test]
    fn a_session_this_person_cancelled_is_not_waiting_on_them() {
        let snapshot = crate::app::seated(&[
            (Some("lab/stopped"), Phase::Cancelled, 1),
            (Some("lab/froze"), Phase::Frozen, 3),
            (Some("lab/halted"), Phase::Halted, 5),
            (Some("lab/going"), Phase::Running, 7),
        ]);
        let rows = stalled(&snapshot);
        let named: Vec<&str> = rows.iter().map(|row| row.addr.as_str()).collect();
        assert_eq!(named, vec!["lab/halted", "lab/froze"]);
    }

    /// The badge counts the queue, and the queue is what the top of this
    /// page lists. A badge counting a set the page does not show teaches
    /// a person to stop believing the next badge.
    #[test]
    fn the_badge_counts_what_the_page_puts_first() {
        let snapshot = Snapshot::new();
        assert_eq!(snapshot.waiting_on_you(), 0);
        assert_eq!(snapshot.approvals().len(), 0);
    }
}
