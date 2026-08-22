// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! What this city wrote down, and where it is.
//!
//! Two halves, two sources, and the page says which is which:
//!
//! - **A search reads the shelves.** `ArchiveSearch` walks every building's
//!   archive at the moment of asking, because the files are the authority
//!   and an index kept beside them would be a second copy of what the disk
//!   says (city-SPEC, `city::archive_index`).
//! - **"Filed lately" reads the record.** `RegistryView` folds
//!   `asset_archived` out of the Ledger, so it can say *when* something was
//!   filed and by which address - facts a directory listing does not carry.
//!
//! They are never merged into one list. The same entry can appear in both,
//! and a reader who cannot tell which list they are looking at cannot tell
//! whether they are reading the disk or the history.
//!
//! **An empty needle asks nothing.** Searching for the empty string matches
//! every entry, which would put a second complete list on the page next to
//! the first - and it would cost a walk of every building to produce it.

use std::collections::BTreeMap;

use channels::{Address, ArchiveAnswer, ArchiveHit, ClientFrame, Query, RegistryAnswer};
use channels::{RegistryLine, TimeMs};
use dioxus::prelude::*;

/// How many recently filed rows the page shows before it stops.
///
/// The registry grows for the life of a city, and a page that renders all
/// of it is a page that stops opening. The cap is stated on screen: a
/// silently shortened list is the difference between a view and a lie.
pub const FILED_LATELY_MAX: usize = 20;

/// The needle worth asking the city about, if there is one.
///
/// Trimmed, because a trailing space is a typing artefact rather than a
/// search term. `None` means "do not ask" - not "search for everything".
#[must_use]
pub fn searchable(needle: &str) -> Option<String> {
    let trimmed = needle.trim();
    (!trimmed.is_empty()).then(|| trimmed.to_owned())
}

/// One building's hits.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shelf {
    pub building: Address,
    pub hits: Vec<ArchiveHit>,
}

/// Groups hits by the building that keeps them.
///
/// Buildings in address order and hits newest first inside each, so two
/// people searching the same word see the same page. Grouping is what
/// turns a flat list of forty lines into an answer to "where was this
/// decided".
#[must_use]
pub fn shelves(answer: &ArchiveAnswer) -> Vec<Shelf> {
    let mut grouped: BTreeMap<String, Vec<ArchiveHit>> = BTreeMap::new();
    for hit in &answer.hits {
        grouped
            .entry(hit.building.as_str().to_owned())
            .or_default()
            .push(hit.clone());
    }
    grouped
        .into_iter()
        .filter_map(|(building, mut hits)| {
            hits.sort_by(|left, right| {
                right
                    .day
                    .cmp(&left.day)
                    .then_with(|| left.kind.cmp(&right.kind))
                    .then_with(|| left.subject.cmp(&right.subject))
            });
            Address::parse(&building)
                .ok()
                .map(|building| Shelf { building, hits })
        })
        .collect()
}

/// What was filed most recently, newest first, capped.
#[must_use]
pub fn filed_lately(answer: &RegistryAnswer, most: usize) -> Vec<RegistryLine> {
    let mut rows = answer.assets.clone();
    rows.sort_by(|left, right| {
        right
            .at
            .cmp(&left.at)
            .then_with(|| left.addr.as_str().cmp(right.addr.as_str()))
            .then_with(|| left.subject.cmp(&right.subject))
    });
    rows.truncate(most);
    rows
}

/// The sentence above the recent list, which states the cap rather than
/// applying it silently.
#[must_use]
pub fn filed_line(answer: &RegistryAnswer, shown: usize) -> String {
    let total = answer.assets.len();
    let held = total.saturating_sub(shown);
    if held == 0 {
        format!("{total} filed, from the record")
    } else {
        format!("{total} filed, from the record - showing the {shown} most recent, {held} older")
    }
}

/// A filing time, rendered as the record holds it.
#[must_use]
pub fn filed_at(at: TimeMs) -> String {
    format!("t+{}", at.value())
}

