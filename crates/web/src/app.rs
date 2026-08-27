// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The root: what the client believes, and which region shows it.
//!
//! The view is a pure function of a snapshot. This
//! module owns the snapshot and the fold that advances it; the rendering
//! functions read it and return markup, holding nothing.
//!
//! **The snapshot is not a second history.** The Ledger is the only history
//! (ARCHITECTURE section 1); this is a disposable projection of it, the same
//! shape as `memory::hot` but living in a browser. Throwing it away and
//! folding the stream again must reach the same value, so the fold is
//! forward-only and idempotent - re-delivering a sequence number already
//! seen changes nothing, which is what makes a reconnect safe.
//!
//! Event kinds this client does not model are skipped rather than refused.
//! That is not the fail-closed rule bending: fail-closed governs decisions
//! that cause effects, and a view causes none. A server one version ahead
//! should leave the interface honest about what it does understand rather
//! than blank.

use std::collections::BTreeMap;

use channels::{Address, ApprovalItem, EventKind, EventRecord, RunId, Seq, Tokens, UsdMicros};
// The `component` macro expands to a `Props` derive and to `rsx!` internals
// that resolve against the prelude by bare name. This is the one glob import
// in the crate, and it is a requirement of the macro, not a convenience.
use crate::lang::Msg;
use dioxus::prelude::*;

/// Which central region is showing. The five regions of the layout contract
///; the top bar, left nav, right status and control
/// surface are always present and are not routes.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum View {
    /// The first screen: how much of this city is working, on what, and
    /// what is waiting on a person. It is the default because it is the
    /// only page that answers the question somebody arrives with.
    #[default]
    Overview,
    /// The isometric city, or its degenerate single-Building form at P0.
    City,
    /// One session, live. `None` is the page before a run is picked:
    /// the destination exists whether or not anything is running, and a
    /// page that says "nothing is running" is an answer, while a nav
    /// entry that vanishes is a question about where it went.
    Live(Option<RunId>),
    Approvals,
    /// What was discarded, and how each row comes back.
    RecycleBin,
    /// What the city wrote down: the shelves, and what was filed lately.
    Archive,
    Dashboard,
    Ledger,
    /// One building's own files and archive.
    Building(Address),
    /// Where a provider is registered. A region rather than a modal:
    /// registering is work, and work that can be interrupted needs a
    /// place to return to.
    Settings,
}

/// How a provider is behaving, as the right-hand status shows it. An enum
/// rather than a bool because "degraded" and "gone" call for different
/// sentences, and the interface must not invent a third state.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProviderHealth {
    #[default]
    Unknown,
    Healthy,
    Degraded,
    Lost,
}

impl ProviderHealth {
    /// The word shown to a person. Microcopy has one authority.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Unknown => "unknown",
            Self::Healthy => "healthy",
            Self::Degraded => "degraded",
            Self::Lost => "lost",
        }
    }
}

/// What a Run looks like in a list. Progress is carried as the two counts
/// the Ledger reports rather than a percentage: the ratio is the view's to
/// render, and a stored percentage would be a second place for it to be
/// wrong.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRow {
    pub addr: Option<Address>,
    /// The run that handed this work down, when one did. Folded from
    /// `run_started`, so a page and an offline replay draw the same
    /// tree.
    pub parent: Option<RunId>,
    pub phase: RunPhase,
    pub steps_done: u32,
    pub steps_planned: Option<u32>,
    pub started_at_seq: Seq,
}

/// Where a Run is. Exhaustive: an interface that cannot name a state will
/// show a blank where a person expected a word.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunPhase {
    Running,
    AwaitingApproval,
    Frozen,
    Halted,
}

impl RunPhase {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::AwaitingApproval => "awaiting approval",
            Self::Frozen => "frozen",
            Self::Halted => "halted",
        }
    }
}

/// What the model calls consumed, and what the city may not claim to know
/// about their price.
///
/// Money and tokens are separate fields because they are separately
/// knowable: every call reports tokens, and only a call whose provider or
/// price sheet settled it reports money. A subscription reports neither
/// price nor bill, and rendering that as `$0.00` would be the interface
/// inventing a fact - zero and unknown are different, and only one of
/// them is true.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Usage {
    pub input: Tokens,
    pub output: Tokens,
    pub cache_read: Tokens,
    /// Calls that came back with a settled amount.
    pub priced_calls: u32,
    /// Calls that came back with tokens and no amount.
    pub unpriced_calls: u32,
}

/// Everything the interface believes about one city.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Snapshot {
    city: Option<Address>,
    runs: BTreeMap<RunId, RunRow>,
    /// The items themselves, keyed by id: the inbox groups by cluster key
    /// and leads with the longest wait, and neither is answerable from a
    /// count. Folded from the stream; the queue query fills in what was
    /// already waiting when this client connected.
    approvals: BTreeMap<String, ApprovalItem>,
    /// Approval records this client could not read as items. Counted and
    /// shown: a page that quietly showed one fewer thing waiting would be
    /// wrong about the one fact a person is here to act on.
    unreadable_approvals: u32,
    spent: UsdMicros,
    usage: Usage,
    provider: ProviderHealth,
    /// The authorization URL of a subscription login this session began,
    /// held until the login finishes. It lives in the snapshot rather
    /// than in the page because the fact arrives as an event, and every
    /// other fact that arrives as an event is folded here.
    login_url: Option<String>,
    /// What each base URL answered when somebody asked what it serves,
    /// keyed by that URL. Held here rather than in the settings page for
    /// the reason `login_url` is: it arrives as an event, and every fact
    /// that arrives as an event is folded in one place.
    served: BTreeMap<String, Vec<String>>,
    halted: bool,
    /// How many signal events have gone by. A count, never the queue: a
    /// page that folded the queue here would be a second answer to what
    /// waits in a room, and the room's queue is the city's to state. What
    /// this is for is knowing that the answer on screen may have moved.
    signals_seen: u64,
    applied_through: Option<Seq>,
}

impl Snapshot {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// The four items the right-hand status keeps on screen at all times.
    /// The other eight `status` fields fold away, because twelve permanent
    /// readouts is a tax on attention.
    #[must_use]
    pub fn city(&self) -> Option<&Address> {
        self.city.as_ref()
    }

    #[must_use]
    pub fn spent(&self) -> UsdMicros {
        self.spent
    }

    #[must_use]
    pub fn approvals_pending(&self) -> u32 {
        u32::try_from(self.approvals.len()).unwrap_or(u32::MAX)
    }

    /// What is waiting, oldest arrival first once the inbox groups it.
    #[must_use]
    pub fn approvals(&self) -> Vec<ApprovalItem> {
        self.approvals.values().cloned().collect()
    }

    /// Names the city this client is connected to.
    ///
    /// Told by the handshake rather than folded from `city_initialized`:
    /// that record was written when the city was made, and a browser
    /// opened afterwards never sees it. The two agree, because the server
    /// reads the same record to fill the welcome.
    pub fn adopt_city(&mut self, city: Address) {
        self.city = Some(city);
    }

    /// Folds a slice of history this client was not connected for.
    ///
    /// Refused once anything live has been folded. The fold is forward
    /// only - `run_started` after `run_frozen` puts a finished run back
    /// on screen as running - so replaying older records over newer ones
    /// would move the page backwards. Saying so is better than guessing:
    /// a page that has already folded live events is not empty, which is
    /// the condition this exists to fix.
    pub fn backfill(&mut self, records: &[EventRecord]) -> Backfill {
        if self.applied_through.is_some() {
            return Backfill::AlreadyLive;
        }
        let mut folded: usize = 0;
        for record in records {
            if self.apply(record) {
                folded = folded.saturating_add(1);
            }
        }
        Backfill::Folded(folded)
    }

    /// Replaces the pending set with what the server says is pending.
    ///
    /// The stream only carries what happened after this client connected,
    /// so an item raised before that would never appear. The answer is
    /// the authority on the set; the stream advances it from there.
    pub fn adopt_approvals(&mut self, items: Vec<ApprovalItem>) {
        self.approvals = items
            .into_iter()
            .map(|item| (item.id.as_str().to_owned(), item))
            .collect();
    }

    #[must_use]
    pub fn usage(&self) -> Usage {
        self.usage
    }

    #[must_use]
    pub fn unreadable_approvals(&self) -> u32 {
        self.unreadable_approvals
    }

    #[must_use]
    pub fn provider(&self) -> ProviderHealth {
        self.provider
    }

    /// Where a person must go to finish the login this session began.
    #[must_use]
    pub fn login_url(&self) -> Option<&str> {
        self.login_url.as_deref()
    }

    /// What each probed base URL serves. The settings page ticks from
    /// this list; an empty map is a city where nobody has asked yet.
    #[must_use]
    pub fn served(&self) -> &BTreeMap<String, Vec<String>> {
        &self.served
    }

    #[must_use]
    pub fn is_halted(&self) -> bool {
        self.halted
    }

    /// How many signal events this client has folded. Not a queue length -
    /// see the field.
    #[must_use]
    pub fn signals_seen(&self) -> u64 {
        self.signals_seen
    }

    /// Where the stream should resume after a reconnect: one past the last
    /// sequence folded in.
    #[must_use]
    pub fn resume_from(&self) -> Option<Seq> {
        self.applied_through
    }

    pub fn runs(&self) -> impl Iterator<Item = (&RunId, &RunRow)> {
        self.runs.iter()
    }

    #[must_use]
    pub fn run(&self, id: &RunId) -> Option<&RunRow> {
        self.runs.get(id)
    }

    /// Folds one event forward.
    ///
    /// Returns whether the snapshot moved. An event at or before
    /// `applied_through` is dropped without effect, which is what lets a
    /// reconnect re-deliver a few frames rather than compute an exact cut.
    pub fn apply(&mut self, event: &EventRecord) -> bool {
        if let Some(seen) = self.applied_through
            && event.seq() <= seen
        {
            return false;
        }
        self.applied_through = Some(event.seq());
        self.absorb(event);
        true
    }

    fn absorb(&mut self, event: &EventRecord) {
        let run = event.run();
        match event.kind() {
            EventKind::CityInitialized => self.city = event.addr().cloned(),
            EventKind::RunStarted | EventKind::RunForked => {
                self.runs.insert(
                    run,
                    RunRow {
                        addr: event.addr().cloned(),
                        parent: event
                            .data()
                            .as_map()
                            .get("parent")
                            .and_then(serde_json::Value::as_str)
                            .and_then(|raw| RunId::parse(raw).ok()),
                        phase: RunPhase::Running,
                        steps_done: 0,
                        steps_planned: None,
                        started_at_seq: event.seq(),
                    },
                );
            }
            EventKind::ToolResult => {
                if let Some(row) = self.runs.get_mut(&run) {
                    row.steps_done = row.steps_done.saturating_add(1);
                }
            }
            EventKind::ModelReturned => {
                if let Some(row) = self.runs.get_mut(&run) {
                    row.steps_done = row.steps_done.saturating_add(1);
                }
                self.absorb_call(event);
            }
            EventKind::ApprovalRequested => {
                // The payload is the item, because that is what the writer
                // serialised. A count would not survive a reload and could
                // not be grouped; the item can do both.
                let value = serde_json::Value::Object(event.data().as_map().clone());
                match serde_json::from_value::<ApprovalItem>(value) {
                    Ok(item) => {
                        self.approvals.insert(item.id.as_str().to_owned(), item);
                    }
                    Err(_) => {
                        self.unreadable_approvals = self.unreadable_approvals.saturating_add(1);
                    }
                }
                self.set_phase(&run, RunPhase::AwaitingApproval);
            }
            EventKind::ApprovalResolved => {
                if let Some(id) = event
                    .data()
                    .as_map()
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                {
                    self.approvals.remove(id);
                }
                self.set_phase(&run, RunPhase::Running);
            }
            EventKind::RunFrozen | EventKind::BudgetLimit => {
                self.set_phase(&run, RunPhase::Frozen);
            }
            EventKind::CityHalted => {
                self.halted = true;
                for row in self.runs.values_mut() {
                    row.phase = RunPhase::Halted;
                }
            }
            // Counted, not read. What waits in a room is the city's
            // answer; this only says that answer may have moved, so the
            // page showing a room asks again.
            EventKind::SignalEnqueued | EventKind::SignalConsumed => {
                self.signals_seen = self.signals_seen.saturating_add(1);
            }
            EventKind::LoginStarted => {
                self.login_url = event
                    .data()
                    .as_map()
                    .get("auth_url")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
            }
            // The login that just produced a credential is finished, so
            // the page stops asking a person to open a URL they already
            // opened.
            EventKind::SecretCaptured => self.login_url = None,
            EventKind::ProviderDegraded => self.provider = ProviderHealth::Degraded,
            EventKind::EndpointLost => self.provider = ProviderHealth::Lost,
            EventKind::EndpointAttached => self.provider = ProviderHealth::Healthy,
            EventKind::EndpointProbed => {
                let data = event.data();
                let map = data.as_map();
                if let Some(base_url) = map.get("base_url").and_then(serde_json::Value::as_str) {
                    let models = map
                        .get("models")
                        .and_then(serde_json::Value::as_array)
                        .map(|list| {
                            list.iter()
                                .filter_map(serde_json::Value::as_str)
                                .map(str::to_owned)
                                .collect()
                        })
                        .unwrap_or_default();
                    self.served.insert(base_url.to_owned(), models);
                }
            }
            // Skipped on purpose - see the module note. A view models what
            // it can show; the Ledger keeps everything either way.
            _ => {}
        }
    }

