// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! What a page says about a snapshot, in the reader's own language.
//!
//! Every function here is a pure function of a snapshot and a language:
//! no clock, no fetch, no stored copy. Returning strings rather than
//! markup is what keeps *what* a page says testable without a renderer
//! and leaves *how it looks* to the component below.
//!
//! Two kinds live here and they are the same kind. A **reading** turns
//! counts into the four lines a status strip keeps on screen; a **name**
//! turns an id into the word a person recognises — the session they
//! called `parser` rather than a hash, the gate they are being asked
//! about rather than its code. Both are the last step before a value
//! reaches a reader, and both must be able to say it in either language.

use channels::{Tokens, UsdMicros};

use crate::app::Snapshot;
use crate::lang::Msg;
use crate::route::View;

/// The four permanent readouts, rendered as text.
///
/// A function of the snapshot and nothing else: no clock, no fetch, no
/// stored copy. Returning strings rather than markup keeps the decision of
/// *what* the status says testable without a renderer, and leaves the
/// decision of *how* it looks to the component below.
#[must_use]
pub fn status_line(lang: crate::lang::Lang, snapshot: &Snapshot) -> [String; 4] {
    [
        snapshot.city().map_or_else(
            || crate::lang::say(lang, Msg::StatusNoCity).to_owned(),
            |a| a.as_str().to_owned(),
        ),
        spend_line(lang, snapshot),
        waiting_line(lang, snapshot),
        crate::lang::fill(
            crate::lang::say(lang, Msg::StatusProvider),
            &[("state", crate::lang::say(lang, snapshot.provider().word()))],
        ),
    ]
}

/// How many things wait for a person, including the ones this build
/// cannot describe. An older client meeting a newer city says so instead
/// of showing a smaller number.
#[must_use]
pub fn waiting_line(lang: crate::lang::Lang, snapshot: &Snapshot) -> String {
    let waiting = snapshot.approvals_pending().to_string();
    match snapshot.unreadable_approvals() {
        0 => crate::lang::fill(
            crate::lang::say(lang, Msg::StatusAwaitingYou),
            &[("count", &waiting)],
        ),
        blind => crate::lang::fill(
            crate::lang::say(lang, Msg::StatusAwaitingAndUnreadable),
            &[("count", &waiting), ("blind", &blind.to_string())],
        ),
    }
}

/// What this city has spent, in the only terms it can honestly state.
///
/// A person cannot say in advance what one task is worth, and on a
/// subscription there is no unit price to say it in - so the interface
/// asks for no budget and reports afterwards (user verdict, 2026-08-22).
/// When no call carried a settled amount the line leads with tokens and
/// says why there is no figure, because `$0.00` would read as free.
#[must_use]
pub fn spend_line(lang: crate::lang::Lang, snapshot: &Snapshot) -> String {
    let usage = snapshot.usage();
    let consumed = render_tokens(Tokens::new(
        usage.input.get().saturating_add(usage.output.get()),
    ));
    let word = |msg| crate::lang::say(lang, msg);
    if usage.priced_calls == 0 {
        return if usage.unpriced_calls == 0 {
            // Not "nothing spent yet": this figure is folded from the
            // stream, which begins when the page connects, so a city that
            // spent money an hour ago would be described as having spent
            // nothing. The window is named instead of being implied.
            word(Msg::StatusNothingSpent).to_owned()
        } else {
            crate::lang::fill(word(Msg::StatusUsedNoPrice), &[("used", &consumed)])
        };
    }
    let spent = render_usd(snapshot.spent());
    if usage.unpriced_calls == 0 {
        crate::lang::fill(
            word(Msg::StatusSpent),
            &[("spent", &spent), ("used", &consumed)],
        )
    } else {
        crate::lang::fill(
            word(Msg::StatusSpentSomeUnpriced),
            &[
                ("spent", &spent),
                ("used", &consumed),
                ("calls", &usage.unpriced_calls.to_string()),
            ],
        )
    }
}

/// Renders a token count short, in integers: `48207` becomes `48.2k`.
#[must_use]
pub fn render_tokens(tokens: Tokens) -> String {
    let count = tokens.get();
    if count < 10_000 {
        return format!("{count} tokens");
    }
    let thousands = count.checked_div(1_000).unwrap_or_default();
    let tenth = count
        .checked_rem(1_000)
        .and_then(|rest| rest.checked_div(100))
        .unwrap_or_default();
    format!("{thousands}.{tenth}k tokens")
}

/// Renders micro-dollars as dollars, in integers.
///
/// No float anywhere: money is an integer count of micro-dollars end to
/// end, and converting to `f64` for display would introduce
/// the one rounding this library spent effort avoiding.
///
/// **Two decimals, or four when two would say zero about money that was
/// actually spent.** Cents are the right resolution for a total and the
/// wrong one for a single turn, which routinely bills a few thousand
/// micro-dollars: rendering that as `$0.00` is not rounding, it is the
/// interface reporting that nothing happened. One rule rather than one
/// renderer per caller - show enough digits to tell this amount apart
/// from zero, and never more than four.
#[must_use]
pub fn render_usd(amount: UsdMicros) -> String {
    let micros = amount.get();
    let dollars = micros.checked_div(1_000_000).unwrap_or_default();
    let rest = micros.checked_rem(1_000_000).unwrap_or_default();
    let cents = rest.checked_div(10_000).unwrap_or_default();
    if dollars == 0 && cents == 0 && rest > 0 {
        let ten_thousandths = rest.checked_div(100).unwrap_or_default();
        return format!("${dollars}.{ten_thousandths:04}");
    }
    format!("${dollars}.{cents:02}")
}

