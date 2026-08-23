// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The one version of a centre panel, and the one version of an empty one.
//!
//! Before this module every page invented its own arrangement: a bold line
//! that might be a conclusion or might be a noun, a grey line that might be
//! scope or might be an apology, and no line at all saying where a number
//! came from. Four parts, in one place, so two pages cannot disagree about
//! what a heading means.
//!
//! **The fourth part is the one this product may not ship without.** A city
//! whose whole claim is an auditable Ledger, showing a figure that does not
//! say what produced it, is asking to be trusted on exactly the point it
//! promised to prove.
//!
//! There are no tests here. What is worth holding is not that this markup
//! renders - it has no decision in it - but that *every page* goes through
//! it, and that assertion can only be written where the pages are rendered:
//! `app`'s test module walks all nine views and fails if one of them states
//! a number without naming its source.

use dioxus::prelude::*;

/// One centre panel.
///
/// `title` is the **conclusion**, not the noun: "nothing has been spent
/// yet" rather than "cost". A reader who only reads headings should come
/// away with the true state of the city, which is the test a noun fails.
///
/// `figure` is the one number the panel exists to state, when there is one.
/// It sits beside the title rather than under it, because a number with a
/// sentence beside it is read as an answer while a number alone is read as
/// a score.
///
/// `source` says where the numbers came from - which query, which stream,
/// which file. Required rather than optional: a panel that cannot name its
/// source has not finished being designed.
#[component]
pub(crate) fn Panel(
    title: String,
    scope: Option<String>,
    figure: Option<String>,
    source: String,
    children: Element,
) -> Element {
    rsx! {
        section { class: "panel",
            div { class: "panel-head",
                h2 { class: "panel-title", "{title}" }
                if let Some(figure) = figure.clone() {
                    span { class: "panel-figure", "{figure}" }
                }
            }
            if let Some(scope) = scope.clone() {
                p { class: "panel-scope", "{scope}" }
            }
            div { class: "panel-body", {children} }
            p { class: "panel-source", "{source}" }
        }
    }
}

/// A container with nothing in it, said in three parts.
///
/// `status` is what the system's state actually is, and it must
/// distinguish the three cases a reader would otherwise have to guess
/// between: nothing has happened yet, the answer has not arrived yet, and
/// a filter excluded everything. `what` says what would be here and what
/// puts it here. The children are the way to do that, when there is a way
/// that can be taken from this page.
///
/// A bare sentence in an empty pane is read as a broken pane. That is the
/// finding this shape answers, and it is why nothing in this library
/// renders an empty container any other way.
#[component]
pub(crate) fn Empty(status: String, what: String, children: Element) -> Element {
    rsx! {
        div { class: "empty",
            span { class: "empty-status", "{status}" }
            span { class: "empty-what", "{what}" }
            div { class: "empty-way", {children} }
        }
    }
}
