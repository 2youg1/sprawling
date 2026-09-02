// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! What a Query comes back as: one shape per view, and the closed set
//! of them.
//!
//! These are read shapes and nothing else. Every one is derived from the
//! city's own state by a projection above this crate, so nothing here
//! decides anything; what this module owns is that a page and a server
//! agree on the field names, and that `Unavailable` is a real answer —
//! a view this build does not evaluate yet says so by name rather than
//! returning an empty result a reader would mistake for an empty city.

use kernel::{
    Address, ApprovalItem, DialectKind, EventKind, EventRecord, FileChange, GitOid, McpServer,
    ModelTag, NodeId, Progress, PursuitState, Restoration, RoadmapStatus, RunId, SandboxLimits,
    Seq, TimeMs, UsdMicros,
};
use serde::{Deserialize, Serialize};

/// A slice of the one history, oldest first.
///
/// Oldest first because that is the order the ledger wrote them and the
/// order a fold expects; a reader that wants the newest first reverses a
/// list it already has, and a server that reversed it would make the
/// fold the caller's problem.
// No `Eq`: an `EventRecord` carries a payload whose numbers may be
// floats, and the wire's other answers derive it only because none of
// them holds one.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HistoryAnswer {
    pub records: Vec<EventRecord>,
    /// Where to ask next to go further back. `None` means this slice
    /// reaches the first record the city ever wrote.
    pub earlier: Option<Seq>,
}

/// What moved between two checkpoints, one row per file, path order.
///
/// Path order rather than size order: somebody looking for one file finds
/// it in the same place every time, and a list that reorders itself as
/// the numbers change cannot be scanned twice.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChangesAnswer {
    pub base: GitOid,
    /// Absent when the comparison ran against the working tree.
    pub head: Option<GitOid>,
    pub files: Vec<FileChange>,
}

/// The most records one `History` answer may carry. A page asking for
/// more gets this many; the whole ledger is not a thing to put on a
/// socket, and a limit the caller cannot exceed is one fewer way for a
/// client to make the server do unbounded work.
pub const HISTORY_MAX: u32 = 500;

/// One run, as a reader needs it. The client folds the live stream for
/// itself; this shape is what a query answers about runs it never saw,
/// which is why it carries the position rather than the whole history.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunSummary {
    pub run: RunId,
    pub who: String,
    pub frozen: bool,
    pub last_seq: Seq,
    pub last_kind: EventKind,
}

/// What the settings page reads back: what is attached, and what each
/// tag currently points at.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointsAnswer {
    pub endpoints: Vec<EndpointSummary>,
    pub chosen: Vec<ChosenSummary>,
}

/// One attached endpoint, as a reader may see it. No credential appears
/// here in any form; `has_credential` answers the only question a page
/// needs, which is whether one was enrolled at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EndpointSummary {
    pub name: String,
    pub base_url: String,
    pub dialect: DialectKind,
    pub models: Vec<String>,
    /// Whether calls to it stay on this machine, which is the only thing
    /// a confidential building may use.
    pub local: bool,
    pub has_credential: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ChosenSummary {
    pub tag: ModelTag,
    pub endpoint: String,
    pub model: String,
    pub max_output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CityAnswer {
    pub runs: Vec<RunSummary>,
    pub active: u64,
    pub frozen: u64,
    /// One entry per building whose roadmap the city could read.
    pub buildings: Vec<BuildingProgress>,
    /// The standing goals this city is working towards, if any.
    pub pursuits: Vec<PursuitLine>,
}

/// A building's plan, as its own `Roadmap.md` states it.
///
/// `problems` carries the rows the table could not state — a row with a
/// status outside the five words, a column count that is not six, or a
/// dependency that runs in a circle. The interface shows them: a plan
/// that quietly drops the lines it could not parse would report progress
/// against a denominator nobody chose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildingProgress {
    pub addr: Address,
    pub progress: Progress,
    pub problems: Vec<String>,
    /// What is stuck, one line per cause. Never one per symptom: a plan
    /// with one real problem sends one line, and the seventeen nodes
    /// waiting behind it are a count on that line rather than seventeen
    /// more of them.
    pub blocked: Vec<BlockedLine>,
    /// How many nodes could be started right now. The number that says
    /// whether a city with a standing goal has anything left to do.
    pub ready: u32,
}