    fn set_phase(&mut self, run: &RunId, phase: RunPhase) {
        if let Some(row) = self.runs.get_mut(run) {
            row.phase = phase;
        }
    }

    /// Folds one model call's cost and consumption.
    ///
    /// `billed_usd_micros` is written only when the provider reported an
    /// amount or the pinned price sheet could settle one. Its absence is
    /// the fact that this call has no price anybody knows, and it is kept
    /// as its own count rather than added to the total as a zero.
    fn absorb_call(&mut self, event: &EventRecord) {
        let data = event.data().as_map();
        match data
            .get("billed_usd_micros")
            .and_then(serde_json::Value::as_u64)
        {
            Some(billed) => {
                self.spent = UsdMicros::new(self.spent.get().saturating_add(billed));
                self.usage.priced_calls = self.usage.priced_calls.saturating_add(1);
            }
            None => {
                self.usage.unpriced_calls = self.usage.unpriced_calls.saturating_add(1);
            }
        }
        let Some(usage) = data.get("usage").and_then(serde_json::Value::as_object) else {
            return;
        };
        let read = |key: &str| usage.get(key).and_then(serde_json::Value::as_u64);
        let add = |held: Tokens, more: Option<u64>| {
            Tokens::new(held.get().saturating_add(more.unwrap_or_default()))
        };
        self.usage.input = add(self.usage.input, read("input_tokens"));
        self.usage.output = add(self.usage.output, read("output_tokens"));
        self.usage.cache_read = add(self.usage.cache_read, read("cache_read_tokens"));
    }
}

/// Rebuilds a snapshot from a stream. The property that matters is stated
/// as a function so tests and a future "reload from scratch" path share one
/// implementation rather than two that can disagree.
#[must_use]
pub fn rebuild<'a>(events: impl IntoIterator<Item = &'a EventRecord>) -> Snapshot {
    let mut snapshot = Snapshot::new();
    for event in events {
        snapshot.apply(event);
    }
    snapshot
}

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
            &[("state", snapshot.provider().as_str())],
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

/// One entry of the left nav.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    pub view: View,
    /// What this destination is called, in whichever language the
    /// person reads. The word itself lives in `web::lang`, so the nav
    /// and the translation cannot disagree about which page is which.
    pub label: crate::lang::Msg,
    /// How many things are waiting behind this destination, when waiting
    /// is a thing that can happen there.
    pub waiting: Option<u32>,
}

/// A heading in the left nav, and the destinations under it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NavGroup {
    pub label: crate::lang::Msg,
    pub places: Vec<Destination>,
}

/// Every destination the left nav offers, grouped by the question it
/// answers, in reading order.
///
/// One producer for the list, its wording, its grouping and its badges: a
/// destination added here appears in the nav, in the router and in the
/// test that walks them, and cannot appear in two of the three.
///
/// **The groups do not collapse.** Eight flat entries read as a menu to be
/// searched rather than a place to go, and three headings fix that; a
/// collapsed group would fix it by hiding pages, which is the same defect
/// wearing a control.
#[must_use]
pub fn destinations(snapshot: &Snapshot) -> Vec<NavGroup> {
    let waiting = snapshot.approvals_pending();
    vec![
        NavGroup {
            label: crate::lang::Msg::NavHappeningNow,
            places: vec![
                Destination {
                    view: View::Overview,
                    label: crate::lang::Msg::NavOverview,
                    waiting: None,
                },
                Destination {
                    view: View::City,
                    label: crate::lang::Msg::NavCity,
                    waiting: None,
                },
                Destination {
                    view: View::Live(latest_run(snapshot)),
                    label: crate::lang::Msg::NavLive,
                    waiting: None,
                },
                Destination {
                    view: View::Approvals,
                    label: crate::lang::Msg::NavApprovals,
                    waiting: (waiting > 0).then_some(waiting),
                },
            ],
        },
        NavGroup {
            label: crate::lang::Msg::NavTheRecord,
            places: vec![
                Destination {
                    view: View::Ledger,
                    label: crate::lang::Msg::NavLedger,
                    waiting: None,
                },
                Destination {
                    view: View::Archive,
                    label: crate::lang::Msg::NavArchive,
                    waiting: None,
                },
                Destination {
                    view: View::RecycleBin,
                    label: crate::lang::Msg::NavRecycleBin,
                    waiting: None,
                },
                Destination {
                    view: View::Dashboard,
                    label: crate::lang::Msg::NavCost,
                    waiting: None,
                },
            ],
        },
        NavGroup {
            label: crate::lang::Msg::NavSetup,
            places: vec![Destination {
                view: View::Settings,
                label: crate::lang::Msg::NavSettings,
                waiting: None,
            }],
        },
    ]
}

/// The building a person is looking at, if the city page has one
/// selected. The nav does not carry buildings - a city may have fifty -
/// so the way in is the city page, and this is what it hands over.
#[must_use]
pub fn opened_building(selected: Option<&str>) -> Option<Address> {
    selected.and_then(|name| Address::parse(name).ok())
}

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
                row.phase == RunPhase::AwaitingApproval,
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
                    row.phase.as_str(),
                    walked.label
                ),
            )
        })
        .collect()
}

/// What to call one run on screen: the session it belongs to.
///
/// The room is the session's own folder, so its last segment is the word
/// the person typed into `call it`. A run whose address this client has
/// not seen falls back to the short hash, which is worse to read and
/// still better than an empty button.
fn session_of(id: &RunId, row: &RunRow) -> String {
    let named = row.addr.as_ref().and_then(|addr| {
        addr.as_str()
            .rsplit('/')
            .next()
            .filter(|segment| !segment.is_empty())
            .map(str::to_owned)
    });
    named.unwrap_or_else(|| crate::live::short_run(*id))
}

/// The efforts this bar offers, in the order it offers them: what the
/// city already resolves, plus the one that leaves the answer to the
/// layer above.
const EFFORTS: [(&str, Msg); 6] = [
    ("", Msg::EffortInherited),
    ("low", Msg::EffortLow),
    ("medium", Msg::EffortMedium),
    ("high", Msg::EffortHigh),
    ("xhigh", Msg::EffortXHigh),
    ("max", Msg::EffortMax),
];

/// The effort a control's value names. An empty value is no choice,
/// which is not the same as `Effort::None` - that one asks a provider
/// for no reasoning at all.
#[must_use]
pub fn effort_named(value: &str) -> Option<channels::Effort> {
    match value {
        "low" => Some(channels::Effort::Low),
        "medium" => Some(channels::Effort::Medium),
        "high" => Some(channels::Effort::High),
        "xhigh" => Some(channels::Effort::XHigh),
        "max" => Some(channels::Effort::Max),
        _ => None,
    }
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
        .max_by_key(|(_, row)| (row.phase == RunPhase::Running, row.started_at_seq))
        .map(|(id, _)| *id)
}

/// What became of a backfill. Exhaustive, because "nothing was folded"
/// and "this page was already live" are different facts and only one of
/// them is worth saying anything about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backfill {
    Folded(usize),
    AlreadyLive,
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
/// more than [`HELD_RECORDS`] of them, and the oldest are the ones that
/// go: what falls out is still in the Ledger.
pub fn hold(held: &mut Vec<EventRecord>, arriving: impl IntoIterator<Item = EventRecord>) {
    held.extend(arriving);
    held.sort_by_key(EventRecord::seq);
    held.dedup_by_key(|record| record.seq());
    let excess = held.len().saturating_sub(HELD_RECORDS);
    if excess > 0 {
        held.drain(..excess);
    }
}

/// Builds one Dispatch. The only place in the client that does.
///
/// No budget travels from a person: `BudgetCap::default()` is what the
/// wire carries, and what a run costs is reported after it runs.
///
/// `session` is the word this person is calling the work by. Given one,
/// the city opens a room of that name under the building in `addr` and
/// the session has a folder nobody else writes into; left empty, `addr`
/// is the room itself and the dispatch continues what is already there.
#[must_use]
pub fn dispatch_command(
    addr: &str,
    task: &str,
    goal: &str,
    mode: &str,
    session: &str,
    effort: Option<channels::Effort>,
) -> Option<channels::ClientFrame> {
    let (task, goal) = (task.trim(), goal.trim());
    if task.is_empty() || goal.is_empty() {
        return None;
    }
    let session = match session.trim() {
        "" => None,
        named => Some(channels::SessionName::parse(named).ok()?),
    };
    let addr = Address::parse(addr.trim()).ok()?;
    Some(channels::ClientFrame::Command(Box::new(
        channels::WireCommand::Dispatch {
            idem: channels::IdemKey::derive(
                &RunId::CITY,
                Seq::FIRST,
                format!("{}|{task}", addr.as_str()).as_bytes(),
            ),
            addr,
            task: task.to_owned(),
            goal: goal.to_owned(),
            mode: channels::ModeTag::parse(mode).ok()?,
            budget: channels::BudgetCap::default(),
            session,
            effort,
        },
    )))
}

/// Renders micro-dollars as dollars and cents, in integers.
///
/// No float anywhere: money is an integer count of micro-dollars end to end
///, and converting to `f64` for display would introduce
/// the one rounding this library spent effort avoiding.
#[must_use]
pub fn render_usd(amount: UsdMicros) -> String {
    let micros = amount.get();
    let dollars = micros.checked_div(1_000_000).unwrap_or_default();
    let cents = micros
        .checked_rem(1_000_000)
        .and_then(|rest| rest.checked_div(10_000))
        .unwrap_or_default();
    format!("${dollars}.{cents:02}")
}

