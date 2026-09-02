// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Process boundary: Command/Query/Event wire, WebSocket server, auth,
//! read-only multi-city aggregation.
//!
//! The kernel types that appear on this crate's own signatures are
//! re-exported below. `web` depends on `channels` and on nothing else
//! (ARCHITECTURE section 2), so a client that cannot name `EventRecord`
//! cannot read the frames it is sent; a boundary crate that hands out
//! frames owes the vocabulary to read them.

mod aggregate;
#[cfg(feature = "server")]
mod assets;
mod auth;
mod control;
#[cfg(feature = "server")]
mod reception;
#[cfg(feature = "server")]
mod server;
mod wire;

pub use aggregate::{Aggregate, CityLabel, Forwarded, Sighting, Upstream};
#[cfg(feature = "server")]
pub use assets::{AssetReply, ClientAssets, EmbeddedFile};
pub use auth::{PairingToken, verify};
pub use control::{ControlVerdict, Intervention, classify};
pub use kernel::{FileChange, How, Lines};
pub use kernel::{Span, Token, markdown};
#[cfg(feature = "server")]
pub use reception::{BindFace, BindVerdict, HandshakeVerdict};
#[cfg(feature = "server")]
pub use reception::{SessionState, SessionStep, decide_frame};
#[cfg(feature = "server")]
pub use reception::{decide_bind, decide_handshake};
#[cfg(feature = "server")]
pub use server::{AcpBody, AcpProgress, AcpSink};
#[cfg(feature = "server")]
pub use server::{Delivered, Reply, ServeConfig, router, serve};
pub use wire::RunSummary;
pub use wire::{Answer, ApprovalsAnswer, ArchiveLine, BlockedLine, BuildingProgress};
pub use wire::{ArchiveAnswer, ArchiveHit, DiscardAnswer, DiscardLine};
pub use wire::{BuildingAnswer, BuildingDoc};
pub use wire::{COMMAND_NAMES, QUERY_NAMES, WIRE_V, schema_hash};
pub use wire::{ChangesAnswer, HISTORY_MAX, HistoryAnswer};
pub use wire::{ChosenSummary, CityAnswer, CostAnswer, EndpointSummary, EndpointsAnswer};
pub use wire::{ClientFrame, Command, Delta, ServerFrame, WireCommand};
pub use wire::{HaltScope, Hello, LoginStep, NoSecret, PlanRow, Query, Welcome};
pub use wire::{InboxAnswer, MetricsAnswer, RegistryAnswer, RegistryLine, SignalLine};
pub use wire::{ModeTag, ProviderName, TemplateName, UploadId};
pub use wire::{PursuitLine, PursuitStep};

pub use kernel::{Address, ApprovalId, Autonomy, AxCode, AxError, B3Hash, BudgetCap};
pub use kernel::{ApprovalClass, ApprovalItem, ApprovalSource, ClusterKey, Restoration};
pub use kernel::{BudgetUse, Locator, PlannedProgress, Progress, UnplannedProgress};
pub use kernel::{DialectKind, Effort, ModelTag};
pub use kernel::{EventDraft, EventKind, EventRecord, GitOid, IdemKey, RunId};
pub use kernel::{McpServer, McpTransport, SandboxLimits, ServerLabel};
pub use kernel::{NodeId, PursuitState, RoadmapStatus, WHOLE_PPB};
pub use kernel::{Payload, Sealed, Seq, TimeMs, Tokens, UsdMicros};
pub use kernel::{PolicyVerdict, SessionName, WriteDomain};
