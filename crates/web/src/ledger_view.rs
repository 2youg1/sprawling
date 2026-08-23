// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Browsing the one history: filter by kind and time, jump to a stored
//! object, export what is on screen.
//!
//! This view exists because of an argument recorded against
//! itself: `live` and `dashboard` are both organised by Run, so neither can
//! answer "what has actually happened in this city". A system that claims
//! everything is replayable and then offers no way to look is asking to be
//! believed rather than checked.
//!
//! Filtering never hides a gap. A filtered list reports how many records it
//! passed over, because a window onto a history that silently omits things
//! is worse than no window.

use channels::{EventKind, EventRecord, Seq, TimeMs};
use dioxus::prelude::*;

/// What to show. Empty fields mean "no restriction" rather than "nothing",
/// which is the only reading that makes an empty filter show everything.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Filter {
    /// Kinds to keep. Empty keeps all.
    pub kinds: Vec<EventKind>,
    pub since: Option<TimeMs>,
    pub until: Option<TimeMs>,
    /// Substring match against the actor. Empty keeps all.
    pub actor: String,
}

impl Filter {
    #[must_use]
    pub fn keeps(&self, record: &EventRecord) -> bool {
        if !self.kinds.is_empty() && !self.kinds.contains(&record.kind()) {
            return false;
        }
        if self.since.is_some_and(|from| record.t() < from) {
            return false;
        }
        if self.until.is_some_and(|to| record.t() > to) {
            return false;
        }
        if !self.actor.is_empty() && !record.who().contains(&self.actor) {
            return false;
        }
        true
    }
}

/// A filtered page, and what it left out.
#[derive(Debug, Clone, PartialEq)]
pub struct Page {
    pub rows: Vec<Row>,
    /// How many records the filter rejected. Shown, never hidden.
    pub filtered_out: usize,
    /// Where the next page starts, if there is one.
    pub next_from: Option<Seq>,
}

/// One line of the browser.
#[derive(Debug, Clone, PartialEq)]
pub struct Row {
    pub seq: Seq,
    pub at: TimeMs,
    pub kind: EventKind,
    pub who: String,
    /// The stored object this row points at, if any. The jump target.
    pub locator: Option<String>,
}

/// Builds one page.
///
/// `limit` bounds the page because a Ledger has no bound: rendering a
/// million rows would freeze the tab, and an infinite stream is refused by
/// the anti-attention layout anyway.
#[must_use]
pub fn page<'a>(
    records: impl IntoIterator<Item = &'a EventRecord>,
    filter: &Filter,
    limit: usize,
) -> Page {
    let mut rows = Vec::new();
    let mut filtered_out = 0usize;
    let mut next_from = None;
    for record in records {
        if !filter.keeps(record) {
            filtered_out = filtered_out.saturating_add(1);
            continue;
        }
        if rows.len() >= limit {
            next_from = Some(record.seq());
            break;
        }
        rows.push(Row {
            seq: record.seq(),
            at: record.t(),
            kind: record.kind(),
            who: record.who().to_owned(),
            locator: None,
        });
    }
    Page {
        rows,
        filtered_out,
        next_from,
    }
}

/// How many rows one page shows.
pub const PAGE_ROWS: usize = 50;