/// One cause, and how much is waiting behind it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlockedLine {
    /// The node it is stuck at.
    pub source: NodeId,
    /// The whole sentence: which branch, which node, and why.
    pub line: String,
    /// How many other nodes cannot move until this one does.
    pub waiting: u32,
}

/// One node of a building's plan, flattened for a renderer.
///
/// The tree travels as rows in reading order rather than as nested
/// objects: every face the client draws — the list, the board, a branch
/// summary — wants a different grouping, and a shape that favoured one
/// of them would make the others re-flatten it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanRow {
    pub node: NodeId,
    pub item: String,
    pub status: RoadmapStatus,
    /// This node's share of the whole plan, in billionths.
    pub share_ppb: u64,
    pub needs: Vec<NodeId>,
    /// Whether a run could take it right now.
    pub ready: bool,
    /// Whether it carries work of its own. Only leaves are counted, so a
    /// renderer that showed branches in the same column as leaves would
    /// be showing the same work twice.
    pub leaf: bool,
    pub evidence: Option<String>,
}

/// A city's standing goal, if it has one.
///
/// `verdict` is the city's own reading of whether there is anything left
/// to do, so a page never has to work out the stop condition for itself
/// — which is what would give the condition a second authority.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PursuitLine {
    pub addr: Address,
    pub goal: String,
    pub state: PursuitState,
    /// One clause: working on 2.3, waiting for two runs, paused, or
    /// finished.
    pub verdict: String,
}

/// What is waiting for a person, as the Ledger recorded it.
///
/// The whole item travels rather than a summary of it. An interface that
/// groups identical questions needs the cluster key, and one that leads
/// with the longest wait needs the arrival time; a summary type that
/// dropped both forced the page to render every item as its own group
/// under a sentence nobody wrote. `tainted` needs no special carriage
/// here for the same reason - it is a field of the item itself.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalsAnswer {
    pub items: Vec<ApprovalItem>,
}

/// One document of a building, as it stands on disk.
///
/// The text is bounded: these files are written by agents over months,
/// and a page that ships an unbounded file has no answer for the day one
/// of them reaches a hundred megabytes. When it is cut, it says so -
/// silence about a cut is the difference between a view and a lie.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildingDoc {
    pub name: String,
    pub text: String,
    pub bytes: u64,
    pub truncated: bool,
}

/// One line of a building's archive index.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveLine {
    pub kind: String,
    /// Whole days since the epoch, as the archive files them.
    pub day: u64,
    pub subject: String,
}

/// What one building is: its plan, its own documents, its rooms and what
/// it has filed. The answer a person reads when they ask "what happened
/// in there".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildingAnswer {
    pub addr: Address,
    pub progress: Progress,
    /// Rows of the plan this build could not read. Shown, never dropped.
    pub problems: Vec<String>,
    /// The plan tree, in reading order. Empty when the plan does not
    /// parse, which `problems` then explains.
    pub plan: Vec<PlanRow>,
    /// What is stuck, one line per cause.
    pub blocked: Vec<BlockedLine>,
    pub rooms: Vec<String>,
    pub docs: Vec<BuildingDoc>,
    pub archive: Vec<ArchiveLine>,
    /// What this building's own layer states its runs may reach. The
    /// resolved value is the ladder's; this is the rung a person edits,
    /// so a form that showed the resolved value would silently rewrite
    /// what a city-wide setting had said.
    pub sandbox: Option<SandboxLimits>,
    pub mcp: Vec<McpServer>,
}

