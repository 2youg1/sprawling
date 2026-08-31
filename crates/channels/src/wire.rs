// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The process boundary's vocabulary: twenty Commands, eleven Queries, and
//! the Event push. Encoding is JSON because the receiving
//! end is a browser and a human reading a network panel is a design goal.
//!
//! Two invariants live in the type system rather than in a check:
//!
//! - `Command` is generic over the carrier of a secret. `WireCommand` fixes
//!   that carrier to an uninhabited type, so a frame arriving from a socket
//!   cannot be a `PutSecret` - not "is rejected", but has no representation.
//!   Credentials are enrolled on the host machine, and that constraint is
//!   held by construction rather than by a check.
//! - Every state-changing Command owns an `IdemKey` field. There is no
//!   constructor that omits it, so "double-clicking twice opens two Runs" is
//!   not reachable from this type.
//!
//! Names of things this crate does not own - modes, providers, templates -
//! travel as validated newtypes with no closed value list. The authority for
//! which values are legal stays upstream (`runtime::Mode`, gateway, city);
//! the mapping point is the assembly layer, and an unknown value is an error
//! there, never a guess.

use kernel::{
    Address, ApprovalId, ApprovalItem, Autonomy, AxCode, AxError, B3Hash, BudgetCap, DialectKind,
    Effort, EventKind, EventRecord, FileChange, GitOid, IdemKey, McpServer, ModelTag,
    PolicyVerdict, Progress, Restoration, RunId, SandboxLimits, Sealed, Seq, SessionName, TimeMs,
    UsdMicros,
};
use serde::{Deserialize, Serialize};

/// Wire format version. Bumped whenever the frame grammar changes shape in a
/// way the schema hash alone would not explain to a human reading a log.
///
/// 5: `Dispatch` carries the name of the session it starts (F2.11).
/// 6: and how hard that session thinks (F2.16).
/// 7: a provider can be asked what it serves before it is attached, and
///    an attachment names which of those models it admits (P3.01).
/// 8: a building's sandbox limits and external servers have a surface
///    (P3.02).
/// 9: a page can ask for the history that happened before it opened
///    (P3.04).
pub const WIRE_V: u32 = 11;

/// The Command surface, in declaration order.
/// This table feeds [`schema_hash`]; a connection whose peer computes a
/// different hash is refused rather than served a half-understood protocol.
pub const COMMAND_NAMES: [&str; 22] = [
    "Dispatch",
    "Wake",
    "Login",
    "ProbeEndpoint",
    "AttachEndpoint",
    "SelectModel",
    "Fork",
    "Attach",
    "CreateBuilding",
    "ConfigureBuilding",
    "PutSecret",
    "Steer",
    "Cancel",
    "Takeover",
    "Rollback",
    "Halt",
    "Release",
    "BatchByBuilding",
    "Approve",
    "CreatePolicy",
    "SetAutonomy",
    "Auth",
];

/// The Query surface, in declaration order.
pub const QUERY_NAMES: [&str; 14] = [
    "History",
    "RunHistory",
    "Changes",
    "RunView",
    "CityView",
    "ApprovalQueue",
    "InboxView",
    "Metrics",
    "CostView",
    "ArchiveSearch",
    "RegistryView",
    "DiscardView",
    "EndpointView",
    "BuildingView",
];

/// Uninhabited on purpose. A value of this type cannot be produced, so
/// `Command<NoSecret>` has no reachable `PutSecret` variant. This is the
/// compile-time half of "a remote connection cannot spell that frame";
/// `Deserialize` supplies the runtime half for bytes that try anyway.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoSecret {}

impl Serialize for NoSecret {
    fn serialize<S: serde::Serializer>(&self, _serializer: S) -> Result<S::Ok, S::Error> {
        match *self {}
    }
}

impl<'de> Deserialize<'de> for NoSecret {
    fn deserialize<D: serde::Deserializer<'de>>(_deserializer: D) -> Result<Self, D::Error> {
        Err(serde::de::Error::custom(
            "a credential cannot be enrolled over a connection; enrol it on the host",
        ))
    }
}

