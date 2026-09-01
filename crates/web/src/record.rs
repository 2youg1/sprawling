// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! What this city wrote down, in three lenses over one history
//! (web-SPEC.md section 8-53 B1).
//!
//! **Three lenses, one destination.** The ledger, the archive and the
//! recycle bin each had a nav entry, and a person had to choose between
//! them before knowing what they were looking for — which is the wrong
//! order, because the choice only makes sense once the question is
//! formed. They are three readings of one append-only history, so they
//! are one place with a switch in it.
//!
//! The lens is in the address, so a link to the archive is still a link
//! to the archive. It is not a filter the page forgets: a reader who
//! sends somebody a link sends them what they were looking at.

use dioxus::prelude::*;

use crate::app::{Lens, View};
use crate::lang::{Lang, Msg, say};

/// One history, read three ways.
#[component]
#[allow(
    clippy::too_many_arguments,
    reason = "one page, one prop per lens it draws"
)]
pub fn RecordView(
    lens: Lens,
    records: Vec<channels::EventRecord>,
    /// What the archive search answered, when one was asked.
    hits: Option<channels::ArchiveAnswer>,
    /// What went on the shelves lately.
    filed: Option<channels::RegistryAnswer>,
    /// What was discarded, and the way each row comes back.
    discards: Option<channels::DiscardAnswer>,
    /// How much this city has written down. Stated under the ledger
    /// lens, because it is a fact about the ledger rather than about a
    /// page: it used to stand in a permanent right-hand column, where it
    /// was true on every page and relevant on one.
    vitals: Option<channels::MetricsAnswer>,
    live: Signal<bool>,
    on_frame: EventHandler<channels::ClientFrame>,
) -> Element {
    let lang = use_context::<Signal<Lang>>();
    let word = move |msg: Msg| say(lang(), msg);
    rsx! {
        header { class: "record-head",
            h1 { class: "address", "{word(Msg::NavTheRecord)}" }
            p { class: "panel-scope", "{word(Msg::RecordScope)}" }
        }
        // Anchors rather than buttons: the lens is in the address, so
        // each one is a place. Middle click, the keyboard and "copy link
        // address" all arrive without a handler being written for them.
        nav { class: "session-tabs",
            for one in Lens::ALL {
                a {
                    key: "{one:?}",
                    class: "tab",
                    href: "{crate::route::to_fragment(&View::Record(one))}",
                    "aria-current": if lens == one { "page" } else { "false" },
                    "{word(one.word())}"
                }
            }
        }
        match lens {
            Lens::Ledger => rsx! {
                crate::vitals::Vitals { answer: vitals.clone(), live, on_frame }
                crate::ledger_view::LedgerView { records: records.clone(), on_frame }
            },
            Lens::Archive => rsx! {
                crate::archive_search::ArchiveView {
                    hits: hits.clone(),
                    filed: filed.clone(),
                    live,
                    on_frame,
                }
            },
            Lens::Bin => rsx! {
                crate::approval::RecycleBinView {
                    answer: discards.clone(),
                    live,
                    on_frame,
                }
            },
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
    use crate::app::Lens;
    use crate::route::{from_fragment, to_fragment};

    /// A reader who sends somebody a link sends them what they were
    /// looking at. A lens the page held in a signal instead would send
    /// every recipient to the first one.
    #[test]
    fn the_lens_travels_in_the_link() {
        for lens in Lens::ALL {
            let view = crate::app::View::Record(lens);
            assert_eq!(from_fragment(&to_fragment(&view)), Some(view));
        }
    }

    /// Three, and the order is the order of the questions: the whole
    /// history, then what was kept, then what was thrown away.
    #[test]
    fn the_three_lenses_read_in_one_order() {
        assert_eq!(Lens::ALL, [Lens::Ledger, Lens::Archive, Lens::Bin]);
        assert_eq!(Lens::default(), Lens::Ledger);
    }
}
