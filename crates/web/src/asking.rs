// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! What this client asks the server for again, and what it keeps.
//!
//! Three questions, one module. **What a tab holds**: a bounded store of
//! records, because a tab that grows all night dies, and what falls out
//! is in the Ledger, which is the authority either way. **What has to be
//! asked again**: an event kind whose arrival makes a query's answer
//! stale, so a page that is already open catches up without polling.
//! **What was missed**: a page opened over a city that has been running
//! for a month, backfilled from history rather than left blank, since
//! the server broadcasts what happens next and never what happened.
//!
//! Each is a pure function over records the client already has, so all
//! three are tested by calling them.

use channels::{EventKind, EventRecord, RunId};

use crate::app::{RunRow, Snapshot};
use crate::phase::Phase;

/// Every run this client knows of, newest first, each with the words the
/// picker shows: its phase, and how far it has walked.
///
/// The step count comes from `web::progress`, which is where a progress
/// reading is written. Without it the picker said only
/// "running", and how much a run had actually done was a number this
/// client folded and never showed anybody.
///
/// Work that was handed down follows the run that handed it down, one
/// arrow deep, because delegation is one level deep. A person watching a
/// city where several runs are going otherwise cannot tell which run
/// answers for which.
#[must_use]
pub fn watchable(snapshot: &Snapshot) -> Vec<(RunId, String)> {
    let mut runs: Vec<(RunId, &RunRow)> = snapshot.runs().map(|(id, row)| (*id, row)).collect();
    runs.sort_by_key(|(_, row)| std::cmp::Reverse(row.started_at_seq));
    let known: std::collections::BTreeSet<RunId> = runs.iter().map(|(id, _)| *id).collect();
    let mut ordered: Vec<(RunId, &RunRow)> = Vec::with_capacity(runs.len());
    for (id, row) in &runs {
        // A child whose parent this page has not seen stands on its own
        // rather than disappearing: an orphan is still a run somebody
        // may want to watch.
        if row.parent.is_some_and(|parent| known.contains(&parent)) {
            continue;
        }
        ordered.push((*id, *row));
        for (child, child_row) in &runs {
            if child_row.parent == Some(*id) {
                ordered.push((*child, *child_row));
            }
        }
    }
    ordered
        .into_iter()
        .map(|(id, row)| {
            let walked = crate::progress::bar(
                &channels::Progress::Unplanned(channels::UnplannedProgress {
                    steps: row.steps_done,
                    // The wire carries no per-run spend, and the bar prints
                    // money only when there is some - so this reports steps
                    // and stays quiet about a figure nobody sent.
                    budget: channels::BudgetUse::default(),
                }),
                row.phase.needs_a_person(),
                crate::progress::Subject::Run,
                crate::lang::Lang::En,
            );
            // The parent's own name, taken from the same function the
            // parent's own row uses, so the two cannot disagree.
            let under = row
                .parent
                .and_then(|parent| snapshot.run(&parent).map(|up| session_of(&parent, up)));
            let name = match (&row.parent, under) {
                (None, _) => session_of(&id, row),
                (Some(_), Some(parent)) => {
                    format!("\u{21b3} {} ({parent})", session_of(&id, row))
                }
                (Some(_), None) => format!("\u{21b3} {}", session_of(&id, row)),
            };
            (
                id,
                format!(
                    "{name} \u{b7} {} \u{b7} {}",
                    crate::lang::say(crate::lang::Lang::En, row.phase.word()),
                    walked.label
                ),
            )
        })
        .collect()
}

/// The room a frame asks a run to be started in, as `building/name`.
///
/// A dispatch that named a session is asking for a room the city has not
/// opened yet; one that did not is asking for the room in the address.
/// Anything else is not a request for a run at all.
#[must_use]
pub fn room_asked_for(frame: &channels::ClientFrame) -> Option<String> {
    let channels::ClientFrame::Command(command) = frame else {
        return None;
    };
    let channels::WireCommand::Dispatch { addr, session, .. } = command.as_ref() else {
        return None;
    };
    Some(match session {
        Some(named) => format!("{}/{}", addr.as_str(), named.as_str()),
        None => addr.as_str().to_owned(),
    })
}

