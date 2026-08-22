// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Deterministic city simulator, the second Main: thin executor, seeded RNG,
//! virtual clock, fault injection. Adapters and the invariant checker land
//! from S1. Dev-only workspace member; not in the product graph.

mod checker;
mod executor;
mod mem_ledger;
mod script_model;
mod script_tools;

pub use checker::check_chain;
pub use executor::{CancelPoint, Scenario, ScenarioReport, run_scenario};
pub use mem_ledger::MemLedger;
pub use script_model::ScriptModel;
pub use script_tools::{ScriptTool, ScriptToolSet};