/// The root component: the five regions of the layout contract, and nothing
/// that decides anything. Business state is the server's; this reads a
/// snapshot handed to it.
#[component]
pub fn Root(
    snapshot: Snapshot,
    view: View,
    endpoints: Option<channels::EndpointsAnswer>,
    city: Option<channels::CityAnswer>,
    cost: Option<channels::CostAnswer>,
    building: Option<channels::BuildingAnswer>,
    discards: Option<channels::DiscardAnswer>,
    inbox: Option<channels::InboxAnswer>,
    hits: Option<channels::ArchiveAnswer>,
    filed: Option<channels::RegistryAnswer>,
    vitals: Option<channels::MetricsAnswer>,
    /// What the city last refused this person, if anything. Cleared by
    /// the person, never by the passage of time: an answer that fades
    /// before it is read is an answer nobody gave.
    refused: Option<crate::alert::Refused>,
    records: Vec<EventRecord>,
    selected: Option<String>,
    /// What a drop wrote into the control surface. Held beside
    /// `selected` because a drop aims and describes in one gesture, and
    /// the two halves must arrive together or the bar shows an address
    /// with somebody else's task under it.
    dropped: Option<String>,
    following: bool,
    /// Whether frames are flowing yet.
    ///
    /// A page asks its question when it mounts, and the first mount
    /// happens before the socket has finished its handshake - a frame
    /// sent then is dropped, by design, because a queue would be a second
    /// place where "what did the person ask for" lives. So the pages
    /// watch this instead and ask again the moment there is somebody to
    /// ask. Without it the first page a person sees says "asking the city
    /// what it holds" forever.
    live: Signal<bool>,
    on_frame: EventHandler<channels::ClientFrame>,
    on_select: EventHandler<Option<String>>,
    /// Where a gesture goes. One handler for every drop zone, so what a
    /// drag means is answered once.
    on_drop: EventHandler<(crate::drop::Target, crate::drop::Dropped)>,
    on_view: EventHandler<View>,
    on_follow: EventHandler<bool>,
    on_dismiss: EventHandler<()>,
) -> Element {
    // The language every word on this page is said in. One signal for
    // the whole tree rather than a prop through twenty components: what
    // a person reads in is a fact about the page, not about a panel.
    let lang = use_context::<Signal<crate::lang::Lang>>();
    let status = status_line(lang(), &snapshot);
    let busy = busy_buildings(&snapshot);
    let spots = destinations(&snapshot);
    let running = latest_run(&snapshot);
    let word = move |msg: crate::lang::Msg| crate::lang::say(lang(), msg);
    rsx! {
        main { class: "layout",
            header { class: "top-bar",
                span { class: "address", "{status[0]}" }
                if let Some(told) = refused.clone() {
                    div { class: "refusal", role: "alert",
                        span { class: "refusal-code", "{told.code}" }
                        span { class: "refusal-what", "{told.what}" }
                        span { class: "refusal-way", "{told.recovery}" }
                        button {
                            class: "refusal-close",
                            "aria-label": "dismiss",
                            onclick: move |_| on_dismiss.call(()),
                            "×"
                        }
                    }
                }
            }
            nav { class: "left-nav",
                for group in spots {
                    div { key: "{group.label:?}", class: "nav-group",
                        h2 { class: "nav-heading", "{word(group.label)}" }
                        for spot in group.places {
                            button {
                                key: "{spot.label:?}",
                                class: "nav-item",
                                "aria-current": if spot.view == view { "page" } else { "false" },
                                onclick: {
                                    let going = spot.view.clone();
                                    move |_| on_view.call(going.clone())
                                },
                                "{word(spot.label)}"
                                if let Some(waiting) = spot.waiting {
                                    span { class: "badge", "{waiting}" }
                                }
                            }
                        }
                    }
                }
            }
            section { class: "centre",
                match view {
                    View::Overview => rsx! {
                        crate::overview::OverviewView {
                            snapshot: snapshot.clone(),
                            city: city.clone(),
                            live,
                            on_frame,
                            on_view,
                            on_open: move |name: String| {
                                if let Some(addr) = opened_building(Some(name.as_str())) {
                                    on_view.call(View::Building(addr));
                                }
                            },
                        }
                    },
                    View::City => rsx! {
                        crate::city_view::CityView {
                            city: city.clone(),
                            busy: busy.clone(),
                            selected: selected.clone(),
                            live,
                            on_frame,
                            on_select,
                            on_open: move |name: String| {
                                if let Some(addr) = opened_building(Some(name.as_str())) {
                                    on_view.call(View::Building(addr));
                                }
                            },
                        }
                    },
                    View::Live(run) => rsx! {
                        crate::live::LiveView {
                            feed: crate::live::Feed::replay(records.iter(), run, following),
                            run,
                            runs: watchable(&snapshot),
                            following,
                            on_frame,
                            on_follow,
                            on_watch: move |id| on_view.call(View::Live(id)),
                        }
                    },
                    View::Approvals => rsx! {
                        crate::approval::ApprovalsView {
                            items: snapshot.approvals(),
                            live,
                            on_frame,
                        }
                    },
                    View::RecycleBin => rsx! {
                        crate::approval::RecycleBinView {
                            answer: discards.clone(),
                            live,
                            on_frame,
                        }
                    },
                    View::Archive => rsx! {
                        crate::archive_search::ArchiveView {
                            hits: hits.clone(),
                            filed: filed.clone(),
                            live,
                            on_frame,
                        }
                    },
                    View::Dashboard => rsx! {
                        crate::dashboard::CostsView {
                            answer: cost.clone(),
                            usage: snapshot.usage(),
                            spent: snapshot.spent(),
                            live,
                            on_frame,
                        }
                    },
                    View::Ledger => rsx! {
                        crate::ledger_view::LedgerView {
                            records: records.clone(),
                            on_frame,
                        }
                    },
                    View::Building(ref addr) => rsx! {
                        crate::building_view::BuildingView {
                            addr: addr.clone(),
                            answer: building.clone(),
                            inbox: inbox.clone(),
                            signals: snapshot.signals_seen(),
                            live,
                            on_frame,
                            on_select,
                            on_drop,
                        }
                    },
                    View::Settings => rsx! {
                        crate::settings::Settings {
                            answer: endpoints.clone(),
                            login_url: snapshot.login_url().map(str::to_owned),
                            served: snapshot.served().clone(),
                            live,
                            on_frame,
                        }
                    },
                }
            }
            aside { class: "right-status",
                for item in status.iter().skip(1) {
                    p { key: "{item}", class: "standing", "{item}" }
                }
                // The three counts no page states. They stand here rather
                // than above one page's heading: they are facts about the
                // city, they are true on every page, and the right-hand
                // column is where facts that outlive a page belong.
                crate::vitals::Vitals { answer: vitals.clone(), live, on_frame }
            }
            footer { class: "control-surface",
                DispatchBar { addr: selected.clone(), dropped: dropped.clone(), on_frame, on_view }
                button {
                    class: "halt",
                    onclick: move |_| on_frame.call(halt_command(!snapshot.is_halted())),
                    if snapshot.is_halted() {
                        "{word(crate::lang::Msg::ReleaseCity)}"
                    } else {
                        "{word(crate::lang::Msg::HaltCity)}"
                    }
                }
                if let Some(run) = running {
                    button {
                        class: "cancel",
                        onclick: move |_| on_frame.call(cancel_command(run)),
                        "{word(crate::lang::Msg::CancelLastRun)}"
                    }
                }
            }
        }
    }
}

