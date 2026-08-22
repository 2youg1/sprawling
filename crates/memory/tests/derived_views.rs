// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Shape 7 — the disposable derived view — asserted once and
//! instantiated three times.
//!
//! `index`, `hot` and `projection` differ in what they store and where,
//! but they make the same promise: throw the view away, replay the same
//! ledger, and the observable answer comes back identical. That promise
//! is what makes them safe to delete, and it is stated here in one
//! place so the three cannot drift apart.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

use std::fmt::Debug;
use std::path::Path;

use kernel::{B3Hash, EventDraft, EventKind, EventRecord, Payload, RunId, Seq, TimeMs};
use memory::{HotView, LedgerIndex, Projection};
use proptest::prelude::*;

/// The skeleton: build twice from the same records, observe both, and
/// require the observations to agree. A view that fails this is not
/// disposable, whatever else it does.
fn rebuild_is_stable<V, O: PartialEq + Debug>(
    records: &[EventRecord],
    build: impl Fn(&[EventRecord], u8) -> V,
    observe: impl Fn(&V) -> O,
) {
    let first = build(records, 0);
    let second = build(records, 1);
    assert_eq!(
        observe(&first),
        observe(&second),
        "the same ledger must rebuild to the same view"
    );
}

fn record(run: RunId, seq: u64, kind: EventKind, data: serde_json::Value) -> EventRecord {
    let map = data.as_object().cloned().unwrap_or_default();
    let draft = EventDraft {
        run,
        t: TimeMs::new(seq.saturating_mul(7)),
        who: format!("resident-{}", seq % 3),
        addr: None,
        kind,
        data: Payload::new(map).expect("payload"),
        ig: false,
    };
    EventRecord::from_draft(draft, Seq::new(seq), B3Hash::digest(b"prev"))
}

/// Four kinds are enough to exercise every branch the three views have:
/// two that create rows, one that closes them, one that is plain
/// progress.
fn kind_of(pick: u8) -> EventKind {
    match pick % 4 {
        0 => EventKind::RunStarted,
        1 => EventKind::FileDiscarded,
        2 => EventKind::RunFrozen,
        _ => EventKind::ToolCalled,
    }
}

fn script(picks: &[(u8, u8)]) -> Vec<EventRecord> {
    picks
        .iter()
        .enumerate()
        .map(|(i, (kind_pick, run_pick))| {
            let run = RunId::from_bytes([run_pick.saturating_add(1); 16]);
            let seq = u64::try_from(i).unwrap_or(0);
            let kind = kind_of(*kind_pick);
            let data = match kind {
                EventKind::FileDiscarded => serde_json::json!({
                    "paths": [format!("file:work/{seq}.tmp")],
                    "restoration": { "interred": format!("cas:b3-{seq}") },
                }),
                EventKind::RunFrozen => serde_json::json!({ "completion": "done" }),
                _ => serde_json::json!({}),
            };
            record(run, seq, kind, data)
        })
        .collect()
}

fn write_segment(dir: &Path, records: &[EventRecord]) {
    let mut blob = Vec::new();
    for r in records {
        blob.extend_from_slice(&r.canonical_line().expect("canonical line"));
        blob.push(b'\n');
    }
    std::fs::write(dir.join("ledger-00000000000000000000.jsonl"), &blob).expect("write segment");
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(24))]

    #[test]
    fn every_derived_view_rebuilds_to_the_same_answer(
        picks in prop::collection::vec((0u8..4, 0u8..3), 1..24)
    ) {
        let records = script(&picks);
        let root = tempfile::tempdir().expect("tempdir");

        // Instance 1 — index: the map is rebuilt from the segment, and
        // every line it points at must come back verbatim.
        rebuild_is_stable(
            &records,
            |records, nonce| {
                let dir = root.path().join(format!("idx-{nonce}"));
                std::fs::create_dir_all(&dir).expect("mkdir");
                write_segment(&dir, records);
                let index = LedgerIndex::load_or_rebuild(&dir).expect("index");
                (dir, index)
            },
            |(dir, index)| {
                let lines: Vec<Vec<u8>> = (0..index.len())
                    .map(|i| {
                        index
                            .line_at(dir, Seq::new(u64::try_from(i).unwrap_or(0)))
                            .expect("line")
                    })
                    .collect();
                (index.tail_seq(), lines)
            },
        );

        // Instance 2 — hot: the fold is order-stable and phase-stable.
        rebuild_is_stable(
            &records,
            |records, _| {
                let mut view = HotView::new();
                for r in records {
                    view.apply(r).expect("apply");
                }
                view
            },
            |view| {
                view.runs()
                    .map(|(id, hot)| format!("{id} {:?} {} {:?}", hot.phase, hot.last_seq.value(), hot.last_kind))
                    .collect::<Vec<String>>()
            },
        );

        // Instance 3 — projection: the logical export is byte-identical
        // even though the redb files need not be.
        rebuild_is_stable(
            &records,
            |records, nonce| {
                let path = root.path().join(format!("proj-{nonce}.redb"));
                let mut projection = Projection::open(&path).expect("open");
                for r in records {
                    projection.apply(r).expect("apply");
                }
                projection
            },
            |projection| projection.export_canonical().expect("export"),
        );
    }
}

/// The hot and cold views answer to the same ledger, so where they
/// overlap they must not disagree — a Run counted active in memory and
/// frozen on disk would make the interface a liar.
#[test]
fn hot_and_cold_agree_where_they_overlap() {
    let picks: Vec<(u8, u8)> = vec![
        (0, 0),
        (3, 0),
        (0, 1),
        (1, 1),
        (2, 0),
        (3, 1),
        (0, 2),
        (2, 2),
    ];
    let records = script(&picks);
    let tmp = tempfile::tempdir().expect("tempdir");

    let mut hot = HotView::new();
    let mut cold = Projection::open(&tmp.path().join("overlap.redb")).expect("open");
    for r in &records {
        hot.apply(r).expect("hot");
        cold.apply(r).expect("cold");
    }

    let hot_runs: Vec<(String, bool)> = hot
        .runs()
        .map(|(id, h)| (id.to_string(), h.phase == memory::RunPhase::Frozen))
        .collect();
    let cold_runs: Vec<(String, bool)> = cold
        .run_rows()
        .expect("rows")
        .into_iter()
        .map(|row| (row.run, row.frozen.is_some()))
        .collect();
    assert_eq!(hot_runs, cold_runs, "same runs, same phase, same order");
}
