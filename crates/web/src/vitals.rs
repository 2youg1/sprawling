// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The city's vital signs: the few numbers no other surface states.
//!
//! `MetricsAnswer` carries seven counts, and this strip shows three. That
//! is the whole design of the module. The other four are already on screen
//! somewhere, and a number with two homes is a number that will one day
//! disagree with itself in front of a person who has no way to tell which
//! one is right:
//!
//! | not shown here | where it already lives |
//! |---|---|
//! | `buildings` | the city picture and its index, on the same page |
//! | `runs_active` | the same picture: a building with work in it is lit |
//! | `runs_frozen` | the live page, which lists runs and their phase |
//! | `approvals_waiting` | the left nav's badge, and the approvals page |
//!
//! What is left is what nothing else can say:
//!
//! - **how long the Ledger is** - the client sees only what arrived after
//!   it connected, and the ledger page says so rather than implying it
//!   holds everything. This is the number that closes that admission.
//! - **how many signals wait in rooms** - a building page shows one room's
//!   queue at a time, and no page adds them up.
//! - **how much was discarded and not taken back** - the Recycle Bin lists
//!   rows; this is the only count of them.
//!
//! The reading is a point in time. It is asked for when the page opens and
//! not kept warm: re-asking on every event would be polling with extra
//! steps, and the fold that keeps the rest of the interface live cannot
//! produce these numbers - that is why they are a query.

use channels::{ClientFrame, MetricsAnswer, Query};
use dioxus::prelude::*;

/// One readout: what it counts, and the number.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Sign {
    /// What this readout counts, as a message the reader's own language
    /// answers; the words live in `web::lang`.
    pub what: crate::lang::Msg,
    pub count: u64,
}

/// The three signs this strip shows, in reading order.
///
/// A function rather than markup, so the choice of what earns a permanent
/// readout is one decision in one place - and so the module note above can
/// be checked against it.
#[must_use]
pub fn signs(answer: &MetricsAnswer) -> [Sign; 3] {
    [
        Sign {
            what: crate::lang::Msg::VitalsRecords,
            count: answer.events,
        },
        Sign {
            what: crate::lang::Msg::VitalsSignals,
            count: answer.signals_waiting,
        },
        Sign {
            what: crate::lang::Msg::VitalsDiscards,
            count: answer.discards_outstanding,
        },
    ]
}

/// The city's vital signs, above the picture of it.
#[component]
pub fn Vitals(
    answer: Option<MetricsAnswer>,
    /// Whether the socket is live; see `app::Root`.
    live: Signal<bool>,
    on_frame: EventHandler<ClientFrame>,
) -> Element {
    let lang = use_context::<Signal<crate::lang::Lang>>();
    let asked = use_signal(|| false);
    use_effect(move || {
        let mut asked = asked;
        if live() && !asked() {
            asked.set(true);
            on_frame.call(ClientFrame::Query(Query::Metrics));
        }
    });
    let Some(answer) = answer else {
        return rsx! {
            p { class: "vitals empty", "{crate::lang::say(lang(), crate::lang::Msg::VitalsAsking)}" }
        };
    };
    rsx! {
        div { class: "vitals",
            for sign in signs(&answer) {
                span { key: "{sign.what:?}", class: "sign",
                    b { "{sign.count}" }
                    " {crate::lang::say(lang(), sign.what)}"
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

    fn vital() -> MetricsAnswer {
        MetricsAnswer {
            events: 12_400,
            runs_active: 2,
            runs_frozen: 5,
            buildings: 3,
            approvals_waiting: 1,
            signals_waiting: 4,
            discards_outstanding: 7,
        }
    }

    #[test]
    fn the_strip_shows_only_what_no_other_surface_states() {
        let shown = signs(&vital());
        let counts: Vec<u64> = shown.iter().map(|sign| sign.count).collect();
        assert_eq!(counts, vec![12_400, 4, 7]);
        // The four that live elsewhere must not appear here, or the
        // interface acquires two answers to one question.
        let elsewhere = [
            vital().buildings,
            vital().runs_active,
            vital().runs_frozen,
            vital().approvals_waiting,
        ];
        for count in elsewhere {
            assert!(
                !counts.contains(&count),
                "{count} is stated by another surface and repeated here"
            );
        }
    }

    #[test]
    fn every_sign_says_what_it_counts_in_both_languages() {
        for sign in signs(&vital()) {
            let said = crate::lang::phrase(sign.what);
            for words in [said.en, said.zh] {
                assert!(!words.is_empty());
                assert!(!words.contains('$'), "money belongs to the cost page");
            }
        }
    }
}