/// The control surface's one form: where work is started.
///
/// It asks for an address, a task, a goal and a mode - and for nothing
/// about money. The four it asks for are the four a Run cannot be started
/// without; a budget is not one of them, because the person typing here
/// has no way to know the number and, on a subscription, neither has
/// anybody else.
#[component]
fn DispatchBar(
    addr: Option<String>,
    /// A task line a drop wrote. It fills the box the person would have
    /// typed into and stops there: a gesture that also pressed the
    /// button would spend money nobody agreed to spend.
    dropped: Option<String>,
    on_frame: EventHandler<channels::ClientFrame>,
    /// Where the person is taken once the frame is away. A control that
    /// leaves the page exactly as it found it cannot be told apart from
    /// one that did nothing.
    on_view: EventHandler<View>,
) -> Element {
    let mut at = use_signal(|| addr.clone().unwrap_or_default());
    // The bar follows what the person selected on the page above it: a
    // building chosen in the city or opened on its own page arrives
    // here. Reactive, because a `use_effect` that only closed over the
    // first render's prop would leave the field showing whatever was
    // selected when the window opened (web-SPEC.md section 8-17).
    let chosen = addr.clone();
    use_effect(use_reactive!(|chosen| {
        let mut at = at;
        if let Some(ref selected) = chosen
            && !selected.is_empty()
        {
            at.set(selected.clone());
        }
    }));
    let mut task = use_signal(String::new);
    // Reactive for the same reason the address is: a line written by a
    // drop after the first render would otherwise never reach the box.
    let written = dropped.clone();
    use_effect(use_reactive!(|written| {
        let mut task = task;
        if let Some(ref line) = written
            && !line.is_empty()
        {
            task.set(line.clone());
        }
    }));
    let mut goal = use_signal(String::new);
    let mut mode = use_signal(|| "plan".to_owned());
    let mut session = use_signal(String::new);
    // How hard this session thinks. Beside the send button because a
    // person chooses it once for the session they are starting, and
    // because the layers above answer when they do not (user verdict,
    // 2026-08-24).
    let mut effort = use_signal(|| None::<channels::Effort>);
    let lang = use_context::<Signal<crate::lang::Lang>>();
    let word = move |msg: Msg| crate::lang::say(lang(), msg);
    rsx! {
        form {
            class: "dispatch",
            onsubmit: move |event| {
                event.prevent_default();
                let frame = dispatch_command(
                    &at.read(),
                    &task.read(),
                    &goal.read(),
                    &mode.read(),
                    &session.read(),
                    *effort.read(),
                );
                if let Some(frame) = frame {
                    on_frame.call(frame);
                    task.set(String::new());
                    goal.set(String::new());
                    session.set(String::new());
                    // The run this frame starts reports itself on the
                    // live page, so that is where the person who sent it
                    // belongs. Which session they then watch stays their
                    // choice (web-SPEC.md section 8-31).
                    on_view.call(View::Live(None));
                }
            },
            div { class: "field",
                label { r#for: "dispatch-addr", "{word(Msg::DispatchRoom)}" }
                input {
                    id: "dispatch-addr",
                    name: "addr",
                    // Short, because the label above already said what
                    // the field is: a placeholder wider than its column
                    // teaches half a format.
                    placeholder: "{word(Msg::DispatchRoomHint)}",
                    value: "{at}",
                    oninput: move |event| at.set(event.value()),
                }
            }
            div { class: "field",
                label { r#for: "dispatch-session", "{word(Msg::DispatchCallIt)}" }
                input {
                    id: "dispatch-session",
                    name: "session",
                    placeholder: "{word(Msg::DispatchCallItHint)}",
                    value: "{session}",
                    oninput: move |event| session.set(event.value()),
                }
            }
            div { class: "field",
                label { r#for: "dispatch-task", "{word(Msg::DispatchTask)}" }
                input {
                    id: "dispatch-task",
                    name: "task",
                    placeholder: "{word(Msg::DispatchTaskHint)}",
                    value: "{task}",
                    oninput: move |event| task.set(event.value()),
                }
            }
            div { class: "field",
                label { r#for: "dispatch-goal", "{word(Msg::DispatchDoneWhen)}" }
                input {
                    id: "dispatch-goal",
                    name: "goal",
                    placeholder: "{word(Msg::DispatchDoneWhenHint)}",
                    value: "{goal}",
                    oninput: move |event| goal.set(event.value()),
                }
            }
            div { class: "field",
                label { r#for: "dispatch-mode", "{word(Msg::DispatchMode)}" }
                select {
                    id: "dispatch-mode",
                    name: "mode",
                    onchange: move |event| mode.set(event.value()),
                    option { value: "plan", "plan" }
                    option { value: "build", "build" }
                    option { value: "review", "review" }
                }
            }
            div { class: "field",
                label { r#for: "dispatch-effort", "{word(Msg::DispatchEffort)}" }
                select {
                    id: "dispatch-effort",
                    name: "effort",
                    onchange: move |event| effort.set(effort_named(&event.value())),
                    for offered in EFFORTS {
                        option {
                            key: "{offered.0}",
                            value: "{offered.0}",
                            "{word(offered.1)}"
                        }
                    }
                }
            }
            button {
                r#type: "submit",
                disabled: dispatch_command(
                        &at.read(),
                        &task.read(),
                        &goal.read(),
                        &mode.read(),
                        &session.read(),
                        *effort.read(),
                    )
                    .is_none(),
                "{word(Msg::DispatchSend)}"
            }
        }
    }
}

/// Stopping and releasing the whole city, as one control with two states.
#[must_use]
fn halt_command(halting: bool) -> channels::ClientFrame {
    let scope = channels::HaltScope::City;
    let idem =
        channels::IdemKey::derive(&RunId::CITY, Seq::FIRST, b"halt-from-the-control-surface");
    channels::ClientFrame::Command(Box::new(if halting {
        channels::WireCommand::Halt { scope, idem }
    } else {
        channels::WireCommand::Release { scope, idem }
    }))
}

#[must_use]
fn cancel_command(run: RunId) -> channels::ClientFrame {
    channels::ClientFrame::Command(Box::new(channels::WireCommand::Cancel {
        idem: channels::IdemKey::derive(&run, Seq::FIRST, b"cancel-from-the-control-surface"),
        run,
    }))
}

/// The live client: it holds the snapshot the stream folds into, and
/// renders [`Root`] against it. Every judgement about the connection
/// belongs to `socket::Link`; every judgement about what an event means
/// belongs to `Snapshot::apply`. This component only holds the two
/// together and decides nothing itself.
#[component]
pub fn App() -> Element {
    // Provided before anything renders, because every component below
    // reads it. The first value is the browser's own setting: a person
    // whose machine is in Chinese should not have to find a switch to
    // be spoken to in Chinese.
    use_context_provider(|| Signal::new(crate::lang::preferred()));
    // Read back rather than kept from the line above: the signal is the
    // one authority for what language this page reads in, and the
    // listeners below say their words when they fire, not when they are
    // registered - so a person who switches language mid-session is
    // answered in the new one.
    let lang = use_context::<Signal<crate::lang::Lang>>();
    let snapshot = use_signal(Snapshot::new);
    #[cfg_attr(
        target_arch = "wasm32",
        expect(
            unused_mut,
            reason = "in a browser the address bar moves the signal, not this handle"
        )
    )]
    let mut view = use_signal(View::default);
    // The address bar is the authority for which page is showing, and
    // the listener below is the only thing that moves the signal. A
    // click writes the fragment and hears its own change back, so a
    // click and the browser's back button travel the same path and
    // cannot disagree about where the person is.
    let endpoints = use_signal(|| None::<channels::EndpointsAnswer>);
    let city = use_signal(|| None::<channels::CityAnswer>);
    let cost = use_signal(|| None::<channels::CostAnswer>);
    let building = use_signal(|| None::<channels::BuildingAnswer>);
    let discards = use_signal(|| None::<channels::DiscardAnswer>);
    let inbox = use_signal(|| None::<channels::InboxAnswer>);
    let hits = use_signal(|| None::<channels::ArchiveAnswer>);
    let filed = use_signal(|| None::<channels::RegistryAnswer>);
    let vitals = use_signal(|| None::<channels::MetricsAnswer>);
    let records = use_signal(Vec::<EventRecord>::new);
    let mut refused = use_signal(|| None::<crate::alert::Refused>);
    // The address bar is the authority for which page is showing, and the
    // listener below is the only thing that moves the signal, so a click
    // and the browser's back button travel one path and cannot disagree
    // about where the person is. A fragment this build cannot resolve
    // becomes a refusal rather than a silent landing on the first page.
    follow_the_address_bar(view, refused, lang);
    let mut selected = use_signal(|| None::<String>);
    let mut dropped = use_signal(|| None::<String>);
    let mut following = use_signal(|| true);
    let live = use_signal(|| false);
    // The room the last dispatch asked for, so its run can be opened
    // when it starts rather than left for the person to find among the
    // others.
    let mut expecting = use_signal(|| None::<String>);
    #[cfg(target_arch = "wasm32")]
    let outbound = connect(Wiring {
        snapshot,
        endpoints,
        city,
        cost,
        building,
        discards,
        inbox,
        hits,
        filed,
        vitals,
        records,
        live,
        view,
        expecting,
        refused,
        lang,
    });
    #[cfg(not(target_arch = "wasm32"))]
    let outbound = Outbound;
    rsx! {
        Root {
            snapshot: snapshot(),
            view: view(),
            endpoints: endpoints(),
            city: city(),
            cost: cost(),
            building: building(),
            discards: discards(),
            inbox: inbox(),
            hits: hits(),
            filed: filed(),
            vitals: vitals(),
            refused: refused(),
            records: records(),
            selected: selected(),
            dropped: dropped(),
            following: following(),
            live,
            on_frame: move |frame: channels::ClientFrame| {
                if let Some(room) = room_asked_for(&frame) {
                    expecting.set(Some(room));
                }
                outbound.call(frame);
            },
            on_select: move |id| selected.set(id),
            // One place answers what a drag meant, whichever zone it
            // landed on. A refusal takes the same route every other
            // refusal takes, so a gesture with no meaning reads like
            // everything else the city would not do.
            on_drop: move |(target, what): (crate::drop::Target, crate::drop::Dropped)| {
                match crate::drop::read(&target, &what) {
                    crate::drop::Meaning::Aim { addr, task } => {
                        selected.set(Some(addr.as_str().to_owned()));
                        dropped.set(Some(task));
                    }
                    crate::drop::Meaning::Refused { because } => {
                        refused.set(Some(crate::alert::refused(
                            lang(),
                            &crate::drop::refusal(lang(), because),
                        )));
                    }
                }
            },
            on_view: move |next: View| {
                #[cfg(target_arch = "wasm32")]
                crate::route::go(&next);
                #[cfg(not(target_arch = "wasm32"))]
                view.set(next);
            },
            on_follow: move |on| following.set(on),
            on_dismiss: move |()| refused.set(None),
        }
    }
}

/// Which buildings have a run in flight, folded from the snapshot rather
/// than asked of the server: the event stream already says it, and a
/// second question would be a second answer.
#[must_use]
fn busy_buildings(snapshot: &Snapshot) -> std::collections::BTreeSet<Address> {
    snapshot
        .runs()
        .filter(|(_, row)| matches!(row.phase, RunPhase::Running | RunPhase::AwaitingApproval))
        .filter_map(|(_, row)| row.addr.clone())
        .filter_map(|addr| building_of(&addr))
        .collect()
}

/// The building an address belongs to: its first segment. The city keeps
/// the authority on that (a building is a top-level address); this is the
/// same rule read on the page, so a run in `lab/room1` lights `lab`.
fn building_of(addr: &Address) -> Option<Address> {
    let head = addr.as_str().split('/').next()?;
    Address::parse(head).ok()
}

/// Opens the socket once per mount and folds what arrives into the
/// snapshot. A frame that the link turns into anything other than an
/// event changes no business state: refusals are shown, answers reach
/// the view that asked, and neither is history.
/// Every place an arriving frame may land.
///
/// One parameter rather than a growing argument list: each field is a
/// standing answer some page asked for, and adding a page should be
/// adding a field here and an arm below, not rewriting a signature.
#[cfg(target_arch = "wasm32")]
struct Wiring {
    snapshot: Signal<Snapshot>,
    endpoints: Signal<Option<channels::EndpointsAnswer>>,
    city: Signal<Option<channels::CityAnswer>>,
    cost: Signal<Option<channels::CostAnswer>>,
    building: Signal<Option<channels::BuildingAnswer>>,
    discards: Signal<Option<channels::DiscardAnswer>>,
    inbox: Signal<Option<channels::InboxAnswer>>,
    hits: Signal<Option<channels::ArchiveAnswer>>,
    filed: Signal<Option<channels::RegistryAnswer>>,
    vitals: Signal<Option<channels::MetricsAnswer>>,
    records: Signal<Vec<EventRecord>>,
    live: Signal<bool>,
    /// Which page is showing, so the run a person just asked for can be
    /// opened when it starts.
    view: Signal<View>,
    /// The room this client last dispatched to, as `building/name`.
    /// Cleared by the run it was waiting for; see [`started_here`].
    expecting: Signal<Option<String>>,
    /// The last thing the city refused. Beside the snapshot rather than
    /// inside it: a refusal is not something that happened to the city,
    /// it is the answer to something one person asked, and the snapshot
    /// holds only what the ledger says.
    refused: Signal<Option<crate::alert::Refused>>,
    /// What language the words this wiring produces are said in. A
    /// signal rather than a value: these closures speak long after the
    /// page mounted.
    lang: Signal<crate::lang::Lang>,
}

/// Mounts the one reader of the address bar.
///
/// Registered once for the life of the page: `use_hook` runs on the
/// first render only, so the listener is not rebuilt on every state
/// change - a second listener would apply the same change twice.
#[cfg(target_arch = "wasm32")]
fn follow_the_address_bar(
    mut view: Signal<View>,
    mut refused: Signal<Option<crate::alert::Refused>>,
    lang: Signal<crate::lang::Lang>,
) {
    use dioxus::prelude::use_hook;
    use wasm_bindgen::JsCast as _;
    use_hook(move || {
        // What the person arrived at, before any event has happened.
        match crate::route::current() {
            Some(arrived) => view.set(arrived),
            // A link that does not land is a fact the person may want to
            // act on. Leaving them on the first page without a word is the
            // quiet substitution this design refuses: it teaches somebody
            // their own bookmarks are unreliable while never admitting it.
            None => {
                if let Some(named) = crate::route::unresolved() {
                    let said = lang();
                    refused.set(Some(crate::alert::Refused {
                        code: "E_NO_SUCH_PAGE".to_owned(),
                        what: crate::lang::fill(
                            crate::lang::say(said, Msg::RouteNoSuchPage),
                            &[("named", &named)],
                        ),
                        recovery: crate::lang::say(said, Msg::RouteNoSuchPageRecovery).to_owned(),
                    }));
                }
            }
        }
        let Some(window) = web_sys::window() else {
            return;
        };
        let moved = wasm_bindgen::closure::Closure::<dyn FnMut()>::new(move || {
            // A fragment that names nothing leaves the page where it is
            // rather than landing somewhere the person did not ask for.
            if let Some(next) = crate::route::current() {
                view.set(next);
            }
        });
        if window
            .add_event_listener_with_callback("hashchange", moved.as_ref().unchecked_ref())
            .is_ok()
        {
            // The listener outlives this scope, and the page outlives
            // the listener: dropping the closure here would unregister
            // the only thing that reads the address bar.
            moved.forget();
        }
    });
}

/// Off the browser there is no address bar, so the signal is the only
/// authority and nothing has to follow anything.
#[cfg(not(target_arch = "wasm32"))]
fn follow_the_address_bar(
    _view: Signal<View>,
    _refused: Signal<Option<crate::alert::Refused>>,
    _lang: Signal<crate::lang::Lang>,
) {
}

#[cfg(target_arch = "wasm32")]
fn connect(wiring: Wiring) -> Outbound {
    use dioxus::prelude::use_hook;

    // Only two of these move here now. The rest are copied into the
    // frame's own wiring and moved once per animation frame instead of
    // once per arriving message, which is the whole of what this loop
    // changed (`web::pace`).
    let Wiring {
        mut snapshot,
        endpoints,
        city,
        cost,
        building,
        discards,
        inbox,
        hits,
        filed,
        vitals,
        records,
        mut live,
        view,
        expecting,
        refused,
        lang,
    } = wiring;
    use_hook(move || {
        let outbound = std::rc::Rc::new(std::cell::RefCell::new(None));
        let Some(url) = crate::socket::socket_url() else {
            return send_through(outbound);
        };
        // The code the host put on this URL. Hard-coded `None` here made
        // every exposed city unreachable by its own WebUI: the server
        // asked for a token and the page had no way to have one.
        let link = std::rc::Rc::new(std::cell::RefCell::new(crate::socket::Link::new(
            crate::socket::pairing_token(),
        )));
        // What has already claimed somebody's attention. Held beside the
        // link because a reconnect re-delivers events, and one fact must
        // not interrupt twice for having been sent twice.
        let alerts = std::rc::Rc::new(std::cell::RefCell::new(crate::alert::Alerts::new()));
        let socket = std::rc::Rc::new(std::cell::RefCell::new(None));
        // Where frames wait for the next animation frame. A run does not
        // deliver one event at a time - a tool wave writes five in a few
        // milliseconds - and applying each on arrival repainted the page
        // once per event at whatever rate the network chose. A display
        // cannot show more than one frame per refresh, so those paints
        // were work produced for nobody (`web::pace`).
        let buffer = crate::pace::browser::Buffer::default();
        {
            let buffer_for_loop = buffer.clone();
            let alerts = std::rc::Rc::clone(&alerts);
            let socket = std::rc::Rc::clone(&socket);
            crate::pace::browser::each_frame(buffer_for_loop, move |paint| {
                apply_frame(
                    paint,
                    &socket,
                    &alerts,
                    FrameWiring {
                        snapshot,
                        endpoints,
                        city,
                        cost,
                        building,
                        discards,
                        inbox,
                        hits,
                        filed,
                        vitals,
                        records,
                        view,
                        expecting,
                        refused,
                        lang,
                    },
                );
            });
        }
        let opened = {
            let link = std::rc::Rc::clone(&link);
            let socket = std::rc::Rc::clone(&socket);
            let buffer = buffer.clone();
            crate::socket::open(&url, move |event| {
                let action = match link.try_borrow_mut() {
                    Ok(mut link) => link.advance(event),
                    Err(_) => return,
                };
                // The pages watch this to know when asking is worth
                // anything. Read from the link rather than inferred from
                // the action, because the link owns what "live" means.
                let flowing = link.try_borrow().is_ok_and(|link| link.is_live());
                let opened = flowing && !*live.peek();
                if *live.peek() != flowing {
                    live.set(flowing);
                }
                if let Ok(link) = link.try_borrow()
                    && let Some(city) = link.city()
                    && snapshot.peek().city() != Some(city)
                {
                    snapshot.write().adopt_city(city.clone());
                }
                let held = socket.borrow();
                let Some(socket) = held.as_ref() else {
                    return;
                };
                // The stream carries what happens next, so a tab opened
                // over a city that has been running for a month saw an
                // empty one. Asked once, the moment frames start
                // flowing, and before any live record can have been
                // folded - which is the condition `backfill` refuses to
                // work without.
                if opened {
                    let _ = crate::socket::send(
                        socket,
                        &channels::ClientFrame::Query(channels::Query::History {
                            before: None,
                            limit: channels::HISTORY_MAX,
                        }),
                    );
                }
                match action {
                    crate::socket::LinkAction::Send(hello) => {
                        let _ = crate::socket::send(socket, &channels::ClientFrame::Hello(*hello));
                    }
                    // The three actions that change the page do not change
                    // it here. They go into the buffer and the animation
                    // frame applies them together, because the rate a
                    // network delivers at is not a rate a display can show
                    // (`web::pace`).
                    crate::socket::LinkAction::Deliver(event) => {
                        buffer.push(crate::pace::Arrived::Event(event));
                    }
                    crate::socket::LinkAction::Answered(answer) => {
                        buffer.push(crate::pace::Arrived::Answer(answer));
                    }
                    crate::socket::LinkAction::Report(error) => {
                        buffer.push(crate::pace::Arrived::Refusal(error));
                    }
                    // The retry ladder is not history either, and
                    // closing on the way out of view is the transport
                    // layer's to carry out; here they are the same as
                    // any other instruction that moves no snapshot.
                    crate::socket::LinkAction::WaitMs(_)
                    | crate::socket::LinkAction::OpenSocket
                    | crate::socket::LinkAction::CloseSocket
                    | crate::socket::LinkAction::Nothing => {}
                }
            })
        };
        if let Ok(handle) = opened {
            *socket.borrow_mut() = Some(handle);
            let _ = link.borrow_mut().connect();
        }
        *outbound.borrow_mut() = Some(std::rc::Rc::clone(&socket));
        send_through(outbound)
    })
}

/// The one way a component reaches the server. A frame sent before the
/// socket exists is dropped rather than queued: the page that sent it
/// asks again, and a queue would be a second place where "what did the
/// person ask for" lives.
#[cfg(target_arch = "wasm32")]
fn send_through(outbound: OutboundCell) -> Outbound {
    Outbound(outbound)
}

/// The socket handle as the component tree may hold it: cloneable,
/// because a hook's value is cloned on every render.
#[cfg(target_arch = "wasm32")]
type OutboundCell = std::rc::Rc<
    std::cell::RefCell<Option<std::rc::Rc<std::cell::RefCell<Option<web_sys::WebSocket>>>>>,
>;

#[cfg(target_arch = "wasm32")]
#[derive(Clone)]
pub struct Outbound(OutboundCell);

#[cfg(target_arch = "wasm32")]
impl Outbound {
    fn call(&self, frame: channels::ClientFrame) {
        let held = self.0.borrow();
        let Some(socket) = held.as_ref() else {
            return;
        };
        let socket = socket.borrow();
        if let Some(socket) = socket.as_ref() {
            let _ = crate::socket::send(socket, &frame);
        }
    }
}

/// Off the browser there is no socket, so a frame goes nowhere. The
/// type exists on both targets because the component tree names it.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Clone)]
pub struct Outbound;

#[cfg(not(target_arch = "wasm32"))]
impl Outbound {
    fn call(&self, _frame: channels::ClientFrame) {}
}

/// Hands the client to the browser. The only wasm-specific entry in this
/// crate, and it decides nothing.
#[cfg(target_arch = "wasm32")]
#[wasm_bindgen::prelude::wasm_bindgen(start)]
pub fn start() {
    install_theme();
    crate::alert::ask_to_interrupt();
    launch(App);
}

/// Writes the token set into the document before the first paint.
///
/// The shipped page names no colour; it reads custom properties that arrive
/// here. That is what makes "one production point for colour" true of what
/// the browser renders and not only of the Rust source.
#[cfg(target_arch = "wasm32")]
fn install_theme() {
    let Some(document) = web_sys::window().and_then(|w| w.document()) else {
        return;
    };
    let Some(head) = document.head() else {
        return;
    };
    let Ok(style) = document.create_element("style") else {
        return;
    };
    style.set_text_content(Some(&crate::theme::custom_properties()));
    let _ = head.append_child(&style);
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
    use channels::{B3Hash, Payload};

    fn record(seq: u64, kind: EventKind, run: [u8; 16]) -> EventRecord {
        EventRecord::from_draft(
            kernel_draft(kind, run),
            Seq::new(seq),
            B3Hash::digest(b"prev"),
        )
    }

    fn kernel_draft(kind: EventKind, run: [u8; 16]) -> channels::EventDraft {
        channels::EventDraft {
            run: RunId::from_bytes(run),
            t: channels::TimeMs::new(1_000),
            who: "test".to_owned(),
            addr: None,
            kind,
            data: Payload::empty(),
            ig: false,
        }
    }

    /// Section 8-37 promises a tab has one answer to how much history
    /// it holds. That answer used to be written twice, both times
    /// inside a function only a browser could reach.
    #[test]
    fn what_a_tab_holds_has_one_answer_on_both_roads() {
        let bound = u64::try_from(HELD_RECORDS).unwrap();
        let mut held = Vec::new();

        // The live road brings one record at a time.
        hold(&mut held, [record(2, EventKind::RunStarted, [1u8; 16])]);
        // The backfill road brings a batch, in whatever order the
        // answer came back, overlapping what is already held.
        hold(
            &mut held,
            [
                record(3, EventKind::RunFrozen, [1u8; 16]),
                record(1, EventKind::CityInitialized, [0u8; 16]),
                record(2, EventKind::RunStarted, [1u8; 16]),
            ],
        );

        let seqs: Vec<u64> = held.iter().map(|record| record.seq().value()).collect();
        assert_eq!(seqs, vec![1, 2, 3], "one record per seq, in seq order");

        // Past the bound, the oldest are the ones that go.
        hold(
            &mut held,
            (4..=bound + 8).map(|seq| record(seq, EventKind::RunStarted, [1u8; 16])),
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

    #[test]
    fn folding_the_same_stream_twice_reaches_the_same_snapshot() {
        // The snapshot is a projection, not a history: discard it, fold
        // again, land on the same value (ARCHITECTURE section 1).
        let stream = [
            record(1, EventKind::RunStarted, [1u8; 16]),
            record(2, EventKind::ToolResult, [1u8; 16]),
            record(3, EventKind::ApprovalRequested, [1u8; 16]),
            record(4, EventKind::ApprovalResolved, [1u8; 16]),
        ];
        let once = rebuild(stream.iter());
        let twice = rebuild(stream.iter());
        assert_eq!(once, twice);
    }

    #[test]
    fn redelivering_a_frame_after_a_reconnect_changes_nothing() {
        let mut snapshot = Snapshot::new();
        let started = record(1, EventKind::RunStarted, [2u8; 16]);
        let stepped = record(2, EventKind::ToolResult, [2u8; 16]);
        assert!(snapshot.apply(&started));
        assert!(snapshot.apply(&stepped));
        let after_first_pass = snapshot.clone();

        // A reconnect resumes a little early rather than computing an exact
        // cut; the overlap must be free.
        assert!(!snapshot.apply(&started));
        assert!(!snapshot.apply(&stepped));
        assert_eq!(snapshot, after_first_pass);
        assert_eq!(
            snapshot
                .run(&RunId::from_bytes([2u8; 16]))
                .unwrap()
                .steps_done,
            1
        );
    }

    #[test]
    fn halting_the_city_marks_every_run_not_just_the_one_that_carried_it() {
        let mut snapshot = Snapshot::new();
        snapshot.apply(&record(1, EventKind::RunStarted, [3u8; 16]));
        snapshot.apply(&record(2, EventKind::RunStarted, [4u8; 16]));
        snapshot.apply(&record(3, EventKind::CityHalted, [3u8; 16]));
        assert!(snapshot.is_halted());
        for (_, row) in snapshot.runs() {
            assert_eq!(row.phase, RunPhase::Halted);
        }
    }

    #[test]
    fn a_pending_count_never_goes_below_nothing() {
        // A client that joined mid-stream can see a resolution whose request
        // it never saw. Saturating is the honest answer; wrapping would
        // print four billion pending approvals.
        let mut snapshot = Snapshot::new();
        snapshot.apply(&record(1, EventKind::ApprovalResolved, [5u8; 16]));
        assert_eq!(snapshot.approvals_pending(), 0);
    }

    /// A tab opened over a city that has been running for a month used
    /// to show an empty one: the server broadcasts what happens next and
    /// never what happened.
    #[test]
    fn a_page_folds_the_history_it_was_not_connected_for() {
        let mut snapshot = Snapshot::new();
        let history = [
            record(1, EventKind::RunStarted, [1u8; 16]),
            record(2, EventKind::ModelReturned, [1u8; 16]),
        ];
        assert_eq!(snapshot.backfill(&history), Backfill::Folded(2));
        assert_eq!(snapshot.runs().count(), 1);
        assert_eq!(snapshot.resume_from(), Some(Seq::new(2)));

        // And the live stream continues from there rather than being
        // refused as a duplicate.
        assert!(snapshot.apply(&record(3, EventKind::RunFrozen, [1u8; 16])));
    }

    /// The fold is forward only, so replaying older records over newer
    /// ones would put a finished run back on screen as running. A page
    /// that has already folded live events is not the empty one this
    /// exists to fix, so it says so instead of guessing.
    #[test]
    fn a_page_that_is_already_live_refuses_to_be_backfilled() {
        let mut snapshot = Snapshot::new();
        snapshot.apply(&record(9, EventKind::RunFrozen, [1u8; 16]));
        assert_eq!(
            snapshot.backfill(&[record(1, EventKind::RunStarted, [1u8; 16])]),
            Backfill::AlreadyLive
        );
        assert_eq!(snapshot.resume_from(), Some(Seq::new(9)));
    }

    #[test]
    fn an_unmodelled_event_kind_advances_the_cursor_without_inventing_state() {
        let mut snapshot = Snapshot::new();
        assert!(snapshot.apply(&record(9, EventKind::PromptAssembled, [6u8; 16])));
        assert_eq!(snapshot.resume_from(), Some(Seq::new(9)));
        assert_eq!(snapshot.runs().count(), 0);
    }

    #[test]
    fn the_default_view_answers_the_question_somebody_arrives_with() {
        // The city page answers "where is everything"; a person opening
        // this product is asking "is anything happening, and does any of
        // it need me". Those are different questions, and only the
        // second one is asked on arrival.
        assert_eq!(View::default(), View::Overview);
        assert_eq!(
            crate::route::from_fragment("#/"),
            Some(View::Overview),
            "the bare fragment and the default view are the same page"
        );
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

    /// What a rendered tree actually contains.
    ///
    /// The four failures this whole card repairs were invisible to every
    /// existing test because those tests called functions instead of
    /// rendering the tree that is supposed to call them. So this walks
    /// the mutations a real `VirtualDom` produces: element tags, static
    /// classes, and every piece of text a reader would see.
    #[derive(Default)]
    struct Painted {
        tags: Vec<String>,
        classes: Vec<String>,
        /// Every other static attribute value, so a placeholder - which is
        /// what a form says to a person before they type - is readable
        /// evidence like any other word on the page.
        attrs: Vec<String>,
        text: Vec<String>,
    }

    impl Painted {
        fn absorb(&mut self, node: &dioxus::dioxus_core::TemplateNode) {
            use dioxus::dioxus_core::{TemplateAttribute, TemplateNode};
            match *node {
                TemplateNode::Element {
                    tag,
                    attrs,
                    children,
                    ..
                } => {
                    self.tags.push(tag.to_string());
                    for attr in attrs {
                        if let TemplateAttribute::Static { name, value, .. } = *attr {
                            if name == "class" {
                                self.classes.push(value.to_string());
                            } else {
                                self.attrs.push(value.to_string());
                            }
                        }
                    }
                    for child in children {
                        self.absorb(child);
                    }
                }
                TemplateNode::Text { text } => self.text.push(text.to_string()),
                TemplateNode::Dynamic { .. } => {}
            }
        }

        fn says(&self, needle: &str) -> bool {
            self.text.iter().any(|line| line.contains(needle))
                || self.attrs.iter().any(|value| value.contains(needle))
        }

        fn has_class(&self, needle: &str) -> bool {
            self.classes.iter().any(|class| class.contains(needle))
        }

        /// Where a piece of text sits in reading order, ignoring what is
        /// only in an attribute. Order is what two of this card's
        /// defects were about, and a placeholder is not a label.
        fn wrote(&self, needle: &str) -> Option<usize> {
            self.text.iter().position(|line| line.contains(needle))
        }
    }

    impl dioxus::dioxus_core::WriteMutations for Painted {
        fn load_template(
            &mut self,
            template: dioxus::dioxus_core::Template,
            index: usize,
            _id: dioxus::dioxus_core::ElementId,
        ) {
            if let Some(root) = template.roots.get(index) {
                self.absorb(root);
            }
        }

        fn create_text_node(&mut self, value: &str, _id: dioxus::dioxus_core::ElementId) {
            self.text.push(value.to_owned());
        }

        fn set_node_text(&mut self, value: &str, _id: dioxus::dioxus_core::ElementId) {
            self.text.push(value.to_owned());
        }

        fn set_attribute(
            &mut self,
            name: &'static str,
            _ns: Option<&'static str>,
            value: &dioxus::dioxus_core::AttributeValue,
            _id: dioxus::dioxus_core::ElementId,
        ) {
            if let dioxus::dioxus_core::AttributeValue::Text(text) = value {
                if name == "class" {
                    self.classes.push(text.clone());
                } else {
                    self.attrs.push(text.clone());
                }
            }
        }

        fn append_children(&mut self, _id: dioxus::dioxus_core::ElementId, _m: usize) {}
        fn assign_node_id(&mut self, _path: &'static [u8], _id: dioxus::dioxus_core::ElementId) {}
        fn create_placeholder(&mut self, _id: dioxus::dioxus_core::ElementId) {}
        fn replace_node_with(&mut self, _id: dioxus::dioxus_core::ElementId, _m: usize) {}
        fn replace_placeholder_with_nodes(&mut self, _path: &'static [u8], _m: usize) {}
        fn insert_nodes_after(&mut self, _id: dioxus::dioxus_core::ElementId, _m: usize) {}
        fn insert_nodes_before(&mut self, _id: dioxus::dioxus_core::ElementId, _m: usize) {}
        fn create_event_listener(
            &mut self,
            _name: &'static str,
            _id: dioxus::dioxus_core::ElementId,
        ) {
        }
        fn remove_event_listener(
            &mut self,
            _name: &'static str,
            _id: dioxus::dioxus_core::ElementId,
        ) {
        }
        fn remove_node(&mut self, _id: dioxus::dioxus_core::ElementId) {}
        fn push_root(&mut self, _id: dioxus::dioxus_core::ElementId) {}
    }

    /// The handlers have to be minted inside a running scope, so the tree
    /// is entered through a component rather than by building props by
    /// hand.
    #[component]
    fn Harness(
        view: View,
        snapshot: Snapshot,
        records: Vec<EventRecord>,
        refused: Option<crate::alert::Refused>,
    ) -> Element {
        // Live, because a test that rendered the disconnected client
        // would be asserting about the waiting room rather than the city.
        let live = use_signal(|| true);
        // The language, as `App` provides it in a browser. Without it
        // every component that says a word would be reading a context
        // nobody put there.
        use_context_provider(|| Signal::new(crate::lang::Lang::En));
        rsx! {
            Root {
                live,
                snapshot,
                view,
                endpoints: Some(endpoints_answer()),
                city: Some(city_answer()),
                cost: Some(cost_answer()),
                building: Some(building_answer()),
                discards: Some(discard_answer()),
                inbox: None,
                hits: None,
                filed: Some(registry_answer()),
                vitals: Some(metrics_answer()),
                refused,
                records,
                selected: None,
                dropped: None,
                following: true,
                on_frame: move |_| {},
                on_select: move |_| {},
                on_drop: move |_| {},
                on_view: move |_| {},
                on_follow: move |_| {},
                on_dismiss: move |()| {},
            }
        }
    }

    fn paint(view: View, snapshot: Snapshot, records: Vec<EventRecord>) -> Painted {
        painted_with(view, snapshot, records, None)
    }

    fn painted_with(
        view: View,
        snapshot: Snapshot,
        records: Vec<EventRecord>,
        refused: Option<crate::alert::Refused>,
    ) -> Painted {
        let mut dom = VirtualDom::new_with_props(
            Harness,
            HarnessProps {
                view,
                snapshot,
                records,
                refused,
            },
        );
        let mut painted = Painted::default();
        dom.rebuild(&mut painted);
        painted
    }

    fn endpoints_answer() -> channels::EndpointsAnswer {
        channels::EndpointsAnswer {
            endpoints: Vec::new(),
            chosen: Vec::new(),
        }
    }

    fn city_answer() -> channels::CityAnswer {
        channels::CityAnswer {
            runs: Vec::new(),
            active: 0,
            frozen: 0,
            // A city with a building in it, because an empty city
            // exercises the empty state and nothing else.
            buildings: vec![channels::BuildingProgress {
                addr: Address::parse("lab").unwrap(),
                progress: channels::Progress::Planned(channels::PlannedProgress {
                    done: 1,
                    blocked: 0,
                    total: 4,
                }),
                problems: Vec::new(),
            }],
        }
    }

    fn building_answer() -> channels::BuildingAnswer {
        channels::BuildingAnswer {
            sandbox: None,
            mcp: Vec::new(),
            addr: Address::parse("lab").unwrap(),
            progress: channels::Progress::Planned(channels::PlannedProgress {
                done: 1,
                blocked: 0,
                total: 4,
            }),
            problems: Vec::new(),
            rooms: vec!["room1".to_owned()],
            docs: vec![channels::BuildingDoc {
                name: "Roadmap.md".to_owned(),
                text: "| # | item |".to_owned(),
                bytes: 12,
                truncated: false,
            }],
            archive: Vec::new(),
        }
    }

    fn discard_answer() -> channels::DiscardAnswer {
        channels::DiscardAnswer {
            rows: vec![channels::DiscardLine {
                path: "file:lab/room1/notes.md".to_owned(),
                restoration: Some(channels::Restoration::Tracked(
                    channels::Locator::parse(&format!(
                        "file:lab/room1/notes.md@{}",
                        "5a".repeat(20)
                    ))
                    .unwrap(),
                )),
                at: channels::TimeMs::new(900),
                restored: false,
            }],
        }
    }

    fn metrics_answer() -> channels::MetricsAnswer {
        channels::MetricsAnswer {
            events: 12_400,
            runs_active: 1,
            runs_frozen: 0,
            buildings: 1,
            approvals_waiting: 1,
            signals_waiting: 0,
            discards_outstanding: 1,
        }
    }

    fn registry_answer() -> channels::RegistryAnswer {
        channels::RegistryAnswer {
            assets: vec![channels::RegistryLine {
                addr: Address::parse("lab/room1").unwrap(),
                kind: "decision".to_owned(),
                subject: "we build without dx".to_owned(),
                at: channels::TimeMs::new(500),
            }],
        }
    }

    fn cost_answer() -> channels::CostAnswer {
        channels::CostAnswer {
            total: UsdMicros::new(420_000),
            by_run: Vec::new(),
            by_actor: Vec::new(),
            by_segment: Vec::new(),
            by_tool: vec![("exec".to_owned(), UsdMicros::new(420_000))],
            by_skill: Vec::new(),
        }
    }

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

    /// What each view must put on the page, as an exhaustive match.
    ///
    /// A hand-written list of views would be a second authority for
    /// "which views exist", and the variant somebody forgets to add is
    /// exactly the one that ships as an empty div. This match does not
    /// compile until a new variant states what it draws.
    fn evidence_of(view: &View) -> Vec<(&'static str, &'static str)> {
        match *view {
            // The two counts, and the list a person walks into. The
            // headline is the page: if it renders nothing, the first
            // screen is a blank one.
            View::Overview => vec![("overview", "in flight"), ("in-flight", "phase")],
            View::City => vec![
                ("city-view", "raise a building"),
                ("index", "read it"),
                // The one place the length of the Ledger is stated.
                ("vitals", "records in the Ledger"),
            ],
            // The picker, because "the latest run" is a coin toss once two
            // are in flight, and the paging controls, because a browser
            // onto a history that cannot go back one page is a window
            // painted on a wall.
            View::Live(_) => vec![("runs", "everything"), ("live", "follow the end")],
            View::Approvals => vec![("approvals", "push to the remote")],
            View::RecycleBin => vec![("recycle-bin", "restore from the checkpoint")],
            // Both halves, because the page's whole point is that the
            // disk and the record are different sources.
            View::Archive => vec![
                ("archive-search", "a word the archives may hold"),
                ("filed", "filed lately"),
            ],
            View::Dashboard => vec![("dashboard", "exec")],
            View::Ledger => vec![("ledger", "sprawling replay"), ("paging", "older")],
            // The room tab, because a room is a face of a building and
            // the page lists them in exactly one place.
            View::Building(_) => vec![("building", "Roadmap.md"), ("room", "room1/")],
            View::Settings => vec![
                ("settings", "put the key in the vault"),
                ("subscription", "start the login"),
            ],
        }
    }

    #[test]
    fn every_destination_in_the_nav_reaches_a_page_that_shows_something() {
        // The defect this pins down: four of the six views rendered an
        // empty `div` and the nav had no links at all, while every
        // module test stayed green because it called the module's pure
        // functions directly. A page is not mounted until the tree says
        // so, and this is the only test in the crate that asks the tree.
        let mut snapshot = Snapshot::new();
        snapshot.apply(&record(1, EventKind::RunStarted, [1u8; 16]));
        snapshot.adopt_approvals(vec![waiting_item()]);
        let records = vec![record(2, EventKind::ToolCalled, [1u8; 16])];
        let run = latest_run(&snapshot);

        let every_view = [
            View::Overview,
            View::City,
            View::Live(run),
            View::Approvals,
            View::RecycleBin,
            View::Archive,
            View::Dashboard,
            View::Ledger,
            View::Building(Address::parse("lab").unwrap()),
            View::Settings,
        ];
        for (view, (marker, sentence)) in every_view
            .iter()
            .flat_map(|view| evidence_of(view).into_iter().map(move |ev| (view, ev)))
        {
            let painted = paint(view.clone(), snapshot.clone(), records.clone());

            assert!(
                painted.has_class(marker),
                "{view:?} rendered no element of its own"
            );
            assert!(
                painted.says(sentence),
                "{view:?} rendered nothing a reader could read: wanted {sentence:?}"
            );
        }
    }

    #[test]
    fn work_with_no_price_is_never_rendered_as_a_column_of_zeroes() {
        // A real provider on a subscription reports what it used and not
        // what it cost, so the authoritative total is zero while four runs
        // sit in the attribution. Rendering that as $0.00 five times over
        // is the interface answering a question nobody can answer.
        let painted = paint(View::Dashboard, Snapshot::new(), Vec::new());
        if painted.says("no provider reported a price") {
            assert!(
                painted.says("unpriced"),
                "the rows say so too, rather than each showing a zero"
            );
        } else {
            assert!(
                painted.says("where the money went") || painted.says("nothing has been spent"),
                "a cost page states one of the three cases and no fourth"
            );
        }
    }

    #[test]
    fn a_person_can_reach_every_intervention_the_wire_classifies() {
        // `channels::control` names five interventions. Before F2.05 this
        // client could send two of them, so an interface for delegated
        // work offered "say something" and "stop" and nothing else. This
        // asserts the pages that own each verb actually render it; the
        // scopes that own Halt and Release are the control surface, which
        // the nav test already walks.
        let mut snapshot = Snapshot::new();
        snapshot.apply(&record(1, EventKind::RunStarted, [1u8; 16]));
        let run = latest_run(&snapshot);
        let live = paint(
            View::Live(run),
            snapshot.clone(),
            vec![record(2, EventKind::ToolCalled, [1u8; 16])],
        );
        assert!(live.says("answer for this run from here"), "Takeover");
        assert!(live.says("branch a new run from step"), "Fork");
        assert!(live.says("send at the next safe point"), "Steer");
        let bin = paint(View::RecycleBin, snapshot, Vec::new());
        assert!(
            bin.says("put the whole worktree back to that checkpoint"),
            "Rollback, and only where the way back really is a checkpoint"
        );
    }

    #[test]
    fn the_city_is_drawn_as_shapes_a_person_can_reach() {
        // What a canvas could not be asked. Before F2.02 the drawing
        // existed only on wasm, so no host test could see whether the
        // picture had been drawn at all - which is how it once shipped
        // painting the ground and no buildings.
        let painted = paint(View::City, Snapshot::new(), Vec::new());
        assert!(painted.tags.iter().any(|tag| tag == "svg"));
        assert!(painted.tags.iter().any(|tag| tag == "polygon"));
        assert!(
            painted.tags.iter().any(|tag| tag == "text"),
            "a tower says its own name; there is no legend to look away to"
        );
        assert!(
            painted.has_class("prism"),
            "each building is one group, which is what hover, focus and a keyboard reach"
        );
        assert!(
            painted.attrs.iter().any(|value| value == "button"),
            "and the group says it is a button, so a screen reader can say so too"
        );
    }

    #[test]
    fn every_page_says_where_its_numbers_came_from() {
        // The rule this holds is the product's own claim turned into a
        // property of the interface: a city whose whole promise is an
        // auditable Ledger may not put a figure on screen without saying
        // what produced it. It is asserted here, over every view at once,
        // rather than in `panel` - what matters is not that the markup
        // renders but that no page escapes it.
        let mut snapshot = Snapshot::new();
        snapshot.apply(&record(1, EventKind::RunStarted, [1u8; 16]));
        snapshot.adopt_approvals(vec![waiting_item()]);
        let run = latest_run(&snapshot);
        for view in [
            View::Overview,
            View::City,
            View::Live(run),
            View::Approvals,
            View::RecycleBin,
            View::Archive,
            View::Dashboard,
            View::Ledger,
            View::Building(Address::parse("lab").unwrap()),
            View::Settings,
        ] {
            let painted = paint(view.clone(), snapshot.clone(), Vec::new());
            assert!(
                painted.has_class("panel-source"),
                "{view:?} states something without saying where it came from"
            );
            assert!(
                painted.has_class("panel-title"),
                "{view:?} has no heading, so a reader cannot tell which page they are on"
            );
        }
    }

    #[test]
    fn a_begun_login_puts_the_url_on_the_page_and_a_finished_one_takes_it_away() {
        let mut snapshot = Snapshot::new();
        assert!(
            paint(View::Settings, snapshot.clone(), Vec::new()).says("no login is waiting"),
            "a page with no login pending says so rather than showing an empty box"
        );

        let mut data = serde_json::Map::new();
        data.insert(
            "provider".to_owned(),
            serde_json::Value::String("anthropic".to_owned()),
        );
        data.insert(
            "auth_url".to_owned(),
            serde_json::Value::String("https://example.invalid/authorize?state=x".to_owned()),
        );
        let mut draft = kernel_draft(EventKind::LoginStarted, [2u8; 16]);
        draft.data = Payload::new(data).unwrap();
        let begun = EventRecord::from_draft(draft, Seq::new(3), B3Hash::digest(b"prev"));
        snapshot.apply(&begun);
        let painted = paint(View::Settings, snapshot.clone(), Vec::new());
        assert!(
            painted.says("https://example.invalid/authorize?state=x"),
            "the url a person must open is the one the server recorded"
        );
        assert!(painted.says("finish the login"));

        snapshot.apply(&record(4, EventKind::SecretCaptured, [2u8; 16]));
        assert!(
            paint(View::Settings, snapshot, Vec::new()).says("no login is waiting"),
            "a credential in the vault ends the step that was asking for it"
        );
    }

    #[test]
    fn the_left_nav_carries_every_destination_and_says_how_many_wait() {
        let mut snapshot = Snapshot::new();
        snapshot.adopt_approvals(vec![waiting_item()]);
        let painted = paint(View::City, snapshot.clone(), Vec::new());
        // In the language the harness renders in, which is this
        // client's own: what matters here is that every destination
        // reaches the page, and `lang` holds that both languages exist.
        let word = |msg| crate::lang::say(crate::lang::Lang::En, msg);
        for group in destinations(&snapshot) {
            assert!(
                painted.says(word(group.label)),
                "the nav does not head {:?}",
                group.label
            );
            for spot in group.places {
                assert!(
                    painted.says(word(spot.label)),
                    "the nav does not offer {:?}",
                    spot.label
                );
            }
        }
        assert!(
            painted.has_class("badge"),
            "one item waits and none is shown"
        );
    }

    #[test]
    fn the_control_surface_asks_for_work_and_not_for_a_budget() {
        // A person cannot say what a task is worth before it runs, and a
        // subscription has no unit price to say it in (user verdict,
        // 2026-08-22). The form asks for the four facts a Run needs.
        let painted = paint(View::City, Snapshot::new(), Vec::new());
        assert!(painted.has_class("dispatch"));
        assert!(painted.says("what to produce"));
        assert!(painted.says("what counts as done"));
        assert!(
            !painted.says("budget") && !painted.says("how much"),
            "the dispatch bar asks for money: {:?} / {:?}",
            painted.text,
            painted.attrs
        );
    }

    #[test]
    fn a_call_with_no_reported_price_is_never_rendered_as_zero_dollars() {
        let mut snapshot = Snapshot::new();
        let mut data = serde_json::Map::new();
        data.insert(
            "usage".to_owned(),
            serde_json::json!({ "input_tokens": 40_000, "output_tokens": 8_207 }),
        );
        snapshot.apply(&EventRecord::from_draft(
            channels::EventDraft {
                run: RunId::from_bytes([9u8; 16]),
                t: channels::TimeMs::new(1),
                who: "urbanite-1".to_owned(),
                addr: None,
                kind: EventKind::ModelReturned,
                data: channels::Payload::new(data).unwrap(),
                ig: false,
            },
            Seq::new(1),
            channels::B3Hash::digest(b"prev"),
        ));
        let line = spend_line(crate::lang::Lang::En, &snapshot);
        assert!(!line.contains('$'), "{line}");
        assert!(line.contains("48.2k tokens"), "{line}");
        assert!(line.contains("no price reported"), "{line}");
        assert_eq!(snapshot.usage().unpriced_calls, 1);
    }

    /// The defect this card exists for, seen from the end that matters:
    /// somebody presses attach, the city refuses, and the page has to
    /// say so. Until this test existed the client received the refusal
    /// frame and dropped it, and no page anywhere in this crate could
    /// have shown one.
    #[test]
    fn a_refusal_is_on_the_page_with_the_way_out_beside_it() {
        let told = crate::alert::refused(
            crate::lang::Lang::En,
            &channels::AxError::failure(
                channels::AxCode::ConfigInvalid,
                "attach an endpoint",
                "modelscope",
            )
            .with_recovery("the base url needs its /v1"),
        );
        let painted = painted_with(View::City, Snapshot::new(), Vec::new(), Some(told));
        assert!(
            painted.classes.iter().any(|c| c == "refusal"),
            "the refusal has nowhere to appear: {:?}",
            painted.classes
        );
        // A refusal a person can read is one they can act on, so the
        // way out is on screen beside what was refused.
        for part in ["refusal-what", "refusal-way"] {
            assert!(
                painted.classes.iter().any(|c| c == part),
                "{part} is missing from the page"
            );
        }
    }

    /// A city that has refused nothing draws no strip at all: a banner
    /// that is always there is a banner nobody reads.
    #[test]
    fn a_page_with_nothing_refused_carries_no_strip() {
        let painted = painted_with(View::City, Snapshot::new(), Vec::new(), None);
        assert!(!painted.classes.iter().any(|c| c == "refusal"));
    }

    /// The order of the settings page, which is the order a person can
    /// perform its steps in.
    ///
    /// The city this card was cut from had a key in its vault, two
    /// buildings, and no endpoint: the one section that makes the other
    /// three non-empty sat last, below the fold, with its own submit
    /// button off-screen.
    /// The run a person just started is the one they are taken to.
    ///
    /// Not a guess between several runs - the client sent this dispatch
    /// and knows which room it asked for, so recognising the start of
    /// that run is knowledge rather than a coin toss (web-SPEC 8-31).
    #[test]
    fn the_session_a_person_just_started_is_the_one_they_are_shown() {
        let started = |addr: &str, run: [u8; 16]| {
            let mut draft = kernel_draft(EventKind::RunStarted, run);
            draft.addr = Some(Address::parse(addr).unwrap());
            EventRecord::from_draft(draft, Seq::new(1), B3Hash::digest(b"prev"))
        };
        let mine = started("lab/refactor", [4u8; 16]);
        assert_eq!(
            started_here(&mine, "lab/refactor"),
            Some(RunId::from_bytes([4u8; 16]))
        );
        // The city suffixes a name that is taken, so the room it opened
        // is not always the room that was asked for.
        let suffixed = started("lab/refactor-2", [5u8; 16]);
        assert_eq!(
            started_here(&suffixed, "lab/refactor"),
            Some(RunId::from_bytes([5u8; 16]))
        );
        // Somebody else's work, and a name that merely begins the same
        // way, are not this person's session.
        assert_eq!(
            started_here(&started("lab/other", [6u8; 16]), "lab/refactor"),
            None
        );
        assert_eq!(
            started_here(&started("lab/refactoring", [7u8; 16]), "lab/refactor"),
            None
        );
        // Only the start of a run: a later event in that room is not a
        // second reason to move the page somebody may have navigated
        // away from.
        let mut later = kernel_draft(EventKind::ToolCalled, [4u8; 16]);
        later.addr = Some(Address::parse("lab/refactor").unwrap());
        let later = EventRecord::from_draft(later, Seq::new(2), B3Hash::digest(b"prev"));
        assert_eq!(started_here(&later, "lab/refactor"), None);
    }

    /// A person standing on a building's page can start work there.
    #[test]
    fn a_building_page_offers_to_start_a_session_in_that_building() {
        let painted = paint(
            View::Building(Address::parse("lab").unwrap()),
            Snapshot::new(),
            Vec::new(),
        );
        assert!(
            painted.says("start a session here"),
            "the only way to work in this building is a bar on another page"
        );
    }

    /// A session is picked by the name its person gave it. The picker
    /// offered `d41d8cd9 · running`, which identifies a run to a machine
    /// and nothing at all to the person who started it.
    #[test]
    fn a_session_is_offered_by_its_name_and_not_by_its_hash() {
        let mut snapshot = Snapshot::new();
        let mut draft = kernel_draft(EventKind::RunStarted, [3u8; 16]);
        draft.addr = Some(Address::parse("lab/refactor-the-ledger").unwrap());
        snapshot.apply(&EventRecord::from_draft(
            draft,
            Seq::new(1),
            B3Hash::digest(b"prev"),
        ));
        let run = latest_run(&snapshot).expect("one run started");

        let offered = watchable(&snapshot);
        let (_, label) = offered.first().expect("the run is offered");
        assert!(
            label.contains("refactor-the-ledger"),
            "the picker does not name the session: {label}"
        );

        let painted = paint(View::Live(Some(run)), snapshot, Vec::new());
        assert!(
            painted.says("refactor-the-ledger"),
            "the page being watched does not say which session it is"
        );
    }

    /// The session list was flat, so a person watching a city where a
    /// run had handed work down saw two peers and no way to tell which
    /// answered for which.
    #[test]
    fn work_handed_down_is_listed_under_the_run_that_handed_it_down() {
        let mut snapshot = Snapshot::new();
        let parent = RunId::from_bytes([3u8; 16]);
        let mut opened = kernel_draft(EventKind::RunStarted, [3u8; 16]);
        opened.addr = Some(Address::parse("lab/room1").unwrap());
        snapshot.apply(&EventRecord::from_draft(
            opened,
            Seq::new(1),
            B3Hash::digest(b"prev"),
        ));

        let mut handed = kernel_draft(EventKind::RunStarted, [4u8; 16]);
        handed.addr = Some(Address::parse("lab/helper").unwrap());
        let mut data = serde_json::Map::new();
        data.insert(
            "parent".to_owned(),
            serde_json::Value::String(parent.to_string()),
        );
        handed.data = Payload::new(data).unwrap();
        snapshot.apply(&EventRecord::from_draft(
            handed,
            Seq::new(2),
            B3Hash::digest(b"prev2"),
        ));

        let offered = watchable(&snapshot);
        let labels: Vec<&str> = offered.iter().map(|(_, label)| label.as_str()).collect();
        assert_eq!(labels.len(), 2);
        assert!(
            labels[0].starts_with("room1"),
            "the run that asked comes first: {labels:?}"
        );
        assert!(
            labels[1].starts_with("\u{21b3} helper (room1)"),
            "the delegate is listed under it and says whose it is: {labels:?}"
        );
    }

    #[test]
    fn the_settings_page_leads_with_the_step_a_new_city_cannot_skip() {
        let painted = paint(View::Settings, Snapshot::new(), Vec::new());
        let attach = painted
            .wrote("Attach a provider")
            .expect("the page never offers to attach a provider");
        let choose = painted
            .wrote("choose a model for a job")
            .expect("the page never offers to choose a model");
        let tags = painted
            .wrote("what each model is for")
            .expect("the page never says what each tag is for");
        assert!(
            attach < choose && attach < tags,
            "the first step is not first: attach {attach}, choose {choose}, tags {tags}"
        );
    }

    /// The four controls of the dispatch bar carry labels, not only
    /// placeholders - this repository's own stylesheet says why, and the
    /// bar was the one form in the client that ignored it.
    #[test]
    fn the_dispatch_bar_names_its_fields_where_the_name_survives_typing() {
        let painted = paint(View::Overview, Snapshot::new(), Vec::new());
        for label in ["room", "task", "done when", "mode"] {
            assert!(
                painted.text.iter().any(|line| line == label),
                "the dispatch bar has no label {label:?}: {:?}",
                painted.text
            );
        }
    }

    /// A steer box with no session chosen sent nothing, said nothing,
    /// and looked exactly like one that would work.
    #[test]
    fn a_session_nobody_chose_gets_a_sentence_rather_than_a_dead_input() {
        let mut snapshot = Snapshot::new();
        snapshot.apply(&record(1, EventKind::RunStarted, [1u8; 16]));
        let painted = paint(View::Live(None), snapshot, Vec::new());
        assert!(
            painted.says("pick a session above to speak into it"),
            "the page offers no way to learn what to do next"
        );
        assert!(
            !painted.says("say something into this run"),
            "a box that cannot send is still on the page"
        );
    }

    #[test]
    fn money_renders_through_integers_only() {
        assert_eq!(render_usd(UsdMicros::new(0)), "$0.00");
        assert_eq!(render_usd(UsdMicros::new(1_000_000)), "$1.00");
        assert_eq!(render_usd(UsdMicros::new(1_234_567)), "$1.23");
        assert_eq!(render_usd(UsdMicros::new(9_999)), "$0.00", "truncates down");
        assert_eq!(render_usd(UsdMicros::new(u64::MAX)), "$18446744073709.55");
    }
}

/// The signals one painted frame may move.
///
/// A struct rather than fifteen parameters, and by value because every
/// field is a `Copy` handle: this is the same reasoning that gave `Wiring`
/// its shape, and splitting it would produce two halves neither of which
/// can paint a frame.
#[cfg(target_arch = "wasm32")]
#[derive(Clone, Copy)]
struct FrameWiring {
    snapshot: Signal<Snapshot>,
    endpoints: Signal<Option<channels::EndpointsAnswer>>,
    city: Signal<Option<channels::CityAnswer>>,
    cost: Signal<Option<channels::CostAnswer>>,
    building: Signal<Option<channels::BuildingAnswer>>,
    discards: Signal<Option<channels::DiscardAnswer>>,
    inbox: Signal<Option<channels::InboxAnswer>>,
    hits: Signal<Option<channels::ArchiveAnswer>>,
    filed: Signal<Option<channels::RegistryAnswer>>,
    vitals: Signal<Option<channels::MetricsAnswer>>,
    records: Signal<Vec<channels::EventRecord>>,
    view: Signal<View>,
    expecting: Signal<Option<String>>,
    refused: Signal<Option<crate::alert::Refused>>,
    lang: Signal<crate::lang::Lang>,
}

/// Applies one animation frame's worth of arrivals.
///
/// The order is the one `Paint::into_parts` hands them out in and the
/// reason is recorded there: an answer describes the city as of some
/// moment, so folding the frame's events first is what stops a page
/// rendering a view its own snapshot has not caught up with.
///
/// Every event is still read one at a time - `alert::absorb` deduplicates
/// an interruption, `invalidated_by` asks again, `started_here` recognises
/// the room this client asked for. What the frame changed is *when the
/// signals move*, and they now move once for the whole burst.
#[cfg(target_arch = "wasm32")]
fn apply_frame(
    paint: crate::pace::Paint,
    socket: &std::rc::Rc<std::cell::RefCell<Option<web_sys::WebSocket>>>,
    alerts: &std::rc::Rc<std::cell::RefCell<crate::alert::Alerts>>,
    wiring: FrameWiring,
) {
    let FrameWiring {
        mut snapshot,
        mut endpoints,
        mut city,
        mut cost,
        mut building,
        mut discards,
        mut inbox,
        mut hits,
        mut filed,
        mut vitals,
        mut records,
        mut view,
        mut expecting,
        mut refused,
        lang,
    } = wiring;
    let (events, answers, refusal) = paint.into_parts();
    let said = lang();
    // Everything the burst adds to history, folded and kept in one write
    // each rather than in one write each per event.
    let mut keep: Vec<channels::EventRecord> = Vec::new();
    for event in events {
        // Decided in the same pass as the snapshot: what happened and
        // whether it needs a person are two readings of one event, not two
        // readers of the stream.
        if let Ok(mut alerts) = alerts.try_borrow_mut()
            && crate::alert::absorb(said, &mut alerts, &event) == crate::alert::Raise::Interrupt
            && let Some(alert) = crate::alert::alert_for(said, &event)
        {
            crate::alert::interrupt(said, &alert);
        }
        if let Some(query) = invalidated_by(event.kind()) {
            let held = socket.borrow();
            if let Some(socket) = held.as_ref() {
                let _ = crate::socket::send(socket, &channels::ClientFrame::Query(query));
            }
        }
        // The session this person asked for, opening. Knowledge rather
        // than a guess: this client sent that dispatch and knows the room
        // it named. Read and released before the write below: a signal
        // held open across its own set is a panic in a browser and nothing
        // at all in a host test.
        let waiting = expecting.read().clone();
        if let Some(waiting) = waiting
            && let Some(run) = started_here(&event, &waiting)
        {
            expecting.set(None);
            view.set(View::Live(Some(run)));
            crate::route::go(&View::Live(Some(run)));
        }
        if snapshot.write().apply(&event) {
            keep.push(event);
        }
    }
    if !keep.is_empty() {
        // Kept once, read by every page that reads history, and bounded by
        // the one function that answers "how much does a tab hold".
        hold(&mut records.write(), keep);
    }
    // An answer reaches the view that asked for it. It is not history: it
    // moves no snapshot, and a reload asks again rather than trusting what
    // is held.
    for answer in answers {
        match answer {
            channels::Answer::Endpoints(held) => endpoints.set(Some(held)),
            channels::Answer::City(held) => city.set(Some(held)),
            channels::Answer::Cost(held) => cost.set(Some(*held)),
            channels::Answer::Building(held) => building.set(Some(*held)),
            channels::Answer::Discards(held) => discards.set(Some(held)),
            channels::Answer::Inbox(held) => inbox.set(Some(held)),
            channels::Answer::Archive(held) => hits.set(Some(held)),
            channels::Answer::Registry(held) => filed.set(Some(held)),
            channels::Answer::Metrics(held) => vitals.set(Some(*held)),
            // What happened before this tab opened. Folded into the
            // snapshot and kept for the pages that read history, in the
            // same bounded store the live stream fills - one answer to
            // "how much does a tab hold".
            channels::Answer::History(held) => {
                snapshot.write().backfill(&held.records);
                hold(&mut records.write(), held.records);
            }
            // What was already waiting when this page connected. The
            // stream carries what happens next; without this the inbox
            // would show only the items raised since the tab opened.
            channels::Answer::Approvals(held) => {
                snapshot.write().adopt_approvals(held.items);
            }
            // Named one by one rather than caught by a wildcard: each of
            // these has an answer the server can give and no page that
            // asks for it yet, and a wildcard here would hide the next one
            // that arrives as well.
            channels::Answer::Run(_) | channels::Answer::Unavailable { .. } => {}
        }
    }
    // A refusal is not history and must not move the snapshot - but it is
    // the answer to something a person just did, so it goes where they can
    // read it.
    if let Some(error) = refusal {
        refused.set(Some(crate::alert::refused(said, &error)));
    }
}
