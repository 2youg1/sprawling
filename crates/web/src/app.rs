// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

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
use crate::phase::Phase;
use dioxus::prelude::*;

/// Which page the content region is showing (web-SPEC.md section 8-53
/// B1). Three regions fill the window — top bar, left nav, content — and
/// only the content region routes.
///
/// **Six destinations, and two shapes that are reached from them.** The
/// previous set had eleven entries and no page for the one object this
/// product has: a session. Eight of those eleven were a list of
/// sessions, a history of sessions, or settings, and each had a nav
/// entry of its own — so the interface asked a person to choose between
/// eleven answers to a question they had not asked yet.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum View {
    /// The list, and the box that starts work. Default because the
    /// action a person arrives to take is on it.
    #[default]
    Sessions,
    /// One session: a work line in one room, named by the name a person
    /// gave it. The object page this interface never had.
    Session(Address),
    /// Everything that cannot move until a person answers.
    Waiting,
    /// One history, in three lenses. They were three pages with three
    /// nav entries, and no reader ever had to choose between them
    /// before knowing what they were looking for.
    Record(Lens),
    /// Spend, in five cuts.
    Cost,
    /// Where a provider is registered, and which language this reads in.
    /// A region rather than a modal: registering is work, and work that
    /// can be interrupted needs a place to return to.
    Setup,
    /// One building's own files and archive. Reached from a session and
    /// from the city drawing, not from the nav: it is where sessions
    /// live rather than a sixth thing to check.
    Building(Address),
    /// A run named by a link written before sessions had addresses.
    /// Held as its own view because the router is pure and the room a
    /// run is in is a fact only the snapshot has; the page resolves it
    /// and moves on, so this is a state the address bar passes through.
    Run(RunId),
}

/// Which lens the record is read through.
///
/// One page, three lenses, because they are three questions about one
/// history and a person picks the lens after deciding to look — not
/// before. `Bin` is spelled short in the fragment and long on screen:
/// the address bar is typed and the heading is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lens {
    /// Every record, newest first.
    #[default]
    Ledger,
    /// What was filed on the shelves, and what went there lately.
    Archive,
    /// What was discarded, and the way each row comes back.
    Bin,
}

impl Lens {
    /// Every lens, in the order the page offers them: the whole history,
    /// then what was kept, then what was thrown away.
    pub const ALL: [Lens; 3] = [Lens::Ledger, Lens::Archive, Lens::Bin];