/// The run this record starts, when it is the run the person just asked
/// for and no other.
///
/// `expecting` is `building/name` as the client sent it. The city opens
/// exactly that room or suffixes it (`-2`), so those two spellings are
/// the whole answer; a room whose name merely begins the same way is
/// somebody else's work. Only `run_started` counts, because a later
/// event in that room would move a page the person may since have
/// navigated away from.
#[must_use]
pub fn started_here(record: &EventRecord, expecting: &str) -> Option<RunId> {
    if record.kind() != EventKind::RunStarted {
        return None;
    }
    let addr = record.addr()?.as_str();
    let mine = addr == expecting
        || addr
            .strip_prefix(expecting)
            .and_then(|rest| rest.strip_prefix('-'))
            .is_some_and(|suffix| {
                !suffix.is_empty() && suffix.chars().all(|digit| digit.is_ascii_digit())
            });
    mine.then(|| record.run())
}

/// The run a person most likely means: the one that started last, and a
/// running one ahead of a finished one.
#[must_use]
pub fn latest_run(snapshot: &Snapshot) -> Option<RunId> {
    snapshot
        .runs()
        .max_by_key(|(_, row)| (row.phase == Phase::Running, row.started_at_seq))
        .map(|(id, _)| *id)
}

/// What to call one run on screen: the session it belongs to.
///
/// The room is the session's own folder, so its last segment is the word
/// the person typed into `call it`. A run whose address this client has
/// not seen falls back to the short hash, which is worse to read and
/// still better than an empty button.
fn session_of(id: &RunId, row: &RunRow) -> String {
    row.session
        .clone()
        .unwrap_or_else(|| crate::live::short_run(*id))
}

/// Which standing answer an event makes stale.
///
/// The snapshot folds what it can model, and the rest lives in answers
/// the server computes. When an event says one of those answers has
/// changed, the client asks again - it does not try to fold the answer
/// itself, which would be a second authority for what an endpoint list
/// or a plan says.
///
/// Without this, attaching a provider left the settings page showing the
/// list from before the attach, so the model could never be chosen: the
/// three selects were still empty.
#[must_use]
pub fn invalidated_by(kind: EventKind) -> Option<channels::Query> {
    match kind {
        EventKind::EndpointAttached | EventKind::EndpointLost | EventKind::ModelSelected => {
            Some(channels::Query::EndpointView)
        }
        EventKind::BuildingCreated | EventKind::CityInitialized => Some(channels::Query::CityView),
        EventKind::ApprovalRequested | EventKind::ApprovalResolved => {
            Some(channels::Query::ApprovalQueue)
        }
        EventKind::FileDiscarded | EventKind::DiscardRestored => Some(channels::Query::DiscardView),
        EventKind::AssetArchived => Some(channels::Query::RegistryView),
        EventKind::RunFrozen => Some(channels::Query::CostView),
        _ => None,
    }
}

/// How many records the client keeps for the pages that read history.
///
/// Bounded for the same reason the live window is: a tab that grows all
/// night dies. What falls out is in the Ledger, which is the authority
/// either way - and the ledger page says so rather than implying it holds
/// everything.
pub const HELD_RECORDS: usize = 2_000;