/// The five cuts of one authoritative total. Each dimension sums to
/// `total` exactly; the interface renders shares against `total` rather
/// than normalising its own rows, so an unattributed remainder stays
/// visible instead of being divided away.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CostAnswer {
    pub total: UsdMicros,
    pub by_run: Vec<(String, UsdMicros)>,
    pub by_actor: Vec<(String, UsdMicros)>,
    pub by_segment: Vec<(String, UsdMicros)>,
    pub by_tool: Vec<(String, UsdMicros)>,
    pub by_skill: Vec<(String, UsdMicros)>,
}

/// What a query returns. `Unavailable` is a real answer: a view this
/// build does not evaluate yet says so by name, rather than returning an
/// empty result a reader would mistake for an empty city.
///
/// `Eq` went when `History` arrived: a record's payload is arbitrary
/// JSON, and JSON has no total equality. Nothing compared two answers
/// for equality outside a test.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Answer {
    History(Box<HistoryAnswer>),
    Changes(ChangesAnswer),
    City(CityAnswer),
    Run(Option<RunSummary>),
    Approvals(ApprovalsAnswer),
    Cost(Box<CostAnswer>),
    Endpoints(EndpointsAnswer),
    Building(Box<BuildingAnswer>),
    Inbox(InboxAnswer),
    Discards(DiscardAnswer),
    Registry(RegistryAnswer),
    Archive(ArchiveAnswer),
    Metrics(Box<MetricsAnswer>),
    Unavailable { query: String },
}

/// What waits in one room, without taking it.
///
/// Folded from the ledger rather than read off the queue: a queue that
/// is read by being consumed cannot also be looked at, and a view that
/// consumed what it showed would be a view that changes the thing it
/// reports on.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct InboxAnswer {
    pub addr: Address,
    pub waiting: Vec<SignalLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SignalLine {
    pub id: String,
    pub kind: String,
    pub from: String,
    pub at: TimeMs,
}

/// The Recycle Bin: what was discarded and how each row gets back.
///
/// A row without a way back cannot be constructed upstream, so every
/// row here states one; `restored` says whether somebody already took
/// it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscardAnswer {
    pub rows: Vec<DiscardLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct DiscardLine {
    pub path: String,
    /// The way back, as the record kept it - the plan itself, not a
    /// sentence about it.
    ///
    /// Composing the sentence is the interface's job and it already has
    /// one authority for it (`web::approval::ReturnPath`); a server that
    /// also rendered the plan into words would be the second. `None`
    /// means the record names a scheme this build cannot read, which the
    /// interface shows as a row it will not invent an action for - the
    /// row is never dropped, because hiding a discarded thing is worse
    /// than admitting the plan is unreadable.
    pub restoration: Option<Restoration>,
    pub at: TimeMs,
    pub restored: bool,
}

/// What this city has decided is worth keeping.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryAnswer {
    pub assets: Vec<RegistryLine>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RegistryLine {
    pub addr: Address,
    pub kind: String,
    pub subject: String,
    pub at: TimeMs,
}

/// Archive hits across every building, read from the shelves at the
/// moment of asking. The files are the authority; an index kept beside
/// them would be a second copy of what the disk says.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveAnswer {
    pub needle: String,
    pub hits: Vec<ArchiveHit>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ArchiveHit {
    pub building: Address,
    pub kind: String,
    pub day: u64,
    pub subject: String,
}

/// The city's vital signs: the counts a page would otherwise assemble
/// by asking four questions and adding up the answers.
///
/// Every number here is already proven by another view; this query
/// exists so that drawing one readout costs one question. It holds no
/// money - that is `CostView`'s, and one figure with two owners is how
/// two figures start disagreeing.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetricsAnswer {
    pub events: u64,
    pub runs_active: u64,
    pub runs_frozen: u64,
    pub buildings: u64,
    pub approvals_waiting: u64,
    pub signals_waiting: u64,
    /// Discarded and not yet restored.
    pub discards_outstanding: u64,
}
