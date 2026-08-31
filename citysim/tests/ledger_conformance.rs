// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! S1.11 closing card: the in-memory Ledger is the second adapter of the
//! kernel port (V3 made real), the chain checker is invariant 1, and the
//! golden fixture pins cross-OS byte identity (V8 seed).

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

use citysim::{MemLedger, check_chain};
use kernel::conformance::assert_ledger_conformance;
use kernel::{AxCode, EventDraft, EventKind, Ledger as _, Payload, RunId, TimeMs};
use serde_json::json;
use std::path::Path;

fn draft(kind: EventKind, t: u64, who: &str) -> EventDraft {
    EventDraft {
        run: RunId::CITY,
        t: TimeMs::new(t),
        who: who.to_string(),
        addr: None,
        kind,
        data: Payload::empty(),
        ig: false,
    }
}

/// The fixed script behind fixtures/golden-s1: breadth over addr, ig and
/// payload shapes. Never edit without regenerating the fixture (see
/// golden_fixture below).
fn fixture_script() -> Vec<EventDraft> {
    let run = RunId::from_bytes([1; 16]);
    let mut building = serde_json::Map::new();
    building.insert("template".to_owned(), json!("workshop"));
    let mut tool = serde_json::Map::new();
    tool.insert("tool".to_owned(), json!("exec"));
    tool.insert("args".to_owned(), json!(["b", "a"]));
    let mut truncated = serde_json::Map::new();
    truncated.insert("dropped_bytes".to_owned(), json!(7));
    vec![
        draft(EventKind::CityInitialized, 1, "city"),
        EventDraft {
            addr: Some(kernel::Address::parse("lab").unwrap()),
            data: Payload::new(building).unwrap(),
            ..draft(EventKind::BuildingCreated, 2, "city")
        },
        EventDraft {
            run,
            ..draft(EventKind::RunStarted, 3, "planner@lab.1")
        },
        EventDraft {
            run,
            data: Payload::new(tool).unwrap(),
            ..draft(EventKind::ToolCalled, 4, "planner@lab.1")
        },
        EventDraft {
            data: Payload::new(truncated).unwrap(),
            ig: true,
            ..draft(EventKind::LogTruncated, 5, "system")
        },
        EventDraft {
            run,
            ..draft(EventKind::RunFrozen, 6, "planner@lab.1")
        },
    ]
}

#[test]
fn mem_ledger_passes_the_same_conformance_suite_as_jsonl() {
    assert_ledger_conformance(MemLedger::new);
}

#[test]
fn mem_and_jsonl_produce_identical_bytes() {
    let mut mem = MemLedger::new();
    for d in fixture_script() {
        mem.append(d).unwrap();
    }
    let dir = tempfile::tempdir().unwrap();
    let (mut jsonl, _) = memory::JsonlLedger::open(dir.path(), TimeMs::new(0)).unwrap();
    jsonl.append_all(fixture_script()).unwrap();

    assert_eq!(
        mem.raw_lines(),
        jsonl.read_raw_lines().unwrap(),
        "one canonical byte producer, two adapters"
    );
}

#[test]
fn the_chain_checker_passes_truth_and_bites_tampering() {
    let mut mem = MemLedger::new();
    for d in fixture_script() {
        mem.append(d).unwrap();
    }
    let lines = mem.raw_lines().to_vec();
    check_chain(lines.clone()).unwrap();

    let mut tampered = lines;
    let mid = tampered[3].len() / 2;
    tampered[3][mid] ^= 1;
    let err = check_chain(tampered).unwrap_err();
    assert_eq!(err.code(), &AxCode::CasCorrupt);
}

#[test]
fn golden_fixture_pins_cross_os_bytes() {
    let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("..")
        .join("fixtures")
        .join("golden-s1")
        .join("ledger-00000000000000000000.jsonl");

    let mut mem = MemLedger::new();
    for d in fixture_script() {
        mem.append(d).unwrap();
    }
    let mut rebuilt: Vec<u8> = Vec::new();
    for line in mem.raw_lines() {
        rebuilt.extend_from_slice(line);
        rebuilt.push(b'\n');
    }

    if std::env::var("GOLDEN_WRITE").as_deref() == Ok("1") {
        std::fs::write(&fixture, &rebuilt).unwrap();
    }
    let committed = std::fs::read(&fixture).expect("run once with GOLDEN_WRITE=1");
    assert_eq!(
        committed, rebuilt,
        "the same script must materialize byte-identically on every OS"
    );

    // And the fixture replays: chain verification is OS-independent too.
    check_chain(mem.raw_lines().to_vec()).unwrap();
}
