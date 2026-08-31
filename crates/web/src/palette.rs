// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! One box that reaches every page, every building and every session.
//!
//! It exists because addressing this city was a typing exercise: a
//! building had no nav entry - a city may hold fifty - and the only route
//! to a room was to write its address into the control surface by hand.
//! The nav answers "which page", and nothing answered "which of the fifty".
//!
//! **Ranking is a pure function and the component is a shell over it.**
//! What a query matches is the part worth being sure of, so it is decided
//! here, tested on the host, and handed to the view as a list.
//!
//! The order is by how the query matched rather than by how good a guess
//! this module made: a name that begins with what was typed outranks one
//! that merely contains it, which outranks one whose letters appear in
//! order. Inside one rank the caller's order survives, so the pages stay
//! in nav order and the sessions stay newest-first.

use dioxus::prelude::*;

use crate::app::View;
use crate::lang::{Msg, say};

/// What sort of thing a row leads to. Shown beside the name because
/// "lab" can be a building and "lab/parser" a session, and a list that
/// does not say which is a list that has to be guessed at.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Page,
    Building,
    Session,
}

impl Kind {
    /// The word for this kind, as a message rather than a string: the same
    /// rule the rest of the client follows, so a kind cannot be the one
    /// English word left on a Chinese page.
    #[must_use]
    pub fn word(self) -> Msg {
        match self {
            Self::Page => Msg::PaletteKindPage,
            Self::Building => Msg::PaletteKindBuilding,
            Self::Session => Msg::PaletteKindSession,
        }
    }
}

/// One thing the palette can reach.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Offer {
    pub label: String,
    pub kind: Kind,
    pub going: View,
}

/// How a query met a name, worst last. The order of the variants is the
/// ranking, so `derive(Ord)` is the ranking rather than a comparator
/// somebody has to keep in step with it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Rank {
    /// The name begins with what was typed.
    Opens,
    /// The name holds it somewhere.
    Holds,
    /// The letters appear in order, with anything between them.
    Scattered,
}

/// The offers a query reaches, best first.
///
/// An empty query answers with everything: the palette opens on a list of
/// where a person can go, rather than on a blank that has to be guessed at.
#[must_use]
pub fn matching(query: &str, offers: Vec<Offer>) -> Vec<Offer> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return offers;
    }
    let mut ranked: Vec<(Rank, usize, Offer)> = offers
        .into_iter()
        .enumerate()
        .filter_map(|(at, offer)| {
            rank(&offer.label.to_lowercase(), &needle).map(|r| (r, at, offer))
        })
        .collect();
    // By rank, then by the order the caller gave: sorting is stable, and
    // the caller's order already means something.
    ranked.sort_by_key(|&(rank, at, _)| (rank, at));
    ranked.into_iter().map(|(_, _, offer)| offer).collect()
}

/// How `needle` met `hay`, or `None` when it did not.
fn rank(hay: &str, needle: &str) -> Option<Rank> {
    if hay.starts_with(needle) {
        return Some(Rank::Opens);
    }
    if hay.contains(needle) {
        return Some(Rank::Holds);
    }
    scattered(hay, needle).then_some(Rank::Scattered)
}

/// Whether every character of `needle` appears in `hay`, in order.
///
/// This is what lets `cpr` reach `crates/parser`, which is the shape of
/// the names in this product: a reader types the initials of a path
/// rather than a substring of it.
fn scattered(hay: &str, needle: &str) -> bool {
    let mut rest = hay.chars();
    needle.chars().all(|wanted| rest.any(|have| have == wanted))
}

