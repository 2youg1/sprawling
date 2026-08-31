// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The door the wire uses, entered from outside the crate.
//!
//! A served city reaches its assembly point one way: `channels::server`
//! hands a `Command` to `RunWorker::handle`. Nothing in this repository
//! could do the same, because the crate had no lib target and the
//! assembly point was a private module of a binary — so the widest
//! concentration of policy here was reachable only from tests sitting in
//! the same file. This enters by the production door and asserts on the
//! Ledger, which is the city's only history.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

use kernel::{Address, EventKind, EventRecord, IdemKey, RunId, Seq};
use sprawling::assembly;

/// The city this test builds is the one the assertion reads back, so the
/// name appears once.
const LAB: &str = "lab";

#[test]
fn a_command_reaches_the_ledger_through_the_door_the_wire_uses() {
    let dir = tempfile::tempdir().unwrap();
    let raised = assembly::init_city(dir.path()).unwrap();

    // The vault is the in-session one: a test that reached the platform
    // credential service would be a test that writes to the machine
    // running it.
    let mut worker = assembly::RunWorker::new(
        dir.path(),
        gateway::Custodian::in_memory(),
        runtime::diagnostics::Diagnostics::off(),
    )
    .unwrap();

    worker
        .handle(channels::Command::CreateBuilding {
            addr: Address::parse(LAB).unwrap(),
            template: channels::TemplateName::parse("minimal").unwrap(),
            idem: IdemKey::derive(&RunId::CITY, Seq::FIRST, b"create"),
        })
        .unwrap();

    // The Ledger, never the worker's own fields: what a city is, is what
    // its history says. The directory comes from the report the genesis
    // write returned, so this test spells no path of its own.
    //
    // The address is read from the payload rather than the envelope: a
    // city-level record is written against `RunId::CITY` with no `addr`,
    // so an envelope check here would pass for the wrong reason on the
    // day the payload stopped naming the building.
    let verified = runtime::replay::verify_ledger_dir(&raised.ledger_dir).unwrap();
    let laid = verified.raw_lines().iter().any(|line| {
        let record = EventRecord::parse_line(line).unwrap();
        record.kind() == EventKind::BuildingCreated
            && record
                .data()
                .as_map()
                .get("addr")
                .and_then(serde_json::Value::as_str)
                == Some(LAB)
    });
    assert!(
        laid,
        "the command went through the door but never reached the history"
    );
}