/// What the top bar calls the page being read.
///
/// The bar states this page, not this city: the city's name is true on
/// every page, and spending the one line that could say where you are on
/// something that never changes spends it on nothing.
#[must_use]
pub(crate) fn page_named(lang: crate::lang::Lang, view: &View) -> String {
    let word = |msg: crate::lang::Msg| crate::lang::say(lang, msg).to_owned();
    match view {
        View::Sessions | View::Run(_) => word(crate::lang::Msg::NavSessions),
        // The address itself, because it is the name a person gave the
        // work and the one thing that tells two sessions apart.
        View::Session(addr) | View::Building(addr) => addr.as_str().to_owned(),
        View::Waiting => word(crate::lang::Msg::NavWaiting),
        View::Record(_) => word(crate::lang::Msg::NavTheRecord),
        View::Cost => word(crate::lang::Msg::NavCost),
        View::Setup => word(crate::lang::Msg::NavSettings),
    }
}

/// What the city itself is doing, in one sentence at the foot of the nav.
///
/// Three states and no fourth: running, running with nothing to do, and
/// stopped. "Nothing to do" is separated from "running" because a person
/// looking at an empty table needs to know which of the two they are
/// seeing, and the two call for opposite next actions.
#[must_use]
pub(crate) fn standing_of(snapshot: &Snapshot) -> crate::lang::Msg {
    if snapshot.is_halted() {
        return crate::lang::Msg::CityStopped;
    }
    let (running, waiting, _) = snapshot.counts();
    if running == 0 && waiting == 0 {
        return crate::lang::Msg::CityRunningIdle;
    }
    crate::lang::Msg::CityRunning
}

/// The live client: it holds the snapshot the stream folds into, and
/// renders [`Root`] against it. Every judgement about the connection
/// belongs to `socket::Link`; every judgement about what an event means
/// belongs to `Snapshot::apply`. This component only holds the two
/// together and decides nothing itself.
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, reason = "test code")]
mod tests {
    use super::*;
    use channels::{ApprovalItem, EventKind, EventRecord, RunId, Seq};

    /// One request waiting for a person, as the queue delivers it.
    fn waiting_item() -> ApprovalItem {
        ApprovalItem {
            id: channels::ApprovalId::new("item-7".to_owned()).unwrap(),
            source: channels::ApprovalSource::Gate,
            actor: "urbanite-2".to_owned(),
            action_desc: "push to the remote".to_owned(),
            artifact: channels::Locator::parse(
                "file:lab/room1@0000000000000000000000000000000000000000",
            )
            .unwrap(),
            cluster_key: channels::ClusterKey {
                class: channels::ApprovalClass::AgentQuestion,
                detail: "lab".to_owned(),
            },
            created: channels::TimeMs::new(1_000),
            tainted: false,
        }
    }

    #[test]
    fn the_status_line_is_a_function_of_the_snapshot_alone() {
        let mut snapshot = Snapshot::new();
        let first = status_line(crate::lang::Lang::En, &snapshot);
        assert_eq!(
            first,
            status_line(crate::lang::Lang::En, &snapshot),
            "same input, same words"
        );
        assert!(first[0].contains("no city"));

        // The payload of an approval is the item, because that is what
        // the writer serialises; a record carrying anything else is a
        // record this client cannot read, and it says so rather than
        // counting one fewer thing waiting for a person.
        let asked = serde_json::to_value(waiting_item())
            .unwrap()
            .as_object()
            .unwrap()
            .clone();
        snapshot.apply(&EventRecord::from_draft(
            channels::EventDraft {
                run: RunId::from_bytes([8u8; 16]),
                t: channels::TimeMs::new(1),
                who: "gate".to_owned(),
                addr: None,
                kind: EventKind::ApprovalRequested,
                data: channels::Payload::new(asked).unwrap(),
                ig: false,
            },
            Seq::new(2),
            channels::B3Hash::digest(b"prev"),
        ));
        let after = status_line(crate::lang::Lang::En, &snapshot);
        assert_ne!(first, after, "and it moves when the snapshot moves");
        assert!(after[2].starts_with('1'));
    }

    #[test]
    fn money_renders_through_integers_only() {
        assert_eq!(render_usd(UsdMicros::new(0)), "$0.00");
        assert_eq!(render_usd(UsdMicros::new(1_000_000)), "$1.00");
        assert_eq!(render_usd(UsdMicros::new(1_234_567)), "$1.23");
        assert_eq!(
            render_usd(UsdMicros::new(1_230_000)),
            "$1.23",
            "truncates down"
        );
        // What one turn costs. Two decimals would report that a call
        // which spent money spent none.
        assert_eq!(render_usd(UsdMicros::new(9_999)), "$0.0099");
        assert_eq!(render_usd(UsdMicros::new(3_340)), "$0.0033");
        assert_eq!(
            render_usd(UsdMicros::new(1_003_340)),
            "$1.00",
            "a dollar and a fraction of a cent is still a dollar"
        );
        assert_eq!(render_usd(UsdMicros::new(u64::MAX)), "$18446744073709.55");
    }
}