/// The command palette.
///
/// Holds no list of its own: what it can reach is assembled by the caller,
/// which already knows the nav, the city answer and the running sessions.
/// A second list here would be a second answer to "where can a person go".
#[component]
pub fn Palette(
    offers: Vec<Offer>,
    on_go: EventHandler<View>,
    on_close: EventHandler<()>,
) -> Element {
    let lang = use_context::<Signal<crate::lang::Lang>>();
    let word = move |msg: Msg| say(lang(), msg);
    let mut query = use_signal(String::new);
    let found = matching(&query(), offers);
    rsx! {
        div {
            class: "palette-scrim",
            // The scrim is a way out, not a decoration: a person who opened
            // this by accident should not have to find the key again.
            onclick: move |_| on_close.call(()),
            div {
                class: "palette",
                // Clicks inside are not a dismissal.
                onclick: move |event| event.stop_propagation(),
                input {
                    class: "palette-query",
                    r#type: "text",
                    autofocus: true,
                    value: "{query}",
                    placeholder: "{word(Msg::PalettePlaceholder)}",
                    oninput: move |event| query.set(event.value()),
                }
                if found.is_empty() {
                    div { class: "palette-empty",
                        span { class: "empty-status", "{word(Msg::PaletteNothing)}" }
                        span { class: "empty-what", "{word(Msg::PaletteNothingWhat)}" }
                    }
                } else {
                    ul { class: "palette-list",
                        for offer in found {
                            li { key: "{offer.kind:?}-{offer.label}",
                                button {
                                    class: "palette-row",
                                    onclick: {
                                        let going = offer.going.clone();
                                        move |_| on_go.call(going.clone())
                                    },
                                    span { class: "palette-label", "{offer.label}" }
                                    span { class: "palette-kind", "{word(offer.kind.word())}" }
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
    use super::{Kind, Offer, Palette, matching};
    use crate::app::View;

    fn offer(label: &str, kind: Kind) -> Offer {
        Offer {
            label: label.to_owned(),
            kind,
            going: View::Overview,
        }
    }

    fn labels(found: &[Offer]) -> Vec<&str> {
        found.iter().map(|o| o.label.as_str()).collect()
    }

    #[test]
    fn an_empty_query_offers_everywhere_rather_than_nothing() {
        // The palette opens on a list of destinations. A blank box that
        // answers nothing until it is typed into teaches nobody what is
        // reachable through it.
        let all = vec![offer("overview", Kind::Page), offer("lab", Kind::Building)];
        assert_eq!(labels(&matching("  ", all.clone())), ["overview", "lab"]);
    }

    #[test]
    fn a_name_that_opens_with_the_query_beats_one_that_merely_holds_it() {
        let all = vec![
            offer("recycle bin", Kind::Page),
            offer("binding", Kind::Building),
        ];
        assert_eq!(labels(&matching("bin", all)), ["binding", "recycle bin"]);
    }

    #[test]
    fn initials_reach_a_path_because_that_is_how_these_names_are_shaped() {
        let all = vec![offer("crates/parser", Kind::Building)];
        assert_eq!(labels(&matching("cpr", all)), ["crates/parser"]);
    }

    #[test]
    fn scattered_letters_rank_below_a_real_substring() {
        let all = vec![
            offer("lab/parser", Kind::Building),
            offer("my-lpr-tool", Kind::Building),
        ];
        // `lpr` runs whole through the second name and is only scattered
        // through the first, so the substring wins despite being listed
        // second - which is the point: rank outranks the caller's order.
        assert_eq!(labels(&matching("lpr", all)), ["my-lpr-tool", "lab/parser"]);
    }

    #[test]
    fn a_query_nothing_answers_returns_nothing_rather_than_everything() {
        let all = vec![offer("overview", Kind::Page)];
        assert!(matching("zzz", all).is_empty());
    }

    #[test]
    fn matching_ignores_case_on_both_sides() {
        let all = vec![offer("Recycle Bin", Kind::Page)];
        assert_eq!(labels(&matching("RECYCLE", all)), ["Recycle Bin"]);
    }

    #[test]
    fn every_kind_says_which_it_is() {
        // A list where "lab" and "lab/parser" sit together has to say which
        // is a building and which a session, or it has to be guessed at.
        for kind in [Kind::Page, Kind::Building, Kind::Session] {
            let _: crate::lang::Msg = kind.word();
        }
    }

    #[test]
    fn the_component_exists_for_the_route_the_tests_above_cover() {
        let _ = Palette;
    }
}
