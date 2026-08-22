// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! A2 (offline replay verifies the chain) and A19 (a fork prefix is
//! byte-identical and the mother stays untouched), demonstrated end to
//! end over the real durable ledger.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

use kernel::{AxCode, EventDraft, EventKind, Payload, RunId, Seq, TimeMs};
use memory::JsonlLedger;
use runtime::{VerifiedLine, fork, replay};

fn draft(kind: EventKind, t: u64) -> EventDraft {
    EventDraft {
        run: RunId::from_bytes([3; 16]),
        t: TimeMs::new(t),
        who: "planner@lab.1".to_string(),
        addr: None,
        kind,
        data: Payload::empty(),
        ig: false,
    }
}

fn build_ledger(dir: &std::path::Path) -> Vec<Vec<u8>> {
    let (mut ledger, _) = JsonlLedger::open(dir, TimeMs::new(0)).unwrap();
    ledger
        .append_all(vec![
            draft(EventKind::CityInitialized, 1),
            draft(EventKind::RunStarted, 2),
            draft(EventKind::ToolCalled, 3),
            draft(EventKind::ToolResult, 4),
            draft(EventKind::RunFrozen, 5),
        ])
        .unwrap();
    ledger.read_raw_lines().unwrap()
}

#[test]
fn a2_replay_verifies_a_real_ledger_offline() {
    let dir = tempfile::tempdir().unwrap();
    let baseline = build_ledger(dir.path());

    let verified = replay::verify_ledger_dir(dir.path()).unwrap();
    assert_eq!(verified.raw_lines(), &baseline[..]);
    assert_eq!(verified.tail_seq(), Some(Seq::new(4)));
    assert_eq!(verified.lines().len(), 5);
    assert!(
        verified
            .lines()
            .iter()
            .all(|l| matches!(l, VerifiedLine::Known { .. }))
    );

    // The second minting point: refs echo the verified records.
    match &verified.lines()[2] {
        VerifiedLine::Known { record, echo } => {
            assert_eq!(echo.seq(), record.seq());
            assert_eq!(echo.kind(), EventKind::ToolCalled);
        }
        other => panic!("expected Known, got {other:?}"),
    }
}

#[test]
fn a2_any_tampered_byte_is_refused_with_the_line_number() {
    let dir = tempfile::tempdir().unwrap();
    let baseline = build_ledger(dir.path());

    let mut lines = baseline;
    // Flip one byte in the middle of line 3 (still valid JSON shape-wise
    // or not - either way the chain must bite).
    let target = &mut lines[2];
    let mid = target.len() / 2;
    target[mid] ^= 1;

    let err = replay::verify_lines(lines).unwrap_err();
    assert_eq!(err.code(), &AxCode::CasCorrupt);
    assert!(err.subject().contains("line 3"), "{}", err.subject());
}

#[test]
fn a19_fork_prefix_is_byte_identical_and_mother_is_untouched() {
    let dir = tempfile::tempdir().unwrap();
    let baseline = build_ledger(dir.path());

    let mother = replay::verify_ledger_dir(dir.path()).unwrap();
    let prefix = fork::prefix(&mother, Seq::new(2)).unwrap();
    assert_eq!(prefix, baseline[..3].to_vec(), "lines 0..=2, byte-exact");

    // Past the tail: refused, never clamped.
    let err = fork::prefix(&mother, Seq::new(99)).unwrap_err();
    assert_eq!(err.code(), &AxCode::InvalidArgs);

    // The mother sequence is untouched by forking.
    let again = replay::verify_ledger_dir(dir.path()).unwrap();
    assert_eq!(again.raw_lines(), &baseline[..]);

    // The run_forked draft carries from/at_seq in its payload.
    let child = RunId::from_bytes([7; 16]);
    let draft = fork::fork_draft(
        RunId::from_bytes([3; 16]),
        Seq::new(2),
        child,
        TimeMs::new(9),
        "owner".to_string(),
    )
    .unwrap();
    assert_eq!(draft.kind, EventKind::RunForked);
    assert_eq!(
        draft.data.as_map().get("at_seq"),
        Some(&serde_json::Value::from(2u64))
    );
}
