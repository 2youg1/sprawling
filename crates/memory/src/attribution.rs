// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Where the money went. Four independent cuts of
//! one authoritative total: by Run, by actor, by prefix segment, by
//! tool.
//!
//! The invariant that makes the report trustworthy (A20) is that each
//! cut sums to exactly the billed total — not approximately, not after
//! rounding. Percentages cannot promise that, so nothing here is a
//! percentage: shares are split by the largest-remainder method, which
//! distributes the floor divisions and then hands the leftover units
//! out one at a time, in a fixed order. What is billed is what is
//! attributed, to the microdollar.
//!
//! The total comes from `model_returned.billed_usd_micros` and nowhere
//! else. This module never prices a call — that authority is
//! `gateway::cost`, and a second one would be a second answer.

use std::collections::BTreeMap;

use kernel::{EventKind, EventRecord, UsdMicros};
use serde_json::Value;

use crate::jsonl::MemoryError;

/// The bucket a call falls into when the ledger gives no basis for a
/// finer split — an honest "unsplit", never a silent drop.
const NO_TOOL: &str = "no_tool";
const NO_SKILL: &str = "no_skill";
const NO_SEGMENT: &str = "unattributed";
const WINDOW_SLOT: &str = "window";

#[derive(Default)]
pub struct Attribution {
    by_run: BTreeMap<String, u64>,
    by_actor: BTreeMap<String, u64>,
    by_segment: BTreeMap<String, u64>,
    by_tool: BTreeMap<String, u64>,
    by_skill: BTreeMap<String, u64>,
    total: u64,
    /// Basis for the next model_returned: the most recent prefix shape
    /// and the tool bytes returned since the previous call.
    segment_weights: Vec<(String, u64)>,
    tool_weights: BTreeMap<String, u64>,
    skill_weights: BTreeMap<String, u64>,
}

pub struct AttributionReport {
    pub total: UsdMicros,
    pub by_run: Vec<(String, UsdMicros)>,
    pub by_actor: Vec<(String, UsdMicros)>,
    pub by_segment: Vec<(String, UsdMicros)>,
    pub by_tool: Vec<(String, UsdMicros)>,
    pub by_skill: Vec<(String, UsdMicros)>,
}

impl Attribution {
    pub fn new() -> Attribution {
        Attribution::default()
    }

    /// Folds one record in. Only `model_returned` moves money; the
    /// other two kinds set the basis on which the next call is split.
    pub fn apply(&mut self, record: &EventRecord) -> Result<(), MemoryError> {
        match record.kind() {
            EventKind::PromptAssembled => {
                self.segment_weights = segment_weights(record.data().as_map());
            }
            EventKind::ToolResult => {
                let name = record
                    .data()
                    .as_map()
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or("unnamed")
                    .to_owned();
                let bytes = record
                    .data()
                    .as_map()
                    .get("bytes")
                    .and_then(Value::as_u64)
                    .unwrap_or(1);
                let slot = self.tool_weights.entry(name).or_insert(0);
                *slot = slot.saturating_add(bytes);
                // The SKILL a call was made under, when one was: P1's
                // Library fills this; until then every call is unsplit.
                let skill = record
                    .data()
                    .as_map()
                    .get("skill")
                    .and_then(Value::as_str)
                    .unwrap_or(NO_SKILL)
                    .to_owned();
                let slot = self.skill_weights.entry(skill).or_insert(0);
                *slot = slot.saturating_add(bytes);
            }
            EventKind::ModelReturned => {
                let Some(billed) = record
                    .data()
                    .as_map()
                    .get("billed_usd_micros")
                    .and_then(Value::as_u64)
                else {
                    // A call with no authoritative amount attributes
                    // nothing. Estimating here would invent money.
                    self.tool_weights.clear();
                    self.skill_weights.clear();
                    return Ok(());
                };
                self.total = self.total.saturating_add(billed);
                add(&mut self.by_run, &record.run().to_string(), billed);
                add(&mut self.by_actor, record.who(), billed);
                for (bucket, share) in split(billed, &self.segment_weights, NO_SEGMENT) {
                    add(&mut self.by_segment, &bucket, share);
                }
                let tool_basis: Vec<(String, u64)> = self
                    .tool_weights
                    .iter()
                    .map(|(k, v)| (k.clone(), *v))
                    .collect();
                for (bucket, share) in split(billed, &tool_basis, NO_TOOL) {
                    add(&mut self.by_tool, &bucket, share);
                }
                let skill_basis: Vec<(String, u64)> = self
                    .skill_weights
                    .iter()
                    .map(|(k, v)| (k.clone(), *v))
                    .collect();
                for (bucket, share) in split(billed, &skill_basis, NO_SKILL) {
                    add(&mut self.by_skill, &bucket, share);
                }
                // The wave is settled; the next call starts a new basis.
                self.tool_weights.clear();
                self.skill_weights.clear();
            }
            _ => {}
        }
        Ok(())
    }

