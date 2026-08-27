// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! Turn loop and tool surface: frozen prefix, handoff, replay/fork,
//! result pipeline, sandbox ladder.

pub mod catalog;
pub mod clock;
pub mod compaction;
pub mod diagnostics;
pub mod digest;
pub mod fork;
pub mod handoff;
pub mod mode;
pub mod offload;
pub mod pipeline;
pub mod prefix;
pub mod redact;
pub mod replay;
pub mod run;
pub mod tools;

pub use tools::ChildStatus;
pub use tools::EditTool;
pub use tools::ExecTool;
pub use tools::ProviderMode;
pub use tools::ReadTool;
pub use tools::StatusSnapshot;
pub use tools::StatusTool;
pub use tools::parse_arm;
pub use tools::version_of;

mod sandbox;
pub mod turn;

pub use sandbox::AbsentSandbox;
pub use sandbox::EchoSandbox;
pub use sandbox::FaultSandbox;
pub use sandbox::Fuel;
pub use sandbox::Mount;
pub use sandbox::Sandbox;
pub use sandbox::SandboxExit;
pub use sandbox::SandboxJob;
pub use sandbox::SandboxOutcome;

#[cfg(feature = "wasm")]
pub use sandbox::WasmtimeSandbox;

#[cfg(feature = "conformance")]
pub use sandbox::assert_sandbox_conformance;

mod watchdog;

pub use catalog::{Catalog, CatalogEntry, Expansion};
pub use clock::{ClockStamp, StampGate, ZoneEntry, stamp};
pub use digest::{Breaker, BreakerVerdict, Digest, DigestOutcome, StructureNode};
pub use digest::{digest_once, structure_of};
pub use handoff::{Handoff, ResumeSeed, resume};
pub use mode::{Admission, Mode, Produced, admits};
pub use offload::{OffloadRecord, OffloadSite, offload, rematerialize};
pub use pipeline::{PackContext, Packaged, package};
pub use prefix::{FrozenPrefix, FrozenSegment, SegmentSlot};
pub use prefix::{PrefixPlan, SegmentCaps, SourceDoc, build_prefix};
pub use replay::{VerifiedLedger, VerifiedLine};
pub use run::{Advance, Run, RunHooks, RunPlan, SafePoint, drive};
pub use turn::{Interrupt, Opening, PhaseOutcome, Turn, TurnCancelled, TurnReport};
pub use watchdog::{Disposal, FreezeReason, Watchdog};