/// Puts arriving records into the one bounded store a tab holds.
///
/// Records reach a page two ways - one at a time from the live stream,
/// and in a batch when a page that has just opened asks what happened
/// before it - and both land here, because how much history a tab holds
/// has one answer. Kept in `seq` order, one record per `seq`, never
/// more than [`HELD_RECORDS`] of them: what falls out is still in the
/// Ledger.
///
/// `reading` is the session the person currently has open, and it
/// decides **which** records go when the store is full. Age alone is the
/// wrong rule as soon as a page can ask for a session older than the
/// tab: those records sort to the front and a cap that only drops the
/// oldest drains them on the way in, so the page asks the right
/// question, receives the right answer, and still renders blank. What is
/// not being read gives way first; a session longer than the whole store
/// still gives way to itself, because the bound is the point.
pub fn hold(
    held: &mut Vec<EventRecord>,
    arriving: impl IntoIterator<Item = EventRecord>,
    reading: Option<RunId>,
) {
    held.extend(arriving);
    held.sort_by_key(EventRecord::seq);
    held.dedup_by_key(|record| record.seq());
    let mut excess = held.len().saturating_sub(HELD_RECORDS);
    if excess == 0 {
        return;
    }
    if let Some(open) = reading {
        // Oldest first, and only what belongs to some other session.
        held.retain(|record| {
            if excess == 0 || record.run() == open {
                return true;
            }
            excess = excess.saturating_sub(1);
            false
        });
    }
    // Whatever is still over the bound goes by age, which is the rule
    // when nothing is open and the last resort when something is.
    let over = held.len().saturating_sub(HELD_RECORDS);
    if over > 0 {
        held.drain(..over);
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
    use crate::app::record;

    /// Section 8-37 promises a tab has one answer to how much history
    /// it holds. That answer used to be written twice, both times
    /// inside a function only a browser could reach.
    #[test]
    fn what_a_tab_holds_has_one_answer_on_both_roads() {
        let bound = u64::try_from(HELD_RECORDS).unwrap();
        let mut held = Vec::new();

        // The live road brings one record at a time.
        hold(
            &mut held,
            [record(2, EventKind::RunStarted, [1u8; 16])],
            None,
        );
        // The backfill road brings a batch, in whatever order the
        // answer came back, overlapping what is already held.
        hold(
            &mut held,
            [
                record(3, EventKind::RunFrozen, [1u8; 16]),
                record(1, EventKind::CityInitialized, [0u8; 16]),
                record(2, EventKind::RunStarted, [1u8; 16]),
            ],
            None,
        );

        let seqs: Vec<u64> = held.iter().map(|record| record.seq().value()).collect();
        assert_eq!(seqs, vec![1, 2, 3], "one record per seq, in seq order");

        // Past the bound, the oldest are the ones that go.
        hold(
            &mut held,
            (4..=bound + 8).map(|seq| record(seq, EventKind::RunStarted, [1u8; 16])),
            None,
        );
        assert_eq!(held.len(), HELD_RECORDS, "a tab that grows all night dies");
        assert_eq!(
            held.first().map(|record| record.seq().value()),
            Some(9),
            "what fell out is the oldest, and it is still in the Ledger"
        );
        assert_eq!(
            held.last().map(|record| record.seq().value()),
            Some(bound + 8),
            "the newest record is the one a page needs most"
        );
    }

    /// Opening yesterday's session, in a tab that has been watching a
    /// busy city all day.
    ///
    /// The store was full of today's records and the answer's are older,
    /// so sorting by seq put them at the front and the cap drained them
    /// on the way in: the page asked the right question, got the right
    /// answer, and still rendered blank. Age alone cannot decide what
    /// goes - it has to be age within what the reader is looking at.
    #[test]
    fn a_session_being_read_survives_a_store_already_full_of_newer_work() {
        let old = [7u8; 16];
        let busy = [8u8; 16];
        let bound = u64::try_from(HELD_RECORDS).unwrap();
        let mut held = Vec::new();
        // A day of somebody else's work fills the tab.
        hold(
            &mut held,
            (1_000..1_000 + bound).map(|seq| record(seq, EventKind::ToolCalled, busy)),
            None,
        );
        assert_eq!(held.len(), HELD_RECORDS);

        // Yesterday's session arrives, older than everything held.
        let arriving: Vec<EventRecord> = (1..=20)
            .map(|seq| record(seq, EventKind::ToolCalled, old))
            .collect();
        hold(&mut held, arriving, Some(RunId::from_bytes(old)));

        assert_eq!(held.len(), HELD_RECORDS, "the bound still holds");
        let kept = held
            .iter()
            .filter(|record| record.run() == RunId::from_bytes(old))
            .count();
        assert_eq!(kept, 20, "the session being read is what the tab is for");
    }

    /// The session being read is preferred, never exempt. A session
    /// longer than the whole store still cannot grow the tab without
    /// end.
    #[test]
    fn even_the_session_being_read_cannot_grow_a_tab_past_its_bound() {
        let mine = [5u8; 16];
        let bound = u64::try_from(HELD_RECORDS).unwrap();
        let mut held = Vec::new();
        hold(
            &mut held,
            (1..=bound + 500).map(|seq| record(seq, EventKind::ToolCalled, mine)),
            Some(RunId::from_bytes(mine)),
        );
        assert_eq!(held.len(), HELD_RECORDS, "a tab that grows all night dies");
        assert_eq!(
            held.last().map(|record| record.seq().value()),
            Some(bound + 500),
            "and what it keeps is the end of the session"
        );
    }
}
