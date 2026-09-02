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
    /// What the person asked for, as they typed it.
    ///
    /// Folded from `run_started`, where it has been on the wire all
    /// along and no page read it. A session page that shows what a run
    /// spent, which door holds it and what it last said, and never the
    /// sentence it was given, is asking a reader to judge an answer
    /// without the question - and the question is the one thing on that
    /// page a person wrote themselves.
    pub task: Option<String>,
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
                        task: event
                            .data()
                            .as_map()
                            .get("task")
                            .and_then(serde_json::Value::as_str)
                            .map(str::to_owned)
                            .filter(|asked| !asked.trim().is_empty()),
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

/// What became of a backfill. Exhaustive, because "nothing was folded"
/// and "this page was already live" are different facts and only one of
/// them is worth saying anything about.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Backfill {
    Folded(usize),
    AlreadyLive,
}

/// One event, as a stream would deliver it.
#[cfg(test)]
pub(crate) fn record(seq: u64, kind: EventKind, run: [u8; 16]) -> EventRecord {
    EventRecord::from_draft(
        kernel_draft(kind, run),
        Seq::new(seq),
        channels::B3Hash::digest(b"prev"),
    )
}

/// The draft behind [`record`], separated because two tests want to set
/// a payload on it before it is sealed into a record.
#[cfg(test)]
pub(crate) fn kernel_draft(kind: EventKind, run: [u8; 16]) -> channels::EventDraft {
    channels::EventDraft {
        run: RunId::from_bytes(run),
        t: channels::TimeMs::new(1_000),
        who: "test".to_owned(),
        addr: None,
        kind,
        data: channels::Payload::empty(),
        ig: false,
    }
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
}
