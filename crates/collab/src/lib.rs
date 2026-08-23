// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Resident-to-Resident protocols: Inbox, HeldDraft, Workshop, fan-in,
//! PR flow, arbitration, triage.

mod arbiter;
mod archive_tool;
mod claim_tool;
mod delegate_tool;
mod draft;
mod fanin;
mod goal_tool;
mod handback;
mod inbox;
mod pr;
mod pr_tool;
mod signal_tool;
mod steer;
mod triage;
mod workshop;
mod workshop_tool;

pub use arbiter::{Circumstance, Escalation, Level, arbitrate, conflict_payload};
pub use archive_tool::{ARCHIVE_KINDS, ArchiveDesk, ArchiveEffect, ArchiveTool, Held};
pub use claim_tool::{ClaimDesk, ClaimEffect, ClaimTool, evidence_of, still_true};
pub use delegate_tool::{DelegateDesk, DelegateTool, Delegated};
pub use draft::{Draft, Drafts, HoldToken, Resolution, Return, Submission};
pub use fanin::{Artifact, Claim, FanIn, Joined, PrivateQuestion};
pub use goal_tool::{GoalDesk, GoalEffect, GoalTool};
pub use handback::Handback;
pub use inbox::{Inbox, Lane, Signal, SignalId, SignalKind};
pub use pr::{Merged, Open, Pr, Verified};
pub use pr_tool::{OpenRequest, PrDesk, PrEffect, PrTool};
pub use signal_tool::{SignalDesk, SignalEffect, SignalTool};
pub use steer::{AgentSteer, Steer};
pub use triage::{Arrival, Landing, Reflex, Rule, Triage};
pub use workshop::{NodeContract, NodeId, Workshop};
pub use workshop_tool::{WorkshopDesk, WorkshopTool};
