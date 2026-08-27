// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! Pure decision functions: values in, verdicts out. Zero internal deps.
//! Holds no I/O handles, no clock, no global state (ARCHITECTURE.md paragraph 1).

mod error;

pub use error::{AxCode, AxError, Carrier, GateRefusal};

mod address;
mod locator;

pub use address::{Address, RESERVED_PREFIX, SessionName};
pub use locator::{B3Hash, GitOid, Locator, Range};

pub mod consts_external;
pub mod consts_policy;

mod event;

pub use event::{EventDraft, EventKind, EventRecord, EventRef, Payload};
pub use event::{RunId, Seq, TimeMs, WindowClass};

mod idem;
mod version;

pub use idem::{IDEM_DERIVE_V, IdemKey};
pub use version::{Version, VersionVerdict, check_base};

mod ledger;

#[cfg(feature = "conformance")]
pub use ledger::conformance;
pub use ledger::{GENESIS_PREV, Ledger, chain_hash};

mod taint;

pub use taint::{TaintSet, TaintSource, Tainted};

mod backpressure;
mod budget;
mod stall;
mod write_domain;

pub use backpressure::{Admission, ItemMeta, QueueStats, ShedReason, admit};
pub use budget::{BudgetCap, BudgetLadder, BudgetLayer, BudgetLevel, BudgetUse};
pub use budget::{ByteLen, CtxLock, CtxVerdict, SpendVerdict, Tokens, UsdMicros};
pub use budget::{admit_spend, observe_ctx};
pub use stall::{ActionFingerprint, StallVerdict, observe};
pub use write_domain::{DomainVerdict, EditSample, EditWarVerdict, WriteDomain, observe_edit_war};

mod delegation;
mod goal;
mod registry;
mod repair;

pub use delegation::admit as admit_delegation;
pub use delegation::{Delegate, DelegateKind, DelegationVerdict, Delegator, Depth};
pub use goal::{GoalEntry, GoalId, GoalResource, GoalVerdict, detect_conflict};
pub use registry::{Artifact, Claim, RegisterVerdict, Registry, ResidentId};
pub use repair::{RepairVerdict, request as request_repair};

mod approval;
mod completion;
mod spine;

pub use approval::{AnswerVerdict, Answerer, ApprovalClass, ApprovalId, ApprovalItem};
pub use approval::{ApprovalSource, Autonomy, ClusterKey, Policy, PolicyApplication};
pub use approval::{PolicyClass, PolicyExpiry, PolicyMatcher, PolicyRevocation, PolicyVerdict};
pub use approval::{expiry as policy_expiry, match_item, may_answer};
pub use completion::{Completion, Evidence, PlannedProgress, Progress, UnplannedProgress};
pub use spine::{EvidenceCell, MEMO_OUTLINE_FIELDS, MemoShape, ROADMAP_STATUS_SPELLINGS};
pub use spine::{RoadmapRow, RoadmapShape, RoadmapStatus, ScopeChange, WriteMoment};
pub use spine::{check_memo_shape, check_roadmap_shape, set_roadmap_status, tally};

mod change;

pub use change::{FileChange, How, Lines};

mod discard;
mod secret;

pub use discard::{DenyReason, Discard, DiscardForecast, DiscardRequest, DiscardVerdict};
pub use discard::{EscalateReason, Restoration, decide as decide_discard, forecast};
pub use secret::{Sealed, SecretRef, SecretSpan, scan};

mod gate;

pub use gate::{CommitmentDecision, DedupVerdict, EgressAllowlist, EgressOutcome, EgressTarget};
pub use gate::{GateContext, egress_target};
pub use gate::{GateOutcome, commitment, dedup, discard as gate_discard, domain, egress};
pub use gate::{delegation, govern, spawn, spend};

mod config;

pub use config::{ClockStampGranularity, ClockZone, FrozenConfig, McpServer, McpTransport};
pub use config::{LayeredValue, LiveConfig, SandboxLimits, freeze};

mod tool;

#[cfg(feature = "conformance")]
pub use tool::conformance as tool_conformance;
pub use tool::{CostTier, Effect, ExecArm, RenderIntent, Temporal, TimeoutMs};
pub use tool::{ServerLabel, Tool, ToolCall, ToolMeta, ToolName, ToolOutcome};

mod model;

pub use model::SystemBlock;
#[cfg(feature = "conformance")]
pub use model::conformance as model_conformance;
pub use model::{BuildingPolicy, Model, ModelRequest, ModelReturn};
pub use model::{ChatMessage, ChatRequest, ChatResponse, ContentBlock, DialectKind, Effort};
pub use model::{ModelTag, ModelUsage, Role, StopReason, ToolDef};
pub use model::{content_from_message, message_payload, value_has_float};
