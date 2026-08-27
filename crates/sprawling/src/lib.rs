// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! The city harness as a library, so that the assembly point has a door
//! somebody outside this crate can enter (sprawling-SPEC.md section 8-15).
//!
//! Two of the binary's modules stay with the binary rather than here:
//! `install` puts this executable where a shell will find it, and
//! `wire_client` talks to a served city from a terminal. Both are about
//! the command line rather than about a city, and neither has a second
//! caller.
//!
//! What is `pub` is decided per item, never per module. A module is
//! `pub` so that `bin::assembly` keeps the name ARCHITECTURE.md section
//! 12 gives it; the items inside stay `pub(crate)` unless the binary or
//! an integration test enters through them, and `cargo xtask apisync`
//! holds the result against `xtask/api-baselines/sprawling.txt`.

pub mod assembly;
pub mod console;
pub mod firstrun;

mod effect;
mod keying;
mod mcp_http;
mod mcp_stdio;