macro_rules! carried_name {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Sole constructor. Rejects empty and control characters only:
            /// the legal value set belongs to the upstream authority, and a
            /// second copy of it here would be a second authority.
            pub fn parse(raw: &str) -> Result<Self, AxError> {
                if raw.is_empty() || raw.chars().any(char::is_control) {
                    return Err(AxError::failure(
                        AxCode::WireMismatch,
                        concat!("read ", stringify!($name), " from a frame"),
                        "the field is empty or holds control characters",
                    )
                    .with_recovery("send a non-empty single-line value"));
                }
                Ok(Self(raw.to_owned()))
            }

            /// The carried text. Whether it names something that exists is
            /// answered upstream, not here.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

carried_name!(
    ModeTag,
    "A mode name in transit. Authority for the mode set is `runtime::Mode`."
);
carried_name!(
    ProviderName,
    "A provider name in transit. Authority for the provider set is `gateway`."
);
carried_name!(
    TemplateName,
    "A Building template name in transit. Authority is `city`."
);
carried_name!(
    UploadId,
    "Handle for bytes already delivered to the upload endpoint."
);

/// Which step of a subscription login a `Login` frame carries.
///
/// The authorization code arrives by hand: the provider shows it to the
/// person after they approve, and the person brings it back. That is
/// the flow the profile table describes, and it needs no listening port
/// of its own.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LoginStep {
    /// Mint the authorization URL for a person to open.
    Begin,
    /// Redeem the code that person brought back.
    Code { code: String },
}

/// What a Halt, Release or Autonomy change applies to. Unlike modes and
/// providers, this set is the protocol's own and has no upstream owner.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HaltScope {
    City,
    Building(Address),
    Workshop(Address),
}

/// Commands change state, require authorization, and are idempotent.
///
/// Deliberately *not* `#[non_exhaustive]`: the schema hash is this type's
/// version mechanism, so the assembly layer must handle all eighteen and a
/// new one fails to compile until somebody decides what it does.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Command<Secret = Sealed<String>> {
    Dispatch {
        addr: Address,
        task: String,
        goal: String,
        mode: ModeTag,
        budget: BudgetCap,
        idem: IdemKey,
        /// What this session is called, when it is a new one.
        ///
        /// `Some` means `addr` names a building and the city opens a
        /// room of this name under it; `None` means `addr` already
        /// names the room, which is how an earlier session is
        /// continued. Two dispatches to one room are one session with a
        /// history, and that is a different thing from two sessions
        /// sharing a folder.
        session: Option<SessionName>,
        /// How hard the model is asked to think in this session.
        ///
        /// `None` leaves the layer above to answer, which is the city's
        /// own configuration and, failing that, the provider's default.
        /// A value is written into the session's own configuration when
        /// its room is opened, so it is chosen once and holds for every
        /// run in that room (city-SPEC.md section 8-14).
        effort: Option<Effort>,
    },
    /// One step of a subscription login. Which step is named rather
    /// than inferred: beginning and redeeming are different actions
    /// with different failure modes, and a page that means one must not
    /// be readable as the other.
    Login {
        provider: ProviderName,
        step: LoginStep,
        idem: IdemKey,
    },
    /// Register a provider the person just entered. The credential is
    /// already in the vault by the time this frame exists: what travels
    /// here is the reference to it, which is why this command has a byte
    /// form and `PutSecret` does not.
    /// Asks a base URL what it serves, and attaches nothing.
    ///
    /// A person cannot choose from a list they have not seen, and the
    /// list used to arrive only as a side effect of attaching - so
    /// looking at what a key buys meant registering it first. The answer
    /// lands as `endpoint_probed`, which the page folds like any other
    /// fact about this city.
    ProbeEndpoint {
        name: ProviderName,
        base_url: String,
        dialect: DialectKind,
        secret: Option<String>,
        auth_header: Option<String>,
        idem: IdemKey,
    },
    /// What a building's runs may reach: the sandbox's limits, and the
    /// external servers its tools come from.
    ///
    /// Both resolve city -> building -> room and neither had a surface,
    /// so a person could read what they were governed by and not change
    /// it. Each field is optional and an absent one leaves that section
    /// alone; an empty `mcp` list is a building that reaches no server,
    /// which is a different statement from not saying.
    ConfigureBuilding {
        addr: Address,
        sandbox: Option<SandboxLimits>,
        mcp: Option<Vec<McpServer>>,
        idem: IdemKey,
    },
    AttachEndpoint {
        name: ProviderName,
        /// The base URL as a provider's documentation prints it; the
        /// dialect owns the path that hangs off it.
        base_url: String,
        dialect: DialectKind,
        /// `secret:<realm>/<name>`, or absent for a local server that
        /// asks for no credential.
        secret: Option<String>,
        /// Header name for providers that do not take a bearer token.
        auth_header: Option<String>,
        /// Which of the models this endpoint serves the city admits. An
        /// empty list admits everything it serves, which is what a
        /// person who did not look at the list meant.
        admit: Vec<String>,
        idem: IdemKey,
    },
    /// Point one tag at one model of an attached endpoint, with the two
    /// facts no model list returns.
    SelectModel {
        endpoint: ProviderName,
        model: String,
        tag: ModelTag,
        context_tokens: u64,
        max_output_tokens: u64,
        idem: IdemKey,
    },
    Fork {
        run: RunId,
        at_seq: Seq,
        addr: Option<Address>,
        idem: IdemKey,
    },
    Attach {
        upload: UploadId,
        notify: Vec<RunId>,
        idem: IdemKey,
    },
    CreateBuilding {
        addr: Address,
        template: TemplateName,
        idem: IdemKey,
    },
    /// The one Command with no byte form. `Secret` is `Sealed<String>` in
    /// process and uninhabited on the wire.
    PutSecret {
        realm: String,
        name: String,
        value: Secret,
    },
    Steer {
        run: RunId,
        text: String,
        idem: IdemKey,
    },
    Cancel {
        run: RunId,
        idem: IdemKey,
    },
    Takeover {
        run: RunId,
        idem: IdemKey,
    },
    Rollback {
        checkpoint: GitOid,
        idem: IdemKey,
    },
    Halt {
        scope: HaltScope,
        idem: IdemKey,
    },
    Release {
        scope: HaltScope,
        idem: IdemKey,
    },
    BatchByBuilding {
        addr: Address,
        idem: IdemKey,
    },
    Approve {
        item: ApprovalId,
        verdict: PolicyVerdict,
        idem: IdemKey,
    },
    CreatePolicy {
        from_item: ApprovalId,
        idem: IdemKey,
    },
    SetAutonomy {
        scope: HaltScope,
        autonomy: Autonomy,
        idem: IdemKey,
    },
    /// Something happened outside. The city never asks whether anything
    /// did; the service holding the connection pushes, and this is the
    /// shape a push takes once it is inside.
    ///
    /// It carries no address. Where an arrival lands is the watch
    /// table's answer and then triage's, so a caller that could name a
    /// room would be a caller that could reach past the routing a person
    /// wrote.
    Wake {
        source: String,
        subject: String,
        body: String,
        idem: IdemKey,
    },
    /// Presenting a pairing token. Read-only, hence no `IdemKey`; the token
    /// is plain here because a token that must cross a wire has, by
    /// definition, no secrecy left to protect in transit - it is sealed the
    /// moment it lands (see `server::decide_handshake`).
    Auth {
        token: String,
    },
}