    pub fn report(&self) -> AttributionReport {
        AttributionReport {
            total: UsdMicros::new(self.total),
            by_run: quantify(&self.by_run),
            by_actor: quantify(&self.by_actor),
            by_segment: quantify(&self.by_segment),
            by_tool: quantify(&self.by_tool),
            by_skill: quantify(&self.by_skill),
        }
    }
}

fn add(map: &mut BTreeMap<String, u64>, key: &str, amount: u64) {
    let slot = map.entry(key.to_owned()).or_insert(0);
    *slot = slot.saturating_add(amount);
}

fn quantify(map: &BTreeMap<String, u64>) -> Vec<(String, UsdMicros)> {
    map.iter()
        .map(|(k, v)| (k.clone(), UsdMicros::new(*v)))
        .collect()
}

/// The prefix shape as the ledger recorded it: four segment slots plus
/// the in-window history, weighted by bytes.
fn segment_weights(data: &serde_json::Map<String, Value>) -> Vec<(String, u64)> {
    let mut weights = Vec::new();
    if let Some(segments) = data.get("segments").and_then(Value::as_array) {
        for segment in segments {
            let slot = segment
                .get("slot")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_owned();
            let len = segment.get("len").and_then(Value::as_u64).unwrap_or(0);
            weights.push((slot, len));
        }
    }
    if let Some(window) = data.get("window_bytes").and_then(Value::as_u64) {
        weights.push((WINDOW_SLOT.to_owned(), window));
    }
    weights
}

/// Largest remainder: floor every share, then hand the leftover units
/// to the largest remainders, ties broken by bucket name. The result
/// sums to `total` exactly, for any weights, including all-zero ones.
fn split(total: u64, weights: &[(String, u64)], fallback: &str) -> Vec<(String, u64)> {
    let sum: u64 = weights.iter().map(|(_, w)| *w).fold(0, u64::saturating_add);
    if weights.is_empty() || sum == 0 {
        return vec![(fallback.to_owned(), total)];
    }
    let total_wide = u128::from(total);
    let sum_wide = u128::from(sum);
    let mut shares: Vec<(String, u64, u128)> = Vec::new();
    let mut assigned: u64 = 0;
    for (bucket, weight) in weights {
        let scaled = total_wide.saturating_mul(u128::from(*weight));
        let floor = scaled.checked_div(sum_wide).unwrap_or(0);
        let remainder = scaled.checked_rem(sum_wide).unwrap_or(0);
        let floor_u64 = u64::try_from(floor).unwrap_or(u64::MAX);
        assigned = assigned.saturating_add(floor_u64);
        shares.push((bucket.clone(), floor_u64, remainder));
    }
    // Order by remainder descending, then by name ascending, so the
    // leftover units land in the same place on every machine.
    let mut order: Vec<usize> = (0..shares.len()).collect();
    order.sort_by(|a, b| {
        let left = shares.get(*a);
        let right = shares.get(*b);
        match (left, right) {
            (Some(l), Some(r)) => r.2.cmp(&l.2).then_with(|| l.0.cmp(&r.0)),
            _ => std::cmp::Ordering::Equal,
        }
    });
    let mut leftover = total.saturating_sub(assigned);
    for index in order {
        if leftover == 0 {
            break;
        }
        if let Some(entry) = shares.get_mut(index) {
            entry.1 = entry.1.saturating_add(1);
            leftover = leftover.saturating_sub(1);
        }
    }
    // Buckets repeating a name (two segments in one slot) merge, so the
    // caller never sees the same key twice with different amounts.
    let mut merged: BTreeMap<String, u64> = BTreeMap::new();
    for (bucket, share, _) in shares {
        let slot = merged.entry(bucket).or_insert(0);
        *slot = slot.saturating_add(share);
    }
    merged.into_iter().collect()
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]
mod tests {
    use super::*;
    use kernel::{B3Hash, EventDraft, Payload, RunId, Seq, TimeMs};
    use proptest::prelude::*;

