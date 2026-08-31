// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Two protocols, pointing opposite ways: `mcp` lets a resident reach an
//! outside service, `acp` lets an outside editor ask this city for work.
//!
//! The asymmetry is the design. Reaching out is something a resident
//! chose and the egress gate can refuse; reaching in is something a
//! stranger did, so it produces nothing until it has been authenticated
//! and reduced to an ordinary dispatch.

mod acp;
mod mcp;

pub use acp::{Admitted, Incoming, Progress, admit};
pub use mcp::{EXTERNAL_CALL_PATIENCE, Handshake, Listed, McpTool, Outbound};
pub use mcp::{PROTOCOL_VERSION, digits_for_floats, handshake};
pub use mcp::{Rpc, ScriptedOutbound, tools_from};