/// The Command set a socket can carry. `PutSecret` is unreachable because
/// `NoSecret` has no values.
pub type WireCommand = Command<NoSecret>;

impl<Secret> Command<Secret> {
    /// Exhaustive by construction: a new variant cannot compile without
    /// choosing its name here, which is what keeps [`COMMAND_NAMES`] honest.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match *self {
            Self::Dispatch { .. } => "Dispatch",
            Self::Wake { .. } => "Wake",
            Self::Login { .. } => "Login",
            Self::ConfigureBuilding { .. } => "ConfigureBuilding",
            Self::ProbeEndpoint { .. } => "ProbeEndpoint",
            Self::AttachEndpoint { .. } => "AttachEndpoint",
            Self::SelectModel { .. } => "SelectModel",
            Self::Fork { .. } => "Fork",
            Self::Attach { .. } => "Attach",
            Self::CreateBuilding { .. } => "CreateBuilding",
            Self::PutSecret { .. } => "PutSecret",
            Self::Steer { .. } => "Steer",
            Self::Cancel { .. } => "Cancel",
            Self::Takeover { .. } => "Takeover",
            Self::Rollback { .. } => "Rollback",
            Self::Halt { .. } => "Halt",
            Self::Release { .. } => "Release",
            Self::BatchByBuilding { .. } => "BatchByBuilding",
            Self::Approve { .. } => "Approve",
            Self::CreatePolicy { .. } => "CreatePolicy",
            Self::SetAutonomy { .. } => "SetAutonomy",
            Self::Auth { .. } => "Auth",
        }
    }

    /// The deduplication key. `None` only for the two Commands that change
    /// nothing: `Auth`, and `PutSecret` whose effect is confined to the host
    /// process and whose replay writes the same Vault entry.
    pub fn idem(&self) -> Option<&IdemKey> {
        match *self {
            Self::Dispatch { ref idem, .. }
            | Self::Wake { ref idem, .. }
            | Self::Login { ref idem, .. }
            | Self::Fork { ref idem, .. }
            | Self::ProbeEndpoint { ref idem, .. }
            | Self::ConfigureBuilding { ref idem, .. }
            | Self::Attach { ref idem, .. }
            | Self::CreateBuilding { ref idem, .. }
            | Self::Steer { ref idem, .. }
            | Self::Cancel { ref idem, .. }
            | Self::Takeover { ref idem, .. }
            | Self::Rollback { ref idem, .. }
            | Self::Halt { ref idem, .. }
            | Self::Release { ref idem, .. }
            | Self::BatchByBuilding { ref idem, .. }
            | Self::Approve { ref idem, .. }
            | Self::CreatePolicy { ref idem, .. }
            | Self::SetAutonomy { ref idem, .. }
            | Self::AttachEndpoint { ref idem, .. }
            | Self::SelectModel { ref idem, .. } => Some(idem),
            Self::PutSecret { .. } | Self::Auth { .. } => None,
        }
    }
}