/// The archive page.
#[component]
pub fn ArchiveView(
    hits: Option<ArchiveAnswer>,
    filed: Option<RegistryAnswer>,
    /// Whether the socket is live; see `app::Root`.
    live: Signal<bool>,
    on_frame: EventHandler<ClientFrame>,
) -> Element {
    let asked = use_signal(|| false);
    use_effect(move || {
        let mut asked = asked;
        if live() && !asked() {
            asked.set(true);
            on_frame.call(ClientFrame::Query(Query::RegistryView));
        }
    });
    let mut needle = use_signal(String::new);
    rsx! {
        section { class: "archive-search",
            form {
                class: "search",
                onsubmit: move |event| {
                    event.prevent_default();
                    if let Some(term) = searchable(&needle()) {
                        on_frame.call(ClientFrame::Query(Query::ArchiveSearch { needle: term }));
                    }
                },
                input {
                    r#type: "search",
                    value: "{needle}",
                    placeholder: "a word the archives may hold",
                    oninput: move |event| needle.set(event.value()),
                }
                button { r#type: "submit", disabled: searchable(&needle()).is_none(), "search the shelves" }
            }

            match hits.as_ref() {
                None => rsx! {
                    p { class: "empty", "no search yet. The shelves are read when you ask, not before." }
                },
                Some(found) => {
                    let shelves = shelves(found);
                    let total: usize = shelves.iter().map(|shelf| shelf.hits.len()).sum();
                    rsx! {
                        p { class: "note",
                            "{total} hit(s) for \"{found.needle}\" in {shelves.len()} building(s), read from the shelves just now"
                        }
                        for shelf in shelves {
                            article { key: "{shelf.building.as_str()}", class: "shelf",
                                h2 { "{shelf.building.as_str()}" }
                                for hit in shelf.hits {
                                    div { key: "{hit.day}-{hit.kind}-{hit.subject}", class: "filed",
                                        span { class: "day", "{crate::building_view::day_label(hit.day)}" }
                                        span { class: "kind", "{hit.kind}" }
                                        span { class: "subject", "{hit.subject}" }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            match filed.as_ref() {
                None => rsx! {
                    p { class: "empty", "asking the record what was filed lately" }
                },
                Some(record) if record.assets.is_empty() => rsx! {
                    p { class: "empty", "nothing has been filed yet" }
                },
                Some(record) => {
                    let rows = filed_lately(record, FILED_LATELY_MAX);
                    rsx! {
                        h2 { class: "lately", "filed lately" }
                        p { class: "note", "{filed_line(record, rows.len())}" }
                        for row in rows {
                            div { key: "{row.at.value()}-{row.subject}", class: "filed",
                                span { class: "day", "{filed_at(row.at)}" }
                                span { class: "kind", "{row.kind}" }
                                span { class: "subject", "{row.subject}" }
                                span { class: "where", "{row.addr.as_str()}" }
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
    clippy::arithmetic_side_effects,
    reason = "test code"
)]
mod tests {
    use super::*;

    fn hit(building: &str, day: u64, subject: &str) -> ArchiveHit {
        ArchiveHit {
            building: Address::parse(building).unwrap(),
            kind: "decision".to_owned(),
            day,
            subject: subject.to_owned(),
        }
    }

    fn filed(addr: &str, at: u64, subject: &str) -> RegistryLine {
        RegistryLine {
            addr: Address::parse(addr).unwrap(),
            kind: "fact".to_owned(),
            subject: subject.to_owned(),
            at: TimeMs::new(at),
        }
    }

    #[test]
    fn an_empty_needle_is_not_a_search_for_everything() {
        assert_eq!(searchable(""), None);
        assert_eq!(searchable("   "), None);
        assert_eq!(searchable("  git  "), Some("git".to_owned()));
    }

    #[test]
    fn hits_group_by_building_and_two_people_see_one_page() {
        let answer = ArchiveAnswer {
            needle: "git".to_owned(),
            hits: vec![
                hit("lab", 3, "chose git"),
                hit("mill", 9, "chose git too"),
                hit("lab", 7, "and again"),
            ],
        };
        let mut reversed = answer.clone();
        reversed.hits.reverse();
        assert_eq!(shelves(&answer), shelves(&reversed), "order in, order out");

        let grouped = shelves(&answer);
        assert_eq!(grouped.len(), 2);
        assert_eq!(grouped[0].building.as_str(), "lab");
        assert_eq!(
            grouped[0].hits.iter().map(|h| h.day).collect::<Vec<_>>(),
            vec![7, 3],
            "newest first inside a building"
        );
    }

    #[test]
    fn the_recent_list_states_its_cap_rather_than_applying_it_silently() {
        let assets: Vec<RegistryLine> = (0u64..25)
            .map(|n| filed("lab", n, &format!("thing {n}")))
            .collect();
        let answer = RegistryAnswer { assets };
        let rows = filed_lately(&answer, FILED_LATELY_MAX);
        assert_eq!(rows.len(), FILED_LATELY_MAX);
        assert_eq!(rows[0].subject, "thing 24", "newest first");
        let line = filed_line(&answer, rows.len());
        assert!(line.contains("25 filed"), "{line}");
        assert!(line.contains("5 older"), "{line}");
    }

    #[test]
    fn a_list_shorter_than_the_cap_says_nothing_about_a_cap() {
        let answer = RegistryAnswer {
            assets: vec![filed("lab", 1, "one")],
        };
        let line = filed_line(&answer, 1);
        assert!(line.contains("1 filed"), "{line}");
        assert!(!line.contains("older"), "{line}");
    }
}