/// Browsing what happened: filter by kind and actor, see what the filter
/// hid, and take the visible page with you.
///
/// The page states its own window. This client is fed by a broadcast that
/// starts when it connects, so what it holds is recent rather than
/// complete - and a browser onto a history has to say which part of the
/// history it is a window onto, or it is a claim rather than a window.
#[component]
pub fn LedgerView(
    records: Vec<EventRecord>,
    on_frame: EventHandler<channels::ClientFrame>,
) -> Element {
    let mut actor = use_signal(String::new);
    let mut kind = use_signal(String::new);
    // How many pages back from the newest record this page is. Counted in
    // pages rather than in sequence numbers, because a filter changes
    // which records a page holds and a stored seq would point into a
    // page that no longer exists.
    let mut back = use_signal(|| 0usize);
    let held = records.len();
    let filter = Filter {
        kinds: kind_named(&kind.read()).into_iter().collect(),
        since: None,
        until: None,
        actor: actor.read().trim().to_owned(),
    };
    let skipped = back().saturating_mul(PAGE_ROWS);
    let page = page(
        records
            .iter()
            .rev()
            .filter(|record| filter.keeps(record))
            .skip(skipped),
        &filter,
        PAGE_ROWS,
    );
    let older = page.next_from.is_some();
    let exported = export(&page);
    let filtering = !filter.actor.is_empty() || !filter.kinds.is_empty();
    rsx! {
        section { class: "ledger",
            crate::panel::Panel {
                title: if held == 0 { "the city has not said anything since this page connected".to_owned() }
                    else { "what this city has done, newest first".to_owned() },
                figure: (held > 0).then(|| held.to_string()),
                scope: "every kind of event, unless the two filters below narrow it; fifty rows to a page"
                    .to_owned(),
                source: "the live event stream since this page connected. The Ledger on disk holds the rest, and `sprawling replay` verifies the chain over all of it - including the part this page never saw."
                    .to_owned(),
                form { class: "filters", onsubmit: move |event| event.prevent_default(),
                    div { class: "field",
                        label { r#for: "ledger-actor", "who acted" }
                        input {
                            id: "ledger-actor",
                            name: "actor",
                            placeholder: "any part of a name",
                            value: "{actor}",
                            oninput: move |event| actor.set(event.value()),
                        }
                    }
                    div { class: "field",
                        label { r#for: "ledger-kind", "kind of event" }
                        select {
                            id: "ledger-kind",
                            name: "kind",
                            onchange: move |event| kind.set(event.value()),
                            option { value: "", "every kind" }
                            for name in KINDS_OFFERED {
                                option { key: "{name}", value: "{name}", "{name}" }
                            }
                        }
                    }
                    button {
                        r#type: "button",
                        class: "quiet",
                        onclick: move |_| on_frame.call(
                            channels::ClientFrame::Query(channels::Query::CityView),
                        ),
                        "refresh the city with it"
                    }
                }
                if page.filtered_out > 0 {
                    p { class: "filtered", "{page.filtered_out} record(s) hidden by this filter" }
                }
                if page.rows.is_empty() {
                    // Three states a reader cannot otherwise tell apart:
                    // the city has done nothing, the filter excluded
                    // everything, or the page has simply not been sent
                    // anything yet.
                    crate::panel::Empty {
                        status: if filtering { "no record here matches that filter".to_owned() }
                            else { "nothing has arrived since this page connected".to_owned() },
                        what: if filtering {
                            "the filter is a view over what this page holds, not over the Ledger. Widen it, or clear both fields to see everything that has arrived.".to_owned()
                        } else {
                            "every effect in this city becomes an event before it happens, so the first line appears the moment work is sent from the bar below.".to_owned()
                        },
                    }
                } else {
                    table { class: "records",
                        thead {
                            tr {
                                th { "seq" }
                                th { "at" }
                                th { "kind" }
                                th { "who" }
                            }
                        }
                        tbody {
                            for row in page.rows.clone() {
                                tr { key: "{row.seq.value()}",
                                    td { class: "seq", "{row.seq.value()}" }
                                    td { class: "at", "{row.at.value()}" }
                                    td { class: "kind", "{kind_name(row.kind)}" }
                                    td { class: "who", "{row.who}" }
                                }
                            }
                        }
                    }
                    div { class: "paging",
                        button {
                            class: "quiet",
                            disabled: back() == 0,
                            onclick: move |_| back.set(back().saturating_sub(1)),
                            "newer"
                        }
                        span { class: "where",
                            if back() == 0 {
                                "the newest {PAGE_ROWS} that match"
                            } else {
                                "{skipped} newer record(s) skipped"
                            }
                        }
                        button {
                            class: "quiet",
                            disabled: !older,
                            onclick: move |_| back.set(back().saturating_add(1)),
                            "older"
                        }
                    }
                    details { class: "export",
                        summary { "take this page" }
                        textarea { readonly: true, rows: "8", value: "{exported}" }
                    }
                }
            }
        }
    }
}

/// The kinds the filter offers by name. Not every kind: the list is the
/// ones a person looks for, and the empty choice keeps the rest reachable.
const KINDS_OFFERED: [&str; 8] = [
    "ToolCalled",
    "ToolResult",
    "ModelCalled",
    "ModelReturned",
    "GateDenied",
    "ApprovalRequested",
    "ApprovalResolved",
    "RunFrozen",
];

/// Reads a kind back from the name this view prints for it. One table
/// spells kinds in this module, and `kind_name` is the other end of it.
#[must_use]
pub fn kind_named(name: &str) -> Option<EventKind> {
    EventKind::ALL
        .into_iter()
        .find(|kind| kind_name(*kind) == name)
}

/// The kind's name for a reader. Debug rather than a kernel accessor: this
/// is a view, its export says so in its own header, and kernel's public
/// surface should not grow to spell a label.
#[must_use]
pub fn kind_name(kind: EventKind) -> String {
    format!("{kind:?}")
}

/// Renders a page as the export format: one canonical line per row, tab
/// separated.
///
/// **Not a second serialisation of the Ledger.** The Ledger's own bytes are
/// the record; this is a view of what the person filtered to, for pasting
/// into something else. It says so in its header so nobody mistakes an
/// export for an authority.
#[must_use]
pub fn export(page: &Page) -> String {
    let mut out = String::from("# sprawling ledger view - a filtered view, not the Ledger\n");
    out.push_str("seq\tt\tkind\twho\n");
    for row in &page.rows {
        out.push_str(&format!(
            "{}\t{}\t{}\t{}\n",
            row.seq.value(),
            row.at.value(),
            kind_name(row.kind),
            row.who
        ));
    }
    out
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
    use channels::{B3Hash, EventDraft, Payload, RunId};

    fn record(seq: u64, at: u64, kind: EventKind, who: &str) -> EventRecord {
        EventRecord::from_draft(
            EventDraft {
                run: RunId::from_bytes([1u8; 16]),
                t: TimeMs::new(at),
                who: who.to_owned(),
                addr: None,
                kind,
                data: Payload::empty(),
                ig: false,
            },
            Seq::new(seq),
            B3Hash::digest(b"prev"),
        )
    }

    fn stream() -> Vec<EventRecord> {
        vec![
            record(1, 10, EventKind::RunStarted, "alice"),
            record(2, 20, EventKind::ToolCalled, "alice"),
            record(3, 30, EventKind::ToolResult, "bob"),
            record(4, 40, EventKind::RunFrozen, "bob"),
        ]
    }

    #[test]
    fn an_empty_filter_shows_everything() {
        let page = page(stream().iter(), &Filter::default(), 100);
        assert_eq!(page.rows.len(), 4);
        assert_eq!(page.filtered_out, 0);
    }

    #[test]
    fn a_filtered_page_says_how_much_it_passed_over() {
        // A window that silently omits is worse than no window.
        let filter = Filter {
            kinds: vec![EventKind::ToolResult],
            ..Filter::default()
        };
        let page = page(stream().iter(), &filter, 100);
        assert_eq!(page.rows.len(), 1);
        assert_eq!(page.filtered_out, 3);
    }

    #[test]
    fn time_bounds_are_inclusive_at_both_ends() {
        let filter = Filter {
            since: Some(TimeMs::new(20)),
            until: Some(TimeMs::new(30)),
            ..Filter::default()
        };
        let page = page(stream().iter(), &filter, 100);
        let seqs: Vec<u64> = page.rows.iter().map(|r| r.seq.value()).collect();
        assert_eq!(seqs, [2, 3]);
    }

    #[test]
    fn a_page_stops_at_its_limit_and_says_where_to_resume() {
        let page = page(stream().iter(), &Filter::default(), 2);
        assert_eq!(page.rows.len(), 2);
        assert_eq!(page.next_from, Some(Seq::new(3)));
    }

    #[test]
    fn the_last_page_has_no_next() {
        let page = page(stream().iter(), &Filter::default(), 4);
        assert_eq!(page.next_from, None);
    }

    #[test]
    fn an_export_labels_itself_as_a_view() {
        let exported = export(&page(stream().iter(), &Filter::default(), 100));
        assert!(
            exported.starts_with("# sprawling ledger view"),
            "an export must not be mistaken for the Ledger"
        );
        assert!(exported.contains("not the Ledger"));
        assert_eq!(exported.lines().count(), 6, "header, columns, four rows");
    }

    #[test]
    fn filtering_by_actor_is_a_substring_because_names_carry_prefixes() {
        let filter = Filter {
            actor: "ali".to_owned(),
            ..Filter::default()
        };
        assert_eq!(page(stream().iter(), &filter, 100).rows.len(), 2);
    }
}