    fn record(
        run: RunId,
        who: &str,
        seq: u64,
        kind: EventKind,
        data: serde_json::Value,
    ) -> EventRecord {
        let map = data.as_object().cloned().unwrap_or_default();
        let draft = EventDraft {
            run,
            t: TimeMs::new(seq),
            who: who.to_owned(),
            addr: None,
            kind,
            data: Payload::new(map).unwrap(),
            ig: false,
        };
        EventRecord::from_draft(draft, Seq::new(seq), B3Hash::digest(b""))
    }

    fn prompt(run: RunId, who: &str, seq: u64, lens: [u64; 4], window: u64) -> EventRecord {
        record(
            run,
            who,
            seq,
            EventKind::PromptAssembled,
            serde_json::json!({
                "segments": [
                    { "slot": "city", "len": lens[0] },
                    { "slot": "building", "len": lens[1] },
                    { "slot": "resident", "len": lens[2] },
                    { "slot": "run", "len": lens[3] },
                ],
                "window_bytes": window,
            }),
        )
    }

    fn sum(rows: &[(String, UsdMicros)]) -> u64 {
        rows.iter()
            .map(|(_, v)| v.get())
            .fold(0, u64::saturating_add)
    }

    #[test]
    fn a20_every_cut_sums_to_the_billed_total() {
        let run = RunId::from_bytes([1u8; 16]);
        let mut attribution = Attribution::new();
        attribution
            .apply(&prompt(run, "alice", 0, [1000, 3000, 500, 77], 420))
            .unwrap();
        attribution
            .apply(&record(
                run,
                "alice",
                1,
                EventKind::ToolResult,
                serde_json::json!({ "name": "read", "bytes": 900 }),
            ))
            .unwrap();
        attribution
            .apply(&record(
                run,
                "alice",
                2,
                EventKind::ToolResult,
                serde_json::json!({ "name": "exec", "bytes": 100 }),
            ))
            .unwrap();
        attribution
            .apply(&record(
                run,
                "alice",
                3,
                EventKind::ModelReturned,
                serde_json::json!({ "billed_usd_micros": 999_983u64 }),
            ))
            .unwrap();

        let report = attribution.report();
        assert_eq!(report.total.get(), 999_983);
        assert_eq!(sum(&report.by_run), 999_983, "by_run");
        assert_eq!(sum(&report.by_actor), 999_983, "by_actor");
        assert_eq!(sum(&report.by_segment), 999_983, "by_segment");
        assert_eq!(sum(&report.by_tool), 999_983, "by_tool");
        assert_eq!(sum(&report.by_skill), 999_983, "by_skill");
        // The cuts are cuts, not copies: five segment buckets, two tools.
        assert_eq!(report.by_segment.len(), 5);
        assert_eq!(report.by_tool.len(), 2);
        // Weight order is respected: building is the largest segment.
        let biggest = report
            .by_segment
            .iter()
            .max_by_key(|(_, v)| v.get())
            .unwrap();
        assert_eq!(biggest.0, "building");
    }

    #[test]
    fn a_call_with_no_basis_lands_in_the_honest_bucket() {
        let run = RunId::from_bytes([2u8; 16]);
        let mut attribution = Attribution::new();
        attribution
            .apply(&record(
                run,
                "bob",
                0,
                EventKind::ModelReturned,
                serde_json::json!({ "billed_usd_micros": 500u64 }),
            ))
            .unwrap();
        let report = attribution.report();
        assert_eq!(
            report.by_segment,
            vec![("unattributed".to_owned(), UsdMicros::new(500))]
        );
        assert_eq!(
            report.by_tool,
            vec![("no_tool".to_owned(), UsdMicros::new(500))]
        );
        assert_eq!(
            report.by_skill,
            vec![("no_skill".to_owned(), UsdMicros::new(500))]
        );
        assert_eq!(sum(&report.by_segment), 500);
    }