impl From<WireCommand> for Command {
    /// Widening a wire frame into the in-process Command set. Total: the
    /// `PutSecret` arm is unreachable because its payload cannot exist.
    fn from(wire: WireCommand) -> Self {
        match wire {
            Command::Wake {
                source,
                subject,
                body,
                idem,
            } => Self::Wake {
                source,
                subject,
                body,
                idem,
            },
            Command::Dispatch {
                addr,
                task,
                goal,
                mode,
                budget,
                idem,
                session,
                effort,
            } => Self::Dispatch {
                addr,
                task,
                goal,
                mode,
                budget,
                idem,
                session,
                effort,
            },
            Command::Login {
                provider,
                step,
                idem,
            } => Self::Login {
                provider,
                step,
                idem,
            },
            Command::ConfigureBuilding {
                addr,
                sandbox,
                mcp,
                idem,
            } => Self::ConfigureBuilding {
                addr,
                sandbox,
                mcp,
                idem,
            },
            Command::ProbeEndpoint {
                name,
                base_url,
                dialect,
                secret,
                auth_header,
                idem,
            } => Self::ProbeEndpoint {
                name,
                base_url,
                dialect,
                secret,
                auth_header,
                idem,
            },
            Command::AttachEndpoint {
                name,
                base_url,
                dialect,
                secret,
                auth_header,
                admit,
                idem,
            } => Self::AttachEndpoint {
                name,
                base_url,
                dialect,
                secret,
                auth_header,
                admit,
                idem,
            },
            Command::SelectModel {
                endpoint,
                model,
                tag,
                context_tokens,
                max_output_tokens,
                idem,
            } => Self::SelectModel {
                endpoint,
                model,
                tag,
                context_tokens,
                max_output_tokens,
                idem,
            },
            Command::Fork {
                run,
                at_seq,
                addr,
                idem,
            } => Self::Fork {
                run,
                at_seq,
                addr,
                idem,
            },
            Command::Attach {
                upload,
                notify,
                idem,
            } => Self::Attach {
                upload,
                notify,
                idem,
            },
            Command::CreateBuilding {
                addr,
                template,
                idem,
            } => Self::CreateBuilding {
                addr,
                template,
                idem,
            },
            Command::PutSecret { value, .. } => match value {},
            Command::Steer { run, text, idem } => Self::Steer { run, text, idem },
            Command::Cancel { run, idem } => Self::Cancel { run, idem },
            Command::Takeover { run, idem } => Self::Takeover { run, idem },
            Command::Rollback { checkpoint, idem } => Self::Rollback { checkpoint, idem },
            Command::Halt { scope, idem } => Self::Halt { scope, idem },
            Command::Release { scope, idem } => Self::Release { scope, idem },
            Command::BatchByBuilding { addr, idem } => Self::BatchByBuilding { addr, idem },
            Command::Approve {
                item,
                verdict,
                idem,
            } => Self::Approve {
                item,
                verdict,
                idem,
            },
            Command::CreatePolicy { from_item, idem } => Self::CreatePolicy { from_item, idem },
            Command::SetAutonomy {
                scope,
                autonomy,
                idem,
            } => Self::SetAutonomy {
                scope,
                autonomy,
                idem,
            },
            Command::Auth { token } => Self::Auth { token },
        }
    }
}

