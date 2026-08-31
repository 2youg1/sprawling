// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Effect side of persistence: Ledger on disk, CAS, projections, git
//! checkpoints, queues. Implements kernel ports; holds no policy.

mod jsonl;

pub use jsonl::WriteObserver;
// One line on purpose: the index-file rule permits single-line `use`
// declarations only, and rustfmt wraps the list at 100 columns.
#[rustfmt::skip]
pub use jsonl::{JsonlLedger, MemoryError, OpenReport, TailTruncation, ledger_segments_at, read_raw_lines_at};

#[cfg(any(test, feature = "fault"))]
mod fault_fs;

#[cfg(any(test, feature = "fault"))]
pub use fault_fs::{FaultFs, FaultPlan, TornTail};

mod bundle;

pub use bundle::{Bundle, MANIFEST, Manifest, open_restored};

mod cas;

pub use cas::Cas;

mod index;

pub use index::LedgerIndex;

mod hot;

pub use hot::HotView;
pub use hot::RunHot;
pub use hot::RunPhase;

mod projection;

pub use projection::Projection;
pub use projection::ProjectionOpenReport;
pub use projection::RecycleEntry;
pub use projection::RunRow;
pub use projection::ViewRebuilt;

mod attribution;

pub use attribution::Attribution;
pub use attribution::AttributionReport;

mod queue;

pub use queue::EventQueue;
pub use queue::QueueItem;
pub use queue::QueueLane;

mod digest_cache;

pub use digest_cache::DigestCache;

mod worktree;

pub use worktree::PlannedMerge;
pub use worktree::WorktreeLease;
pub use worktree::WorktreeName;
pub use worktree::Worktrees;

mod checkpoint;

pub use checkpoint::Checkpoint;

mod changes;

pub use changes::{Head, between};