    #[test]
    fn a_call_the_provider_never_billed_attributes_nothing() {
        let run = RunId::from_bytes([3u8; 16]);
        let mut attribution = Attribution::new();
        attribution
            .apply(&prompt(run, "carol", 0, [10, 10, 10, 10], 0))
            .unwrap();
        attribution
            .apply(&record(
                run,
                "carol",
                1,
                EventKind::ModelReturned,
                serde_json::json!({ "calls": 0 }),
            ))
            .unwrap();
        let report = attribution.report();
        assert_eq!(report.total.get(), 0);
        assert!(report.by_run.is_empty(), "no invented money");
        assert!(report.by_segment.is_empty());
    }

    #[test]
    fn tool_weights_belong_to_one_wave_only() {
        let run = RunId::from_bytes([4u8; 16]);
        let mut attribution = Attribution::new();
        for (seq, name) in [(0u64, "read"), (1, "edit")] {
            attribution
                .apply(&record(
                    run,
                    "dave",
                    seq,
                    EventKind::ToolResult,
                    serde_json::json!({ "name": name, "bytes": 100 }),
                ))
                .unwrap();
        }
        attribution
            .apply(&record(
                run,
                "dave",
                2,
                EventKind::ModelReturned,
                serde_json::json!({ "billed_usd_micros": 1000u64 }),
            ))
            .unwrap();
        // Second call, no tools in between: it belongs to no_tool, not
        // to the previous wave's tools.
        attribution
            .apply(&record(
                run,
                "dave",
                3,
                EventKind::ModelReturned,
                serde_json::json!({ "billed_usd_micros": 400u64 }),
            ))
            .unwrap();
        let report = attribution.report();
        assert_eq!(sum(&report.by_tool), 1400);
        let tools: BTreeMap<String, u64> = report
            .by_tool
            .iter()
            .map(|(k, v)| (k.clone(), v.get()))
            .collect();
        assert_eq!(tools.get("read"), Some(&500));
        assert_eq!(tools.get("edit"), Some(&500));
        assert_eq!(tools.get("no_tool"), Some(&400));
    }

    proptest! {
        #![proptest_config(ProptestConfig::with_cases(256))]

        /// A20 as a property: whatever the amounts and whatever the
        /// weights, every cut still sums to the total exactly.
        #[test]
        fn a20_holds_for_arbitrary_amounts_and_weights(
            billed in prop::collection::vec(1u64..1_000_000_000, 1..8),
            lens in prop::array::uniform4(0u64..100_000),
            window in 0u64..100_000,
            tool_bytes in prop::collection::vec(0u64..10_000, 0..5),
        ) {
            let run = RunId::from_bytes([9u8; 16]);
            let mut attribution = Attribution::new();
            let mut expected: u64 = 0;
            let mut seq = 0u64;
            for amount in &billed {
                attribution.apply(&prompt(run, "p", seq, lens, window)).unwrap();
                seq = seq.saturating_add(1);
                for (i, bytes) in tool_bytes.iter().enumerate() {
                    attribution
                        .apply(&record(
                            run,
                            "p",
                            seq,
                            EventKind::ToolResult,
                            serde_json::json!({ "name": format!("t{i}"), "bytes": bytes }),
                        ))
                        .unwrap();
                    seq = seq.saturating_add(1);
                }
                attribution
                    .apply(&record(
                        run,
                        "p",
                        seq,
                        EventKind::ModelReturned,
                        serde_json::json!({ "billed_usd_micros": amount }),
                    ))
                    .unwrap();
                seq = seq.saturating_add(1);
                expected = expected.saturating_add(*amount);
            }
            let report = attribution.report();
            prop_assert_eq!(report.total.get(), expected);
            prop_assert_eq!(sum(&report.by_run), expected);
            prop_assert_eq!(sum(&report.by_actor), expected);
            prop_assert_eq!(sum(&report.by_segment), expected);
            prop_assert_eq!(sum(&report.by_tool), expected);
            prop_assert_eq!(sum(&report.by_skill), expected);
        }
    }
}