/// Queries read state. They are cacheable and free of side effects, so none
/// carries an `IdemKey` - a Query that needed one would have stopped being a
/// Query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Query {
    /// A bounded slice of the one history, ending just before `before`
    /// or at the tail when that is absent.
    ///
    /// The server broadcasts what happens next and never what happened,
    /// so a page opened today saw a city that had been running for a
    /// month as an empty one. Bounded because the whole ledger is not a
    /// thing to put on a socket, and paged backwards because what a
    /// reader wants first is the end.
    History {
        before: Option<Seq>,
        limit: u32,
    },
    /// The same slice, narrowed to one session.
    ///
    /// [`Query::History`] carries no run, so a client watching four
    /// sessions divides one bounded slice between them and a session
    /// that started before the tab did is not in it at all - which is
    /// the whole of why opening yesterday's session showed a blank
    /// page. [`Query::RunView`] does not close the gap: five fields say
    /// whether a run exists and where it got to, not what happened in
    /// it.
    ///
    /// Answered with [`HistoryAnswer`], because "a page of history"
    /// already has a shape and a second one would be a second answer to
    /// the same question.
    RunHistory {
        run: RunId,
        before: Option<Seq>,
        limit: u32,
    },
    /// What moved between two checkpoints: paths and counts, never patch
    /// text.
    ///
    /// The caller names both ends because it already knows them - a
    /// checkpoint's oid is in the `checkpoint_committed` payload the
    /// client folded - and computing the pair a second time on the
    /// server would be a second answer to "which fences belong to this
    /// session". Both oids are immutable, so the answer is cacheable
    /// forever by anybody who wants to.
    ///
    /// `head` absent means the working tree: a wave still running has
    /// written files no checkpoint holds yet, and a list that ignored
    /// them would describe the session as it was one fence ago.
    Changes {
        base: GitOid,
        head: Option<GitOid>,
    },
    RunView {
        run: RunId,
    },
    CityView,
    ApprovalQueue,
    InboxView {
        addr: Address,
    },
    Metrics,
    CostView,
    ArchiveSearch {
        needle: String,
    },
    RegistryView,
    DiscardView,
    /// What is attached and what is chosen: the settings page's read.
    EndpointView,
    /// One building's own files and its archive - the pages an agent
    /// writes for the next agent, which are also the pages a person
    /// reads to know what happened in there.
    BuildingView {
        addr: Address,
    },
}

impl Query {
    /// Exhaustive, for the same reason as [`Command::name`].
    #[must_use]
    pub fn name(&self) -> &'static str {
        match *self {
            Self::History { .. } => "History",
            Self::RunHistory { .. } => "RunHistory",
            Self::Changes { .. } => "Changes",
            Self::RunView { .. } => "RunView",
            Self::CityView => "CityView",
            Self::ApprovalQueue => "ApprovalQueue",
            Self::InboxView { .. } => "InboxView",
            Self::Metrics => "Metrics",
            Self::CostView => "CostView",
            Self::ArchiveSearch { .. } => "ArchiveSearch",
            Self::RegistryView => "RegistryView",
            Self::DiscardView => "DiscardView",
            Self::EndpointView => "EndpointView",
            Self::BuildingView { .. } => "BuildingView",
        }
    }
}

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

/// The most ledger lines one [`Query::RunHistory`] answer may read.
///
/// Two bounds rather than one, because narrowing to a session separates
/// them: `limit` bounds what comes back and this bounds what was looked
/// at. Without it, asking for one session that ended a month ago would
/// walk the whole Ledger - unbounded server work a client can ask for,
/// which is the thing [`HISTORY_MAX`] exists to prevent for the
/// unfiltered query.
///
/// The consequence is deliberate and the caller has to handle it: an
/// answer may hold nothing while `earlier` is `Some`, meaning "this
/// stretch held none of it, keep asking". That is a different statement
/// from reaching the first record the city ever wrote.
pub const HISTORY_SCAN: u64 = 5_000;

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
}

/// A building's plan, as its own `Roadmap.md` states it.
///
/// `problems` carries the rows the table could not state — a row with a
/// status outside the five words, or a column count that is not four. The
/// interface shows them: a plan that quietly drops the lines it could not
/// parse would report progress against a denominator nobody chose.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildingProgress {
    pub addr: Address,
    pub progress: Progress,
    pub problems: Vec<String>,
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