    /// What this lens is called on screen.
    #[must_use]
    pub fn word(self) -> Msg {
        match self {
            Self::Ledger => Msg::RecordLensLedger,
            Self::Archive => Msg::RecordLensArchive,
            Self::Bin => Msg::RecordLensBin,
        }
    }
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
    /// The word shown to a person.
    ///
    /// A `Msg` and not a `&str`, which is the whole point: the doc here
    /// used to say "microcopy has one authority" while being a second one,
    /// in English only. Both callers put the result in a slot, so a
    /// Chinese page rendered `provider 状态：unknown` - the variant's own
    /// name, straight out of the enum. `web::lang` makes a missing
    /// translation unrepresentable, and a slot filled with a `&'static
    /// str` is how that guarantee was got around.
    #[must_use]
    pub fn word(self) -> Msg {
        match self {
            Self::Unknown => Msg::ProviderUnknown,
            Self::Healthy => Msg::ProviderHealthy,
            Self::Degraded => Msg::ProviderDegraded,
            Self::Lost => Msg::ProviderLost,
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
    /// What a person called this piece of work. Folded from the address
    /// the run opened in, because the room *is* the name: `Dispatch`
    /// takes a session name and the city opens a room of that name, so
    /// the last segment of the address is the word a person typed.
    pub session: Option<String>,
    /// The run that handed this work down, when one did. Folded from
    /// `run_started`, so a page and an offline replay draw the same
    /// tree.
    pub parent: Option<RunId>,
    pub phase: Phase,
    pub steps_done: u32,
    pub steps_planned: Option<u32>,
    pub started_at_seq: Seq,
    /// The last record this client folded for this run. What the session
    /// page needs in order to say how long ago something happened, and
    /// what `Fork` needs in order to name a point a person can mean.
    pub last_seq: Seq,
    /// How many turns have closed. A turn is a model call and whatever
    /// it caused, so this counts `model_returned` rather than records.
    pub turns: u32,
    /// The turn at which a handoff was last written, if one ever was.
    pub handoff_at_turn: Option<u32>,
    /// Which gate is holding this run, when one is. Named by the gate,
    /// not by the request: a person asked "what is it stuck on" and the
    /// answer is a door, not an identifier.
    pub gate: Option<String>,
    /// What this run has cost so far, in the two halves that are
    /// separately knowable. `None` where no call has settled an amount.
    pub spent: Option<UsdMicros>,
    /// The last thing the model said, trimmed to one line. What a row in
    /// the list shows so that a person can tell two sessions apart
    /// without opening either.
    pub said: Option<String>,
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
    /// What a model is saying right now, per run, before the call it
    /// belongs to has settled.
    ///
    /// **Discardable, and discarded.** It is not folded from the ledger,
    /// it does not survive a reload, and `model_returned` throws the
    /// run's buffer away and lets the record speak. Where an increment
    /// and the settled text disagree — a provider that revised, a stream
    /// that was cut — the record wins, because the record is the one a
    /// replay produces.
    saying: BTreeMap<RunId, String>,
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
    /// How many things cannot move until this person answers.
    ///
    /// What the nav badge shows and what the waiting page lists, from one
    /// producer: a badge counting a set the page does not show is worse
    /// than no badge, because a person who clicks it and finds nothing
    /// stops believing the next one.
    ///
    /// Records this client could not read as items are counted in. One
    /// that is not shown is still one that waits, and a badge quietly
    /// short by one is wrong about the only fact it carries.
    #[must_use]
    pub fn waiting_on_you(&self) -> u32 {
        self.approvals_pending()
            .saturating_add(self.unreadable_approvals)
    }

    /// The room a run is in, when this client has folded its start.
    ///
    /// The one answer to "which session is this run", which is what a
    /// link written as `#/live/<uuid>` needs before it can be shown as
    /// the page a person can name.
    #[must_use]
    pub fn room_of(&self, id: &RunId) -> Option<&Address> {
        self.runs.get(id)?.addr.as_ref()
    }

    /// The newest run in a room, and its id.
    ///
    /// Newest rather than every one, because a room is a work line: a
    /// person who opens `lab/parser` means the current state of that
    /// work, and its earlier runs are its history rather than its
    /// siblings. Ordered by the sequence the run started at, so a replay
    /// and a live fold pick the same one.
    #[must_use]
    pub fn session_at(&self, addr: &Address) -> Option<(RunId, &RunRow)> {
        self.runs
            .iter()
            .filter(|(_, row)| row.addr.as_ref() == Some(addr))
            .max_by_key(|(_, row)| row.started_at_seq)
            .map(|(id, row)| (*id, row))
    }

    /// The three counts the top bar keeps on every page: what is moving,
    /// what is stopped on a person, and how many buildings there are.
    ///
    /// Buildings are counted from the rooms this client has seen work
    /// in, so the number is "buildings with work" rather than "folders on
    /// disk" — the city answer holds the second one and says so.
    #[must_use]
    pub fn counts(&self) -> (u32, u32, u32) {
        let mut running = 0u32;
        let mut waiting = 0u32;
        let mut buildings = std::collections::BTreeSet::new();
        for row in self.runs.values() {
            if row.phase.needs_a_person() {
                waiting = waiting.saturating_add(1);
            } else if row.phase.in_flight() {
                running = running.saturating_add(1);
            }
            if let Some(addr) = row.addr.as_ref()
                && let Some((building, _)) = addr.as_str().split_once('/')
            {
                buildings.insert(building.to_owned());
            }
        }
        (
            running,
            waiting,
            u32::try_from(buildings.len()).unwrap_or(u32::MAX),
        )
    }

    /// The sequence this client has folded through, for the line that
    /// says where an answer came from.
    #[must_use]
    pub fn applied_through(&self) -> Option<Seq> {
        self.applied_through
    }

    /// Adds text a model is still saying to the run's display buffer.
    ///
    /// Not `apply`: an increment is not an event, so it does not move
    /// `applied_through`, does not count as having gone live, and cannot
    /// be replayed. A buffer for a run this client never saw start is
    /// kept anyway — the increments arrived, so the run exists, and
    /// discarding them would blank the page a person is watching.
    pub fn is_saying(&mut self, delta: &channels::Delta) {
        self.saying
            .entry(delta.run)
            .or_default()
            .push_str(&delta.text);
    }

    /// What a model is saying in this run, while it is still saying it.
    ///
    /// `None` once the call has settled: the page then draws the record,
    /// which is the text a replay would produce.
    #[must_use]
    pub fn saying(&self, run: &RunId) -> Option<&str> {
        self.saying.get(run).map(String::as_str)
    }

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
        // Every record moves the run's high-water mark, whatever else it
        // does. Held here rather than in each arm below, because a fact
        // that is true of all of them written once cannot be forgotten
        // by the next arm somebody adds.
        if let Some(row) = self.runs.get_mut(&run) {
            row.last_seq = event.seq();
        }
        match event.kind() {
            EventKind::CityInitialized => self.city = event.addr().cloned(),
            EventKind::RunStarted | EventKind::RunForked => {
                let addr = event.addr().cloned();
                self.runs.insert(
                    run,
                    RunRow {
                        session: addr.as_ref().and_then(session_named_by),
                        addr,
                        parent: event
                            .data()
                            .as_map()
                            .get("parent")
                            .and_then(serde_json::Value::as_str)
                            .and_then(|raw| RunId::parse(raw).ok()),
                        phase: Phase::Running,
                        steps_done: 0,
                        steps_planned: None,
                        started_at_seq: event.seq(),
                        last_seq: event.seq(),
                        turns: 0,
                        handoff_at_turn: None,
                        gate: None,
                        spent: None,
                        said: None,
                    },
                );
            }
            EventKind::ToolResult => {
                if let Some(row) = self.runs.get_mut(&run) {
                    row.steps_done = row.steps_done.saturating_add(1);
                }
            }
            EventKind::ModelReturned => {
                let said = event
                    .data()
                    .as_map()
                    .get("message")
                    .and_then(crate::turn::said_in);
                let billed = event
                    .data()
                    .as_map()
                    .get("billed_usd_micros")
                    .and_then(serde_json::Value::as_u64);
                // The call settled, so the buffer of what it was saying
                // is thrown away and the record speaks. This is the
                // whole of the rule that the settled text wins.
                self.saying.remove(&run);
                if let Some(row) = self.runs.get_mut(&run) {
                    row.steps_done = row.steps_done.saturating_add(1);
                    // A turn is a model call and whatever it caused, so
                    // the return is what closes one. Counting records
                    // here would make a turn that ran six tools six
                    // times longer than one that ran none.
                    row.turns = row.turns.saturating_add(1);
                    if let Some(said) = said {
                        row.said = Some(said);
                    }
                    if let Some(billed) = billed {
                        let held = row.spent.unwrap_or_default().get();
                        row.spent = Some(UsdMicros::new(held.saturating_add(billed)));
                    }
                }
                self.absorb_call(event);
            }
            // What a person asked for and did not get yet. The handoff is
            // the scene the next holder finds, and how long ago it was
            // written is what says whether that scene is still current.
            EventKind::HandoffWritten => {
                if let Some(row) = self.runs.get_mut(&run) {
                    row.handoff_at_turn = Some(row.turns);
                }
            }
            EventKind::ApprovalRequested => {
                // The payload is the item, because that is what the writer
                // serialised. A count would not survive a reload and could
                // not be grouped; the item can do both.
                let value = serde_json::Value::Object(event.data().as_map().clone());
                let mut gate = None;
                match serde_json::from_value::<ApprovalItem>(value) {
                    Ok(item) => {
                        gate = Some(gate_named_by(&item));
                        self.approvals.insert(item.id.as_str().to_owned(), item);
                    }
                    Err(_) => {
                        self.unreadable_approvals = self.unreadable_approvals.saturating_add(1);
                    }
                }
                if let Some(row) = self.runs.get_mut(&run) {
                    row.phase = Phase::Waiting;
                    row.gate = gate;
                }
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
                if let Some(row) = self.runs.get_mut(&run) {
                    row.phase = Phase::Running;
                    row.gate = None;
                }
            }
            // The ending is in the record: `kernel::completion` writes
            // which of the three it was, and a run a person stopped
            // reads differently from one that ran out of turns. The
            // client showed both as "frozen" while the word that told
            // them apart was in the payload it was already folding.
            EventKind::RunFrozen => {
                let ending = event
                    .data()
                    .as_map()
                    .get("completion")
                    .and_then(serde_json::Value::as_str)
                    .map(str::to_owned);
                self.set_phase(&run, Phase::ended_as(ending.as_deref()));
            }
            EventKind::BudgetLimit => self.set_phase(&run, Phase::Frozen),
            EventKind::CityHalted => {
                self.halted = true;
                // Only what was still moving. A session that ended
                // yesterday did not stop because the city did, and
                // rewriting its ending would lose the one a person needs
                // in order to decide whether to pick it back up.
                for row in self.runs.values_mut() {
                    if row.phase.in_flight() {
                        row.phase = Phase::Halted;
                    }
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

    fn set_phase(&mut self, run: &RunId, phase: Phase) {
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

/// Every destination the left nav offers, in reading order.
///
/// One producer for the list, its wording and its badge: a destination
/// added here appears in the nav, in the router and in the test that
/// walks them, and cannot appear in two of the three.
///
/// **Five, and flat.** The previous nav had nine entries under three
/// headings, and the headings existed because nine entries read as a
/// menu to be searched rather than a place to go. Five is inside the
/// span a person holds without searching, so the headings are not
/// replaced by better headings — they are not needed.
#[must_use]
pub fn destinations(snapshot: &Snapshot) -> Vec<Destination> {
    let waiting = snapshot.waiting_on_you();
    vec![
        Destination {
            view: View::Sessions,
            label: crate::lang::Msg::NavSessions,
            waiting: None,
        },
        // The one badge in this interface. It is here on every page
        // because an unfinished thing that is out of sight stops being
        // an unfinished thing and starts being a surprise.
        Destination {
            view: View::Waiting,
            label: crate::lang::Msg::NavWaiting,
            waiting: (waiting > 0).then_some(waiting),
        },
        Destination {
            view: View::Record(Lens::Ledger),
            label: crate::lang::Msg::NavTheRecord,
            waiting: None,
        },
        Destination {
            view: View::Cost,
            label: crate::lang::Msg::NavCost,
            waiting: None,
        },
        Destination {
            view: View::Setup,
            label: crate::lang::Msg::NavSettings,
            waiting: None,
        },
    ]
}

/// Whether this destination is the page being shown.
///
/// The record's three lenses are one destination, so the nav entry stays
/// marked while a person moves between them: an entry that unhighlights
/// when the reader is still inside it says they have left.
#[must_use]
pub fn showing(destination: &View, view: &View) -> bool {
    match (destination, view) {
        (View::Record(_), View::Record(_)) => true,
        // A session and a building are reached from the list, and the
        // list stays lit while a person is inside one: they went deeper
        // into what the first entry offers rather than somewhere else.
        (View::Sessions, View::Session(_) | View::Building(_) | View::Run(_)) => true,
        (left, right) => left == right,
    }
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

/// The name a person gave this piece of work, out of the address the run
/// opened in.
///
/// `Dispatch` carries a session name and the city opens a room of that
/// name, so the last segment of a room address is the word somebody
/// typed. A bare building is not a session: work sent to `lab` with no
/// name is answered in a room the city named, and reporting the
/// building as the session name would put every unnamed run under one
/// heading.
fn session_named_by(addr: &Address) -> Option<String> {
    let raw = addr.as_str();
    let (building, room) = raw.rsplit_once('/')?;
    (!building.is_empty() && !room.is_empty()).then(|| room.to_owned())
}

/// Which door a run is stopped at, named by the door.
///
/// A person who asks what a session is stuck on wants the gate, not the
/// identifier of the request: `ApprovalId` is unique and says nothing,
/// while the class is the one word that tells them whether this is
/// theirs to answer now or later.
fn gate_named_by(item: &ApprovalItem) -> String {
    match item.cluster_key.class {
        channels::ApprovalClass::Commitment => "commit",
        channels::ApprovalClass::BudgetLimit => "budget",
        channels::ApprovalClass::DiscardEscalate => "discard",
        channels::ApprovalClass::AgentQuestion => "question",
        channels::ApprovalClass::Governance => "governance",
        channels::ApprovalClass::Delegation => "delegation",
        // The class set is open on the wire, and a class this build has
        // no word for is still a door this run is stopped at. Saying so
        // beats saying nothing: the person learns the session needs them
        // and the page they open names the request in full.
        _ => "a gate this build cannot name",
    }
    .to_owned()
}

/// The efforts this bar offers, in the order it offers them: what the
/// city already resolves, plus the one that leaves the answer to the
/// layer above.
pub(crate) const EFFORTS: [(&str, Msg); 6] = [
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
        .max_by_key(|(_, row)| (row.phase == Phase::Running, row.started_at_seq))
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

/// The modes a person may pick between, in the order the control offers
/// them.
///
/// `runtime::Mode` is the authority for the set and this is the
/// authority for its spelling on the wire: `ModeTag::parse` accepts any
/// string and `mode_of` reads an unknown one as planning, so a typo here
/// would silently change what a run is allowed to do.
pub const MODES: [&str; 5] = ["build", "up", "sc", "ud", "experiment"];

/// Builds one Dispatch. The only place in the client that does.
///
/// No budget travels from a person: `BudgetCap::default()` is what the
/// wire carries, and what a run costs is reported after it runs. This
/// city has no budget lock, so the composer neither asks for a figure
/// nor shows one.
///
/// **`room` is split, not sent whole.** `lab/parser` means the building
/// `lab` and a session a person is calling `parser`, which is exactly
/// what the wire's two fields say: given a session name the city opens a
/// room of that name under the building, and two dispatches naming one
/// room are one session continued. A bare `lab` sends no session name at
/// all, which is the city's cue to work one out from the task.
///
/// **The goal is left empty, and that is a meaning rather than a gap.**
/// A dispatch with no goal is already how this city spells "a person is
/// at the other end": no job file is written and the frozen prefix says
/// so. That is exactly what one sentence typed into the composer is, so
/// the box sends no goal and the city reads it as it always did. A
/// client that copied the task into the goal field would turn every
/// conversation into a task nobody asked for.
#[must_use]
pub fn dispatch_command(
    room: &str,
    task: &str,
    goal: &str,
    mode: &str,
    effort: Option<channels::Effort>,
) -> Option<channels::ClientFrame> {
    let task = task.trim();
    if task.is_empty() {
        return None;
    }
    let (building, session) = match room.trim().split_once('/') {
        Some((building, named)) => (building, Some(channels::SessionName::parse(named).ok()?)),
        None => (room.trim(), None),
    };
    let addr = Address::parse(building).ok()?;
    Some(channels::ClientFrame::Command(Box::new(
        channels::WireCommand::Dispatch {
            idem: channels::IdemKey::derive(
                &RunId::CITY,
                Seq::FIRST,
                format!("{}|{task}", addr.as_str()).as_bytes(),
            ),
            addr,
            task: task.to_owned(),
            goal: goal.trim().to_owned(),
            mode: channels::ModeTag::parse(mode).ok()?,
            budget: channels::BudgetCap::default(),
            session,
            effort,
        },
    )))
}

/// Renders micro-dollars as dollars, in integers.
///
/// No float anywhere: money is an integer count of micro-dollars end to end
///, and converting to `f64` for display would introduce
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

/// The root: three regions, and nothing that decides anything.
///
/// Business state is the server's; this reads a snapshot handed to it.
///
/// **Five regions became three.** The right-hand column carried a
/// provider status that read "normal" almost always, and a steady
/// "everything is fine" is the absence of a problem rather than a fact:
/// it never changed anybody's next action, so it does not stay on
/// screen. The three counts it also held do change the next action, so
/// they moved into the top bar. The footer held a dispatch bar, which
/// now stands at the top of the table its rows land in.
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
    /// What the city last refused this person, if anything. Cleared by
    /// the person, never by the passage of time: an answer that fades
    /// before it is read is an answer nobody gave.
    refused: Option<crate::alert::Refused>,
    records: Vec<EventRecord>,
    selected: Option<String>,
    /// A task line a drop wrote, on its way to the composer.
    dropped: Option<String>,
    /// A line a drop wrote into an open session's box. Separate from
    /// `dropped` because the two boxes take different gestures: aiming
    /// new work, and saying something into work already running.
    steered: Option<String>,
    /// What this city has written down, counted. Read by the record
    /// page, which is the page those counts are about.
    vitals: Option<channels::MetricsAnswer>,
    /// What the open session changed on disk, once the server has said.
    changes: Option<channels::ChangesAnswer>,
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
    on_dismiss: EventHandler<()>,
) -> Element {
    // The language every word on this page is said in. One signal for
    // the whole tree rather than a prop through twenty components: what
    // a person reads in is a fact about the page, not about a panel.
    let lang = use_context::<Signal<crate::lang::Lang>>();
    let word = move |msg: crate::lang::Msg| crate::lang::say(lang(), msg);
    let spots = destinations(&snapshot);
    let counts = crate::sessions::counts_said(lang(), &snapshot);
    let unwell = !matches!(snapshot.provider(), ProviderHealth::Healthy);

    // An old link naming a run is resolved here, where the room is
    // known, and the address bar is rewritten to the name a person can
    // read. The router stays pure; the redirect happens once, in the one
    // place that holds the fact it needs.
    let resolving = view.clone();
    let resolved = crate::session::room_for_link(&snapshot, &resolving);
    use_effect(use_reactive!(|(resolved,)| {
        if let Some(landed) = resolved.clone() {
            on_view.call(landed);
        }
    }));

    rsx! {
        main { class: "layout",
            header { class: "top-bar",
                span { class: "address", "{page_named(lang(), &view)}" }
                // Only when it is not normal. A steady "provider: fine"
                // is a problem's absence, and an absence that occupies a
                // permanent line teaches a reader to stop reading it.
                if unwell {
                    span { class: "unwell",
                        if matches!(snapshot.provider(), ProviderHealth::Unknown) {
                            "{word(crate::lang::Msg::CityUnwell)}"
                        } else {
                            "{word(snapshot.provider().word())}"
                        }
                    }
                }
                if let Some(told) = refused.clone() {
                    div { class: "refusal", role: "alert",
                        span { class: "refusal-code", "{told.code}" }
                        span { class: "refusal-what", "{told.what}" }
                        span { class: "refusal-way", "{told.recovery}" }
                        button {
                            class: "refusal-close",
                            "aria-label": "dismiss",
                            onclick: move |_| on_dismiss.call(()),
                            "\u{00d7}"
                        }
                    }
                }
                span { class: "counts",
                    for said in counts {
                        span { key: "{said}", "{said}" }
                    }
                }
            }
            nav { class: "left-nav",
                // Anchors, not buttons. Writing the fragment is the only
                // way a view changes, so an `<a href>` is already a whole
                // navigation - and it arrives with the keyboard, the
                // middle click, "copy link address" and the link role a
                // screen reader announces, none of which a button with an
                // onclick would have had.
                div { class: "nav-group",
                    for spot in spots {
                        a {
                            key: "{spot.label:?}",
                            class: "nav-item",
                            href: "{crate::route::to_fragment(&spot.view)}",
                            "aria-current": if showing(&spot.view, &view) { "page" } else { "false" },
                            "{word(spot.label)}"
                            if let Some(waiting) = spot.waiting {
                                span { class: "badge", "{waiting}" }
                            }
                        }
                    }
                }
                // What this whole city is doing, at the foot of the
                // column that names its parts. Stopping it left the top
                // bar because it stood beside the send button, which is
                // the one place a person's hand is already moving fast.
                div { class: "city-state",
                    p { class: "standing", "{word(standing_of(&snapshot))}" }
                    button {
                        class: "quiet",
                        r#type: "button",
                        onclick: move |_| on_frame.call(crate::command::halt(!snapshot.is_halted())),
                        if snapshot.is_halted() {
                            "{word(crate::lang::Msg::ReleaseCity)}"
                        } else {
                            "{word(crate::lang::Msg::HaltCity)}"
                        }
                    }
                }
            }
            section { class: "centre",
                match view {
                    // A run named by an old link, while the fold that
                    // says which room it is in has not arrived. Said
                    // rather than left blank: the link is not broken, the
                    // answer is not here yet.
                    View::Run(_) => rsx! {
                        crate::panel::Panel {
                            title: word(crate::lang::Msg::AskingWhatItHolds).to_owned(),
                            scope: None,
                            figure: None,
                            source: word(crate::lang::Msg::SessionSource).to_owned(),
                        }
                    },
                    View::Sessions => rsx! {
                        crate::sessions::SessionsView {
                            snapshot: snapshot.clone(),
                            city: city.clone(),
                            endpoints: endpoints.clone(),
                            effort: DEFAULT_EFFORT.to_owned(),
                            dropped: dropped.clone(),
                            live,
                            on_frame,
                            on_view,
                            on_drop,
                        }
                    },
                    View::Session(ref addr) => rsx! {
                        crate::session::SessionView {
                            addr: addr.clone(),
                            snapshot: snapshot.clone(),
                            records: records.clone(),
                            changes: changes.clone(),
                            cost: cost.clone(),
                            building: building.clone(),
                            steered: steered.clone(),
                            live,
                            on_frame,
                            on_drop,
                        }
                    },
                    View::Waiting => rsx! {
                        crate::waiting::WaitingView {
                            snapshot: snapshot.clone(),
                            live,
                            on_frame,
                        }
                    },
                    View::Record(lens) => rsx! {
                        crate::record::RecordView {
                            lens,
                            records: records.clone(),
                            hits: hits.clone(),
                            filed: filed.clone(),
                            discards: discards.clone(),
                            vitals: vitals.clone(),
                            live,
                            on_frame,
                        }
                    },
                    View::Cost => rsx! {
                        crate::dashboard::CostsView {
                            answer: cost.clone(),
                            usage: snapshot.usage(),
                            spent: snapshot.spent(),
                            live,
                            on_frame,
                        }
                    },
                    View::Setup => rsx! {
                        crate::settings::Settings {
                            answer: endpoints.clone(),
                            login_url: snapshot.login_url().map(str::to_owned),
                            served: snapshot.served().clone(),
                            live,
                            on_frame,
                        }
                    },
                    View::Building(ref addr) => rsx! {
                        crate::building_view::BuildingView {
                            addr: addr.clone(),
                            answer: building.clone(),
                            pursuits: crate::command::pursuits_of(city.as_ref()),
                            inbox: inbox.clone(),
                            signals: snapshot.signals_seen(),
                            live,
                            on_frame,
                            on_select,
                            on_drop,
                        }
                    },
                }
            }
        }
    }
}

/// How hard this city thinks when nobody has said otherwise.
///
/// The city's own ladder resolves effort per room, and this is only what
/// the composer offers before a person opens that word - so a wrong
/// guess here costs a click rather than a decision.
pub const DEFAULT_EFFORT: &str = "medium";

/// What the top bar calls the page being read.
///
/// The bar states this page, not this city: the city's name is true on
/// every page, and spending the one line that could say where you are on
/// something that never changes spends it on nothing.
#[must_use]
fn page_named(lang: crate::lang::Lang, view: &View) -> String {
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
fn standing_of(snapshot: &Snapshot) -> crate::lang::Msg {
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
    // A line a drop wrote into the session's box, held here for the same
    // reason `dropped` is: the box belongs to a view that a drop can
    // reach from outside it.
    // What the open session changed on disk. An answer, so it is held
    // beside the others and a reload asks again rather than trusting it.
    let changes = use_signal(|| None::<channels::ChangesAnswer>);
    let live = use_signal(|| false);
    // What the keyboard opened. Held here rather than inside `Root`
    // because the listener that sets them is registered once for the
    // window, and a page redraw must not take a reader's palette away.
    let mut palette = use_signal(|| false);
    let mut keymap = use_signal(|| false);
    listen_for_keys(Keyboard {
        chord: use_signal(crate::keys::Chord::default),
        palette,
        keymap,
        view,
        refused,
    });
    // The room the last dispatch asked for, so its run can be opened
    // when it starts rather than left for the person to find among the
    // others.
    // What the record page's ledger lens states about the whole history.
    let vitals = use_signal(|| None::<channels::MetricsAnswer>);
    // A line a drop wrote into an open session's box, on its way there.
    let mut steered = use_signal(|| None::<String>);
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
        changes,
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
            refused: refused(),
            records: records(),
            selected: selected(),
            dropped: dropped(),
            steered: steered(),
            vitals: vitals(),
            changes: changes(),
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
                    // The bar already knows where the work goes, because
                    // somebody put it there. Only the task is written.
                    crate::drop::Meaning::Task { task } => {
                        dropped.set(Some(task));
                    }
                    // Into the session's own box, unsent. The button is
                    // still the person's to press.
                    crate::drop::Meaning::Say { said, .. } => {
                        steered.set(Some(said));
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

            on_dismiss: move |()| refused.set(None),
        }
        if palette() {
            crate::palette::Palette {
                offers: reachable(&snapshot(), city().as_ref(), lang()),
                on_go: move |going: View| {
                    palette.set(false);
                    #[cfg(target_arch = "wasm32")]
                    crate::route::go(&going);
                    #[cfg(not(target_arch = "wasm32"))]
                    view.set(going);
                },
                on_close: move |()| palette.set(false),
            }
        }
        if keymap() {
            KeyMap { on_close: move |()| keymap.set(false) }
        }
    }
}

/// Everything the palette can reach, in the order a reader expects it.
///
/// Pages first because they are the answer most of the time, then the
/// buildings this city holds, then the sessions it knows of. Assembled
/// here because this is where the nav, the city answer and the run list
/// already meet; the palette holding its own list would be a second
/// answer to "where can a person go".
#[must_use]
fn reachable(
    snapshot: &Snapshot,
    city: Option<&channels::CityAnswer>,
    lang: crate::lang::Lang,
) -> Vec<crate::palette::Offer> {
    let mut offers: Vec<crate::palette::Offer> = destinations(snapshot)
        .into_iter()
        .map(|spot| crate::palette::Offer {
            label: crate::lang::say(lang, spot.label).to_owned(),
            kind: crate::palette::Kind::Page,
            going: spot.view,
        })
        .collect();
    if let Some(answer) = city {
        offers.extend(answer.buildings.iter().filter_map(|building| {
            let addr = Address::parse(building.addr.as_str()).ok()?;
            Some(crate::palette::Offer {
                label: building.addr.as_str().to_owned(),
                kind: crate::palette::Kind::Building,
                going: View::Building(addr),
            })
        }));
    }
    offers.extend(
        watchable(snapshot)
            .into_iter()
            .map(|(id, said)| crate::palette::Offer {
                label: said,
                kind: crate::palette::Kind::Session,
                going: View::Run(id),
            }),
    );
    offers
}

/// The key map, shown by the key that is hardest to guess.
///
/// A product whose shortcuts are undocumented has no shortcuts: nobody
/// tries a chord they have not been told about. This is the one page in
/// the client that exists to be read once.
#[component]
fn KeyMap(on_close: EventHandler<()>) -> Element {
    let lang = use_context::<Signal<crate::lang::Lang>>();
    let word = move |msg: Msg| crate::lang::say(lang(), msg);
    let rows = [
        ("Ctrl / \u{2318} + K", Msg::KeysPalette),
        ("Ctrl / \u{2318} + \u{21b5}", Msg::KeysCompose),
        ("Esc", Msg::KeysDismiss),
        ("g", Msg::KeysGo),
        ("?", Msg::KeysShow),
    ];
    rsx! {
        div { class: "palette-scrim", onclick: move |_| on_close.call(()),
            div {
                class: "keymap",
                onclick: move |event| event.stop_propagation(),
                h2 { "{word(Msg::KeysTitle)}" }
                p { class: "note", "{word(Msg::KeysScope)}" }
                dl {
                    for (chord, said) in rows {
                        div { key: "{chord}", class: "keymap-row",
                            dt { class: "chord", "{chord}" }
                            dd { "{word(said)}" }
                        }
                    }
                }
            }
        }
    }
}

/// Which buildings have a run in flight, folded from the snapshot rather
/// than asked of the server: the event stream already says it, and a
/// second question would be a second answer.
#[must_use]
pub(crate) fn busy_buildings(snapshot: &Snapshot) -> std::collections::BTreeSet<Address> {
    snapshot
        .runs()
        .filter(|(_, row)| matches!(row.phase, Phase::Running | Phase::Waiting))
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
    changes: Signal<Option<channels::ChangesAnswer>>,
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

/// What a keystroke may move. Bundled for the reason [`Wiring`] is: a
/// listener that took six handles would grow a seventh without anybody
/// noticing which of them it actually writes.
#[cfg_attr(
    not(target_arch = "wasm32"),
    expect(
        dead_code,
        reason = "the only reader is the browser's keydown listener"
    )
)]
#[derive(Clone, Copy)]
struct Keyboard {
    chord: Signal<crate::keys::Chord>,
    palette: Signal<bool>,
    keymap: Signal<bool>,
    view: Signal<View>,
    refused: Signal<Option<crate::alert::Refused>>,
}

/// Where the `g` sequence's second key goes.
///
/// Here rather than in `web::keys` because a `View` carries a run id and
/// an address, and a module that decides what a key means has no business
/// holding either.
#[cfg_attr(
    not(target_arch = "wasm32"),
    expect(
        dead_code,
        reason = "the only caller is the browser's keydown listener"
    )
)]
fn place_view(place: crate::keys::Place) -> View {
    match place {
        crate::keys::Place::Sessions => View::Sessions,
        crate::keys::Place::Waiting => View::Waiting,
        crate::keys::Place::Record => View::Record(Lens::Ledger),
        crate::keys::Place::Cost => View::Cost,
        crate::keys::Place::Setup => View::Setup,
    }
}

/// The one place a keystroke reaches this client.
///
/// On the window rather than on an element: a person who has clicked
/// nothing still has a keyboard, and a handler hung on the layout would
/// never see a key pressed while the body itself holds focus.
///
/// The browser contributes three facts and no judgement - which key,
/// whether the accelerator was down, and whether focus sits in something
/// the reader types into - and `web::keys` decides the rest, which is what
/// keeps the key map testable on the host.
#[cfg(target_arch = "wasm32")]
fn listen_for_keys(keyboard: Keyboard) {
    use dioxus::prelude::use_hook;
    use wasm_bindgen::JsCast as _;
    use_hook(move || {
        let Some(window) = web_sys::window() else {
            return;
        };
        let mut held = keyboard;
        let pressed = wasm_bindgen::closure::Closure::<dyn FnMut(web_sys::KeyboardEvent)>::new(
            move |event: web_sys::KeyboardEvent| {
                let key = event.key();
                let stroke = crate::keys::Stroke {
                    key: &key,
                    command: event.ctrl_key() || event.meta_key(),
                    in_text: typing_now(),
                };
                // `peek` rather than a read: this closure lives outside
                // the render that created it, and subscribing here would
                // tie a DOM listener to a reactive scope it outlives.
                let (next, act) = crate::keys::press(*held.chord.peek(), &stroke);
                held.chord.set(next);
                match act {
                    crate::keys::Act::Ignore => return,
                    crate::keys::Act::OpenPalette => {
                        held.keymap.set(false);
                        held.palette.set(true);
                    }
                    // One key closes whatever is open, outermost first, so
                    // a reader never has to know how deep they are.
                    crate::keys::Act::Dismiss => {
                        held.palette.set(false);
                        held.keymap.set(false);
                        held.refused.set(None);
                    }
                    crate::keys::Act::Compose => {
                        held.palette.set(false);
                        focus_where_work_starts();
                    }
                    crate::keys::Act::ShowKeys => {
                        held.keymap.set(true);
                    }
                    crate::keys::Act::Go(place) => {
                        held.palette.set(false);
                        held.keymap.set(false);
                        let going = place_view(place);
                        crate::route::go(&going);
                        held.view.set(going);
                    }
                }
                // Only what this client claimed: an ignored key belongs to
                // the browser, and taking it would break the reader's own
                // find-in-page and text entry.
                event.prevent_default();
            },
        );
        if window
            .add_event_listener_with_callback("keydown", pressed.as_ref().unchecked_ref())
            .is_ok()
        {
            // The page outlives the listener; dropping the closure here
            // would unregister the only thing that reads the keyboard.
            pressed.forget();
        }
    });
}

/// Whether focus sits in something the reader is writing into.
///
/// Without this, writing the word "goal" into the task box would navigate
/// away on its `g`.
#[cfg(target_arch = "wasm32")]
fn typing_now() -> bool {
    let Some(active) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.active_element())
    else {
        return false;
    };
    matches!(
        active.tag_name().to_ascii_uppercase().as_str(),
        "INPUT" | "TEXTAREA" | "SELECT"
    ) || active.has_attribute("contenteditable")
}

/// Puts the cursor in the box work is described in.
///
/// The discarded result follows `route::go`: a focus call that the
/// document refuses has no second thing to try, and the page is already
/// showing the field it failed to reach.
#[cfg(target_arch = "wasm32")]
fn focus_where_work_starts() {
    use wasm_bindgen::JsCast as _;
    if let Some(field) = web_sys::window()
        .and_then(|window| window.document())
        .and_then(|document| document.get_element_by_id("dispatch-task"))
        .and_then(|found| found.dyn_into::<web_sys::HtmlElement>().ok())
    {
        let _ = field.focus();
    }
}

/// Off the browser there is no keyboard to listen to.
#[cfg(not(target_arch = "wasm32"))]
fn listen_for_keys(_keyboard: Keyboard) {}

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
        changes,
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
                        changes,
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
                    crate::socket::LinkAction::Saying(delta) => {
                        buffer.push(crate::pace::Arrived::Saying(delta));
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

/// A snapshot holding the named sessions, folded from real records.
///
/// The production door and nothing beside it: every row here arrives
/// through [`Snapshot::apply`], so a test that seats a session is also
/// exercising the fold that seats one in a browser. A setter that wrote
/// into `runs` directly would let these tests pass while the fold was
/// broken, which is the failure they exist to catch.
#[cfg(test)]
pub(crate) fn seated(rows: &[(Option<&str>, Phase, u64)]) -> Snapshot {
    let mut snapshot = Snapshot::new();
    for (index, (addr, phase, seq)) in rows.iter().enumerate() {
        let mut run = [0u8; 16];
        // The index is what makes two rows two runs. Truncation cannot
        // happen below 256 rows and would only collide two fixtures.
        run[0] = u8::try_from(index).unwrap_or(u8::MAX);
        let id = RunId::from_bytes(run);
        snapshot.apply(&started(id, *addr, *seq));
        for record in ending(id, *phase, seq.saturating_add(1)) {
            snapshot.apply(&record);
        }
    }
    snapshot
}

/// A `model_returned` record carrying one text block, through the same
/// shape `runtime::turn` writes.
#[cfg(test)]
pub(crate) fn returned_for_test(run: RunId, said: &str, seq: u64) -> EventRecord {
    let mut data = serde_json::Map::new();
    data.insert(
        "message".to_owned(),
        serde_json::json!({ "content": [{ "kind": "text", "text": said }] }),
    );
    EventRecord::from_draft(
        channels::EventDraft {
            run,
            t: channels::TimeMs::new(1_000),
            who: "test".to_owned(),
            addr: None,
            kind: EventKind::ModelReturned,
            data: channels::Payload::new(data).unwrap_or_else(|_| channels::Payload::empty()),
            ig: false,
        },
        Seq::new(seq),
        channels::B3Hash::digest(b"prev"),
    )
}

#[cfg(test)]
fn started(run: RunId, addr: Option<&str>, seq: u64) -> EventRecord {
    EventRecord::from_draft(
        channels::EventDraft {
            run,
            t: channels::TimeMs::new(1_000),
            who: "test".to_owned(),
            addr: addr.and_then(|raw| Address::parse(raw).ok()),
            kind: EventKind::RunStarted,
            data: channels::Payload::empty(),
            ig: false,
        },
        Seq::new(seq),
        channels::B3Hash::digest(b"prev"),
    )
}

/// The records that put a run into the phase named, through the same
/// arms a live stream would take.
#[cfg(test)]
fn ending(run: RunId, phase: Phase, seq: u64) -> Vec<EventRecord> {
    let froze = |completion: &str| {
        let mut data = serde_json::Map::new();
        data.insert(
            "completion".to_owned(),
            serde_json::Value::String(completion.to_owned()),
        );
        vec![EventRecord::from_draft(
            channels::EventDraft {
                run,
                t: channels::TimeMs::new(1_000),
                who: "test".to_owned(),
                addr: None,
                kind: EventKind::RunFrozen,
                data: channels::Payload::new(data).unwrap_or_else(|_| channels::Payload::empty()),
                ig: false,
            },
            Seq::new(seq),
            channels::B3Hash::digest(b"prev"),
        )]
    };
    match phase {
        Phase::Running => Vec::new(),
        Phase::Frozen => froze("done"),
        Phase::Cancelled => froze("cancelled"),
        Phase::Waiting | Phase::Halted => {
            let kind = if phase == Phase::Waiting {
                EventKind::ApprovalRequested
            } else {
                EventKind::CityHalted
            };
            vec![EventRecord::from_draft(
                channels::EventDraft {
                    run,
                    t: channels::TimeMs::new(1_000),
                    who: "test".to_owned(),
                    addr: None,
                    kind,
                    data: channels::Payload::empty(),
                    ig: false,
                },
                Seq::new(seq),
                channels::B3Hash::digest(b"prev"),
            )]
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
            assert_eq!(row.phase, Phase::Halted);
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
        // A person opening this product is asking "is anything
        // happening, does any of it need me, and how do I start
        // something". One page answers all three, and it is the page the
        // bare fragment lands on.
        assert_eq!(View::default(), View::Sessions);
        assert_eq!(
            crate::route::from_fragment("#/"),
            Some(View::Sessions),
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
                steered: None,
                refused,
                records,
                selected: None,
                dropped: None,
                on_frame: move |_| {},
                on_select: move |_| {},
                on_drop: move |_| {},
                on_view: move |_| {},
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
            pursuits: Vec::new(),
            runs: Vec::new(),
            active: 0,
            frozen: 0,
            // A city with a building in it, because an empty city
            // exercises the empty state and nothing else.
            buildings: vec![channels::BuildingProgress {
                blocked: Vec::new(),
                ready: 0,
                addr: Address::parse("lab").unwrap(),
                progress: channels::Progress::Planned(channels::PlannedProgress {
                    done: 1,
                    blocked: 0,
                    total: 4,
                    done_ppb: 0,
                    blocked_ppb: 0,
                }),
                problems: Vec::new(),
            }],
        }
    }

    fn building_answer() -> channels::BuildingAnswer {
        channels::BuildingAnswer {
            plan: Vec::new(),
            blocked: Vec::new(),
            sandbox: None,
            mcp: Vec::new(),
            addr: Address::parse("lab").unwrap(),
            progress: channels::Progress::Planned(channels::PlannedProgress {
                done: 1,
                blocked: 0,
                total: 4,
                done_ppb: 0,
                blocked_ppb: 0,
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
            // The box work starts in, and the table its rows land in.
            // If either renders nothing, the first screen is a blank one.
            View::Sessions => vec![
                ("composer", "what needs doing?"),
                ("composer-plan", "send it to"),
            ],
            // The head's four facts, and the tabs under them. The third
            // fact is the em rule, which is asserted in `web::session`
            // against a value rather than against markup.
            View::Session(_) => vec![("session-head", "all sessions"), ("session-tabs", "turns")],
            View::Waiting => vec![("approvals", "push to the remote")],
            // The lens switch, because the record is one page with three
            // readings and a page that cannot change lens is one reading.
            View::Record(lens) => match lens {
                Lens::Ledger => vec![
                    ("session-tabs", "the archive"),
                    ("ledger", "sprawling replay"),
                ],
                Lens::Archive => vec![
                    ("session-tabs", "the ledger"),
                    ("archive-search", "filed lately"),
                ],
                Lens::Bin => vec![
                    ("session-tabs", "the ledger"),
                    ("recycle-bin", "the way back to each of it"),
                ],
            },
            View::Cost => vec![("dashboard", "exec")],
            View::Setup => vec![
                ("settings", "put the key in the vault"),
                ("subscription", "start the login"),
            ],
            // The room tab, because a room is a face of a building and
            // the page lists them in exactly one place.
            View::Building(_) => vec![("building", "Roadmap.md"), ("room", "room1/")],
            // An old link, before the fold that names its room arrives.
            View::Run(_) => vec![("panel", "asking the city what it holds")],
        }
    }

    #[test]
    fn every_destination_in_the_nav_reaches_a_page_that_shows_something() {
        // The defect this pins down: four of the six views rendered an
        // empty `div` and the nav had no links at all, while every
        // module test stayed green because it called the module's pure
        // functions directly. A page is not mounted until the tree says
        // so, and this is the only test in the crate that asks the tree.
        let mut snapshot = seated(&[(Some("lab/room1"), Phase::Running, 1)]);
        snapshot.adopt_approvals(vec![waiting_item()]);
        let records = vec![record(2, EventKind::ToolCalled, [0u8; 16])];

        let every_view = [
            View::Sessions,
            View::Session(Address::parse("lab/room1").unwrap()),
            View::Waiting,
            View::Record(Lens::Ledger),
            View::Record(Lens::Archive),
            View::Record(Lens::Bin),
            View::Cost,
            View::Setup,
            View::Building(Address::parse("lab").unwrap()),
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
        let painted = paint(View::Cost, Snapshot::new(), Vec::new());
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
    fn every_verb_this_client_offers_is_one_the_city_executes() {
        // The rule this asserts is the one the audit produced: a control
        // whose command reaches `assembly`'s catch-all can only ever
        // produce a refusal, and a button that cannot succeed is worse
        // than a missing one. Takeover, Rollback and CreatePolicy were
        // all offered and all unexecuted.
        let snapshot = seated(&[(Some("lab/room1"), Phase::Running, 1)]);
        let session = paint(
            View::Session(Address::parse("lab/room1").unwrap()),
            snapshot.clone(),
            vec![record(2, EventKind::ToolCalled, [0u8; 16])],
        );
        assert!(session.says("branch a new run from step"), "Fork");
        assert!(session.says("send at the next safe point"), "Steer");
        assert!(session.says("stop this session"), "Cancel");
        assert!(
            !session.says("answer for this run from here"),
            "Takeover has no executor, so it may not be offered"
        );
        let waiting = paint(View::Waiting, snapshot, Vec::new());
        assert!(
            !waiting.says("and stop asking"),
            "CreatePolicy has no executor, so it may not be offered"
        );
    }

    #[test]
    fn the_city_is_drawn_as_shapes_a_person_can_reach() {
        // What a canvas could not be asked. Before F2.02 the drawing
        // existed only on wasm, so no host test could see whether the
        // picture had been drawn at all - which is how it once shipped
        // painting the ground and no buildings.
        let painted = paint(View::Sessions, Snapshot::new(), Vec::new());
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
        let mut snapshot = seated(&[(Some("lab/room1"), Phase::Running, 1)]);
        snapshot.adopt_approvals(vec![waiting_item()]);
        for view in [
            View::Sessions,
            View::Session(Address::parse("lab/room1").unwrap()),
            View::Waiting,
            View::Record(Lens::Ledger),
            View::Record(Lens::Archive),
            View::Record(Lens::Bin),
            View::Cost,
            View::Setup,
            View::Building(Address::parse("lab").unwrap()),
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
            paint(View::Setup, snapshot.clone(), Vec::new()).says("no login is waiting"),
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
        let painted = paint(View::Setup, snapshot.clone(), Vec::new());
        assert!(
            painted.says("https://example.invalid/authorize?state=x"),
            "the url a person must open is the one the server recorded"
        );
        assert!(painted.says("finish the login"));

        snapshot.apply(&record(4, EventKind::SecretCaptured, [2u8; 16]));
        assert!(
            paint(View::Setup, snapshot, Vec::new()).says("no login is waiting"),
            "a credential in the vault ends the step that was asking for it"
        );
    }

    #[test]
    fn the_left_nav_carries_every_destination_and_says_how_many_wait() {
        let mut snapshot = Snapshot::new();
        snapshot.adopt_approvals(vec![waiting_item()]);
        let painted = paint(View::Sessions, snapshot.clone(), Vec::new());
        // In the language the harness renders in, which is this
        // client's own: what matters here is that every destination
        // reaches the page, and `lang` holds that both languages exist.
        let word = |msg| crate::lang::say(crate::lang::Lang::En, msg);
        for spot in destinations(&snapshot) {
            assert!(
                painted.says(word(spot.label)),
                "the nav does not offer {:?}",
                spot.label
            );
        }
        assert!(
            painted.has_class("badge"),
            "one item waits and none is shown"
        );
    }

    /// The bar a person writes work into is a drop target.
    ///
    /// Before this it was not, and the browser's own default was
    /// answering the gesture instead: a bare text input accepts a
    /// `text/plain` drop without anybody electing it, so a dragged
    /// selection went in raw and `drop::read` never ran. Cancelling
    /// `dragover` is what takes the gesture back.
    #[test]
    fn work_can_be_aimed_by_dropping_onto_the_box_it_is_written_in() {
        let painted = paint(View::Sessions, Snapshot::new(), Vec::new());
        assert!(
            painted.has_class("composer-task"),
            "the box work is written in is not on the page: {:?}",
            painted.classes
        );
        assert!(
            painted
                .attrs
                .iter()
                .any(|value| value.contains("measure every read path")),
            "the placeholder is not one whole real task, so it teaches nothing about size"
        );
    }

    /// Every drop zone must be able to say "a drag is over me" without a
    /// hover rule, because device input events are suppressed for the
    /// whole of a drag and a hover rule therefore never lights.
    #[test]
    fn a_drop_zone_reports_a_drag_through_events_and_not_through_hover() {
        let source = include_str!("../assets/app.css");
        assert!(
            source.contains(".drop-zone.over"),
            "a drop zone has no drag state to show"
        );
        // Read at compile time, so this does not depend on which
        // directory the runner happened to start in.
        for (name, wired) in [
            ("app.rs", include_str!("app.rs")),
            ("live.rs", include_str!("live.rs")),
            ("building_view.rs", include_str!("building_view.rs")),
        ] {
            assert!(
                wired.contains("ondragenter") && wired.contains("ondragleave"),
                "{name} carries a drop zone it never lights"
            );
        }
    }

    #[test]
    fn the_box_that_starts_work_asks_for_work_and_not_for_a_budget() {
        // A person cannot say what a task is worth before it runs, and a
        // subscription has no unit price to say it in (user verdict,
        // 2026-08-22). Whatever the box shows, it never shows a price.
        let painted = paint(View::Sessions, Snapshot::new(), Vec::new());
        assert!(painted.has_class("panel composer"));
        assert!(
            !painted.says("budget") && !painted.says("how much"),
            "the box that starts work asks for money: {:?} / {:?}",
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
        let painted = painted_with(View::Sessions, Snapshot::new(), Vec::new(), Some(told));
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
        let painted = painted_with(View::Sessions, Snapshot::new(), Vec::new(), None);
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
        let _run = latest_run(&snapshot).expect("one run started");

        let offered = watchable(&snapshot);
        let (_, label) = offered.first().expect("the run is offered");
        assert!(
            label.contains("refactor-the-ledger"),
            "the picker does not name the session: {label}"
        );

        let painted = paint(
            View::Session(Address::parse("lab/refactor-the-ledger").unwrap()),
            snapshot,
            Vec::new(),
        );
        assert!(
            painted.says("refactor-the-ledger"),
            "the page being read does not say which session it is"
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
        let painted = paint(View::Setup, Snapshot::new(), Vec::new());
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

    /// At rest the box asks one question, and everything else it needs
    /// is written out as a sentence a person can disagree with.
    ///
    /// The bar this replaced stood seven controls open at once, which
    /// asked a person to read the whole grammar of a dispatch before
    /// writing one word of it.
    #[test]
    fn at_rest_the_box_is_one_field_and_one_sentence() {
        let painted = paint(View::Sessions, Snapshot::new(), Vec::new());
        let fields = painted
            .classes
            .iter()
            .filter(|held| *held == "composer-task" || *held == "composer-field")
            .count();
        assert_eq!(fields, 1, "more than one control stands open at rest");
        for word in ["send it to ", "as ", "think "] {
            assert!(
                painted.text.iter().any(|line| line == word),
                "the inferred sentence does not say {word:?}: {:?}",
                painted.text
            );
        }
    }

    /// Nothing is hidden and nothing is asked.
    ///
    /// The replaced bar folded four controls behind a "more" disclosure,
    /// which is the same defect wearing a control: a page that hides what
    /// it decided is a page answering on the reader's behalf. Every
    /// decision is on screen, as a word that can be clicked.
    #[test]
    fn every_decision_the_city_made_is_on_screen_and_can_be_changed() {
        let painted = paint(View::Sessions, Snapshot::new(), Vec::new());
        assert!(
            !painted.text.iter().any(|line| line == "more"),
            "something is folded away: {:?}",
            painted.text
        );
        let words = painted
            .classes
            .iter()
            .filter(|held| *held == "guess" || *held == "chosen")
            .count();
        assert_eq!(words, 3, "the sentence does not offer all three decisions");
    }

    /// Stopping is not the same kind of act as starting, and it used to
    /// sit against the button a person's hand is already moving towards.
    ///
    /// Only the dress is asserted here. Where the control sits is a fact
    /// about the document, and this harness reads one template at a time
    /// in the order the differ loads them, so an index taken from the top
    /// bar cannot be compared with one taken from the control surface -
    /// the placement was checked by looking at the running client.
    #[test]
    fn stopping_the_city_is_never_dressed_as_the_thing_that_starts_work() {
        let painted = paint(View::Sessions, Snapshot::new(), Vec::new());
        assert!(painted.says("stop the city"), "the city cannot be stopped");
        assert!(
            painted.has_class("quiet"),
            "the halt control is dressed as a primary action: {:?}",
            painted.classes
        );
        assert!(
            painted.has_class("city-state"),
            "stopping the city stands away from the box that starts work"
        );
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
    changes: Signal<Option<channels::ChangesAnswer>>,
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
        mut changes,
        mut records,
        mut view,
        mut expecting,
        mut refused,
        lang,
    } = wiring;
    let (events, answers, refusal, saying) = paint.into_parts();
    let said = lang();
    // Increments first, and into the snapshot's own discardable buffer.
    // Before the events on purpose: `model_returned` in this same burst
    // throws the buffer away, so a call that both streamed and settled
    // inside one frame ends with the record showing rather than the
    // increments that preceded it.
    if !saying.is_empty() {
        snapshot.with_mut(|held| {
            for delta in &saying {
                held.is_saying(delta);
            }
        });
    }
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
            && started_here(&event, &waiting).is_some()
            && let Some(addr) = event.addr().cloned()
        {
            // The room, not the run: a session opened by this person is
            // named by the name they gave it, and that is the address
            // this build puts in the bar.
            expecting.set(None);
            view.set(View::Session(addr.clone()));
            crate::route::go(&View::Session(addr));
        }
        if snapshot.write().apply(&event) {
            keep.push(event);
        }
    }
    // Which session the person has open, so a store at its bound gives
    // way in what they are not reading rather than in what they are.
    let reading = match &*view.read() {
        View::Session(addr) => snapshot.read().session_at(addr).map(|(run, _)| run),
        View::Run(run) => Some(*run),
        _ => None,
    };
    if !keep.is_empty() {
        // Kept once, read by every page that reads history, and bounded by
        // the one function that answers "how much does a tab hold".
        hold(&mut records.write(), keep, reading);
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
            channels::Answer::Changes(held) => changes.set(Some(held)),
            // What happened before this tab opened. Folded into the
            // snapshot and kept for the pages that read history, in the
            // same bounded store the live stream fills - one answer to
            // "how much does a tab hold".
            // What happened before this tab opened, from either of the
            // two questions that ask it: the city's own slice at connect,
            // and one session's when its page opens. The backfill is
            // forward-only and refuses the second, which is correct - a
            // snapshot already folded past these must not be walked
            // back - but the records themselves are still what the
            // session page reads, so they are kept either way.
            channels::Answer::History(held) => {
                snapshot.write().backfill(&held.records);
                hold(&mut records.write(), held.records, reading);
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