/// A digest of the protocol surface, exchanged at connect time.
///
/// A browser can hold a cached older client while the server has moved on;
/// that mismatch is the one error WebUI has that a native window does not,
/// and this hash is its single answer. Pure: same inputs, same bytes, always.
#[must_use]
pub fn schema_hash() -> B3Hash {
    let mut material = Vec::new();
    material.extend_from_slice(b"sprawling/wire/");
    material.extend_from_slice(&WIRE_V.to_le_bytes());
    for name in COMMAND_NAMES {
        material.push(b'C');
        material.extend_from_slice(name.as_bytes());
    }
    for name in QUERY_NAMES {
        material.push(b'Q');
        material.extend_from_slice(name.as_bytes());
    }
    B3Hash::digest(&material)
}

/// The client's opening frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub wire_v: u32,
    pub schema: B3Hash,
    /// Present only when the server binds a non-loopback address.
    pub token: Option<String>,
}

/// The server's answer to a `Hello` it accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Welcome {
    pub wire_v: u32,
    pub schema: B3Hash,
    /// Where the Event stream resumes, so a reconnect leaves no gap.
    pub resume_from: Option<Seq>,
    /// Which city answered. The handshake is where a connection learns
    /// whose city it is: the name is in the Ledger's first record, and a
    /// client that only ever hears what happens *next* would otherwise
    /// have to display "no city" over a city that has been running for a
    /// month.
    pub city: Option<Address>,
}

/// Everything a client may send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientFrame {
    Hello(Hello),
    Command(Box<WireCommand>),
    Query(Query),
}

/// Everything a server may send. Events are the push half; a `Refusal`
/// carries the three-part refusal the interface renders verbatim.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerFrame {
    Welcome(Welcome),
    Event(Box<EventRecord>),
    Answer(Box<Answer>),
    Refusal(Box<AxError>),
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
    fn a_carried_name_rejects_empty_and_control_characters() {
        assert!(ModeTag::parse("").is_err());
        assert!(ModeTag::parse("plan\nsteal").is_err());
        assert_eq!(ModeTag::parse("plan").unwrap().as_str(), "plan");
        // No closed list: an unknown mode is upstream's to refuse, not ours.
        assert!(ModeTag::parse("a-mode-we-have-never-heard-of").is_ok());
    }

    #[test]
    fn a_wire_frame_carrying_put_secret_fails_to_decode() {
        let json = r#"{"put_secret":{"realm":"anthropic","name":"api","value":"sk-nope"}}"#;
        let decoded: Result<WireCommand, _> = serde_json::from_str(json);
        let err = decoded.expect_err("PutSecret has no wire form");
        assert!(
            !err.to_string().contains("sk-nope"),
            "the refusal must not echo the bytes it protects"
        );
    }

    #[test]
    fn the_query_names_match_the_variants() {
        let queries = [
            Query::History {
                before: None,
                limit: 20,
            },
            Query::RunHistory {
                run: RunId::from_bytes([1u8; 16]),
                before: None,
                limit: 20,
            },
            Query::Changes {
                base: kernel::GitOid::from_bytes([2u8; 20]),
                head: None,
            },
            Query::RunView {
                run: RunId::from_bytes([1u8; 16]),
            },
            Query::CityView,
            Query::ApprovalQueue,
            Query::InboxView {
                addr: Address::parse("acme").unwrap(),
            },
            Query::Metrics,
            Query::CostView,
            Query::ArchiveSearch {
                needle: "x".to_owned(),
            },
            Query::RegistryView,
            Query::DiscardView,
            Query::EndpointView,
            Query::BuildingView {
                addr: Address::parse("acme").unwrap(),
            },
        ];
        assert_eq!(queries.len(), QUERY_NAMES.len());
        for (query, expected) in queries.iter().zip(QUERY_NAMES) {
            assert_eq!(query.name(), expected, "declaration order must match");
        }
    }

    #[test]
    fn a_client_frame_round_trips_through_json() {
        let frame = ClientFrame::Query(Query::CityView);
        let text = serde_json::to_string(&frame).unwrap();
        let back: ClientFrame = serde_json::from_str(&text).unwrap();
        assert_eq!(frame, back);
    }
}
