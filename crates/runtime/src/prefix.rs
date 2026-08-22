// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Frozen prefix assembly: four segments, assembled
//! once at Run start, never reassembled within the Run. Volatile types
//! (TimeMs, usage, signals) have no conversion into `FrozenSegment` — the
//! absence of those impls is the isolation guarantee (15.3-4).

use std::collections::BTreeSet;

use kernel::consts_policy::STARTUP_BUDGET_TOKENS;
use kernel::{Address, AxCode, AxError, B3Hash, Payload, SystemBlock};
use serde_json::{Map, Value, json};

/// Documents inside one segment join on this separator; the rebuilder
/// (replay) reuses it — one authority for the concatenation rule.
pub(crate) const DOC_JOIN: &str = "\n\n";

/// The in-place truncation marker. English on purpose — the prefix faces the English window.
pub(crate) fn truncation_marker(dropped: u64) -> String {
    format!("[truncated: {dropped} bytes]")
}

/// The four slots in stability order; the order is the cache economics.
/// Exactly four — deliberately exhaustive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SegmentSlot {
    City,
    Building,
    Resident,
    Run,
}

impl SegmentSlot {
    fn as_str(self) -> &'static str {
        match self {
            SegmentSlot::City => "city",
            SegmentSlot::Building => "building",
            SegmentSlot::Resident => "resident",
            SegmentSlot::Run => "run",
        }
    }
}

/// One frozen segment: static bytes from frozen sources, hashed at
/// construction. The sole constructor takes bytes, not values — there is
/// deliberately no `From<TimeMs>` or any other volatile conversion.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrozenSegment {
    slot: SegmentSlot,
    bytes: Vec<u8>,
    hash: B3Hash,
}

impl FrozenSegment {
    pub fn new(slot: SegmentSlot, bytes: Vec<u8>) -> FrozenSegment {
        let hash = B3Hash::digest(&bytes);
        FrozenSegment { slot, bytes, hash }
    }

    pub fn slot(&self) -> SegmentSlot {
        self.slot
    }

    pub fn hash(&self) -> &B3Hash {
        &self.hash
    }

    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }
}

/// One prefix source document: address plus its frozen bytes, `None`
/// when missing or unreadable (the skip itself is accounted).
#[derive(Debug, Clone)]
pub struct SourceDoc {
    pub addr: Address,
    pub bytes: Option<Vec<u8>>,
}

/// Per-slot byte budgets. Callers derive them from config; the default
/// splits the startup budget evenly (tokens ≈ bytes/4, four slots).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SegmentCaps {
    pub city: u64,
    pub building: u64,
    pub resident: u64,
    pub run: u64,
}

impl SegmentCaps {
    pub fn startup_default() -> SegmentCaps {
        // STARTUP_BUDGET_TOKENS tokens * 4 bytes/token / 4 slots.
        let per_slot = STARTUP_BUDGET_TOKENS;
        SegmentCaps {
            city: per_slot,
            building: per_slot,
            resident: per_slot,
            run: per_slot,
        }
    }
}

/// The four source lists plus caps: everything `build_prefix` needs.
#[derive(Debug, Clone)]
pub struct PrefixPlan {
    pub city: Vec<SourceDoc>,
    pub building: Vec<SourceDoc>,
    pub resident: Vec<SourceDoc>,
    pub run: Vec<SourceDoc>,
    pub caps: SegmentCaps,
}

/// Per-segment build accounting: loaded sources and skipped documents.
struct SegmentNote {
    sources: Vec<Value>,
    skipped: Vec<Value>,
}

/// Builds the four segments from source documents: UTF-8 only, per-slot
/// caps with explicit truncation markers, cross-slot dedup by address
/// (first slot wins), skips accounted in the notes. The notes travel
/// inside the returned prefix and surface in `prompt_assembled` — they
/// are what makes the prefix offline-rebuildable (A15, C16).
pub fn build_prefix(plan: PrefixPlan) -> Result<FrozenPrefix, AxError> {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    let mut notes = Vec::new();
    let mut built = Vec::new();
    let slots = [
        (SegmentSlot::City, &plan.city, plan.caps.city),
        (SegmentSlot::Building, &plan.building, plan.caps.building),
        (SegmentSlot::Resident, &plan.resident, plan.caps.resident),
        (SegmentSlot::Run, &plan.run, plan.caps.run),
    ];
    for (slot, docs, cap) in slots {
        let (segment, note) = build_segment(slot, docs, cap, &mut seen)?;
        notes.push(note);
        built.push(segment);
    }
    let mut iter = built.into_iter();
    let (Some(city), Some(building), Some(resident), Some(run)) =
        (iter.next(), iter.next(), iter.next(), iter.next())
    else {
        return Err(AxError::failure(
            AxCode::InvalidArgs,
            "build prefix",
            "segment construction lost a slot",
        ));
    };
    let mut prefix = FrozenPrefix::assemble(city, building, resident, run)?;
    let full: Vec<Value> = prefix
        .segments()
        .iter()
        .zip(notes)
        .map(|(segment, note)| {
            json!({
                "slot": segment.slot().as_str(),
                "hash": segment.hash().to_string(),
                "len": segment.bytes().len(),
                "sources": note.sources,
                "skipped": note.skipped,
            })
        })
        .collect();
    prefix.notes = Some(json!({
        "segments": full,
        "breakpoints": ["city", "building", "resident", "run"],
    }));
    Ok(prefix)
}

fn floor_char_boundary(s: &str, max: usize) -> usize {
    if max >= s.len() {
        return s.len();
    }
    let mut end = max;
    while end > 0 && !s.is_char_boundary(end) {
        end = end.saturating_sub(1);
    }
    end
}

#[expect(
    clippy::arithmetic_side_effects,
    reason = "budget arithmetic on usize values bounded by document sizes already held \
              in memory; every subtraction is guarded by a preceding comparison"
)]
fn build_segment(
    slot: SegmentSlot,
    docs: &[SourceDoc],
    cap: u64,
    seen: &mut BTreeSet<String>,
) -> Result<(FrozenSegment, SegmentNote), AxError> {
    let cap = usize::try_from(cap).map_err(|_| {
        AxError::failure(
            AxCode::InvalidArgs,
            "build prefix segment",
            "cap exceeds usize",
        )
    })?;
    let mut text = String::new();
    let mut sources = Vec::new();
    let mut skipped = Vec::new();
    for doc in docs {
        let addr = doc.addr.as_str().to_owned();
        if seen.contains(&addr) {
            skipped.push(json!({ "addr": addr, "reason": "duplicate" }));
            continue;
        }
        let Some(bytes) = &doc.bytes else {
            skipped.push(json!({ "addr": addr, "reason": "unreadable" }));
            continue;
        };
        let Ok(body) = std::str::from_utf8(bytes) else {
            skipped.push(json!({ "addr": addr, "reason": "not_utf8" }));
            continue;
        };
        let joiner = if text.is_empty() { 0 } else { DOC_JOIN.len() };
        let used = text.len() + joiner;
        let remaining = cap.saturating_sub(used);
        if body.len() <= remaining {
            if joiner > 0 {
                text.push_str(DOC_JOIN);
            }
            text.push_str(body);
            seen.insert(addr.clone());
            sources
                .push(json!({ "addr": addr, "kept": body.len(), "marker": false, "dropped": 0 }));
            continue;
        }
        // Reserve marker room with the worst-case digit count (dropped
        // can only shrink once bytes are kept), then floor to a char
        // boundary so the segment stays valid UTF-8.
        let worst_marker = truncation_marker(u64::try_from(body.len()).unwrap_or(u64::MAX)).len();
        if remaining <= worst_marker {
            skipped.push(json!({ "addr": addr, "reason": "no_budget" }));
            continue;
        }
        let kept = floor_char_boundary(body, remaining - worst_marker);
        let dropped = u64::try_from(body.len() - kept).map_err(|_| {
            AxError::failure(
                AxCode::InvalidArgs,
                "build prefix segment",
                "length overflow",
            )
        })?;
        if joiner > 0 {
            text.push_str(DOC_JOIN);
        }
        text.push_str(body.get(..kept).unwrap_or_default());
        text.push_str(&truncation_marker(dropped));
        seen.insert(addr.clone());
        sources.push(json!({ "addr": addr, "kept": kept, "marker": true, "dropped": dropped }));
    }
    Ok((
        FrozenSegment::new(slot, text.into_bytes()),
        SegmentNote { sources, skipped },
    ))
}

/// The assembled prefix: city, building, resident, run — in that order,
/// enforced at the only constructor.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrozenPrefix {
    city: FrozenSegment,
    building: FrozenSegment,
    resident: FrozenSegment,
    run: FrozenSegment,
    notes: Option<Value>,
}

impl FrozenPrefix {
    /// Slot order is the type: a segment in the wrong position is
    /// `E_INVALID_ARGS` (fail-closed, never silently reordered).
    pub fn assemble(
        city: FrozenSegment,
        building: FrozenSegment,
        resident: FrozenSegment,
        run: FrozenSegment,
    ) -> Result<FrozenPrefix, AxError> {
        let expected = [
            (SegmentSlot::City, city.slot()),
            (SegmentSlot::Building, building.slot()),
            (SegmentSlot::Resident, resident.slot()),
            (SegmentSlot::Run, run.slot()),
        ];
        for (want, got) in expected {
            if want != got {
                return Err(AxError::failure(
                    AxCode::InvalidArgs,
                    "assemble frozen prefix",
                    format!("slot {} holds a {} segment", want.as_str(), got.as_str()),
                )
                .with_recovery("pass segments in slot order: city, building, resident, run"));
            }
        }
        Ok(FrozenPrefix {
            city,
            building,
            resident,
            run,
            notes: None,
        })
    }

    /// The wire form of the frozen prefix: four system blocks, every one
    /// an explicit cache breakpoint — exactly `CACHE_BREAKPOINTS_MAX`.
    /// Segments must be UTF-8 (build_prefix guarantees it; hand-built
    /// test prefixes must comply to reach the wire).
    pub fn system_blocks(&self) -> Result<Vec<SystemBlock>, AxError> {
        self.segments()
            .iter()
            .map(|segment| {
                let text = std::str::from_utf8(segment.bytes())
                    .map_err(|_| {
                        AxError::failure(
                            AxCode::InvalidArgs,
                            "render system blocks",
                            format!("{} segment is not utf-8", segment.slot().as_str()),
                        )
                    })?
                    .to_owned();
                Ok(SystemBlock { text, cache: true })
            })
            .collect()
    }

    pub fn segments(&self) -> [&FrozenSegment; 4] {
        [&self.city, &self.building, &self.resident, &self.run]
    }

    pub fn segment_hashes(&self) -> [B3Hash; 4] {
        [
            *self.city.hash(),
            *self.building.hash(),
            *self.resident.hash(),
            *self.run.hash(),
        ]
    }

    /// The `prompt_assembled` payload. A built prefix carries its full
    /// source notes (addresses, kept/dropped bytes, skips, breakpoints)
    /// — the C16 load-bearing part; a hand-assembled prefix reports the
    /// minimal `{slot, hash, len}` rows.
    pub fn prompt_payload(&self) -> Result<Payload, AxError> {
        if let Some(notes) = &self.notes {
            let Value::Object(map) = notes.clone() else {
                return Err(AxError::failure(
                    AxCode::InvalidArgs,
                    "encode prompt payload",
                    "prefix notes are not an object",
                ));
            };
            return Payload::new(map);
        }
        let mut segments = Vec::new();
        for segment in self.segments() {
            let mut entry = Map::new();
            entry.insert(
                "slot".to_owned(),
                Value::String(segment.slot().as_str().to_owned()),
            );
            entry.insert("hash".to_owned(), Value::String(segment.hash().to_string()));
            let len = u64::try_from(segment.bytes().len()).map_err(|_| {
                AxError::failure(
                    AxCode::InvalidArgs,
                    "encode prompt payload",
                    "segment length exceeds u64",
                )
            })?;
            entry.insert("len".to_owned(), Value::Number(len.into()));
            segments.push(Value::Object(entry));
        }
        let mut map = Map::new();
        map.insert("segments".to_owned(), Value::Array(segments));
        Payload::new(map)
    }
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

    fn four() -> (FrozenSegment, FrozenSegment, FrozenSegment, FrozenSegment) {
        (
            FrozenSegment::new(SegmentSlot::City, b"city bytes".to_vec()),
            FrozenSegment::new(SegmentSlot::Building, b"building bytes".to_vec()),
            FrozenSegment::new(SegmentSlot::Resident, b"resident bytes".to_vec()),
            FrozenSegment::new(SegmentSlot::Run, b"run bytes".to_vec()),
        )
    }

    #[test]
    fn same_input_same_bytes_same_hashes() {
        let (c, b, r, run) = four();
        let (c2, b2, r2, run2) = four();
        let one = FrozenPrefix::assemble(c, b, r, run).unwrap();
        let two = FrozenPrefix::assemble(c2, b2, r2, run2).unwrap();
        assert_eq!(one.segment_hashes(), two.segment_hashes());
        assert_eq!(
            one.prompt_payload().unwrap(),
            two.prompt_payload().unwrap(),
            "A4: same input, same payload bytes"
        );
    }

    #[test]
    fn slot_order_is_enforced() {
        let (c, b, r, run) = four();
        let err = FrozenPrefix::assemble(b, c, r, run).unwrap_err();
        assert_eq!(err.code(), &AxCode::InvalidArgs);
    }

    #[test]
    fn payload_names_all_four_slots_in_order() {
        let (c, b, r, run) = four();
        let prefix = FrozenPrefix::assemble(c, b, r, run).unwrap();
        let json = serde_json::to_value(prefix.prompt_payload().unwrap()).unwrap();
        let slots: Vec<&str> = json["segments"]
            .as_array()
            .unwrap()
            .iter()
            .map(|s| s["slot"].as_str().unwrap())
            .collect();
        assert_eq!(slots, ["city", "building", "resident", "run"]);
    }

    fn doc(addr: &str, body: &str) -> SourceDoc {
        SourceDoc {
            addr: Address::parse(addr).unwrap(),
            bytes: Some(body.as_bytes().to_vec()),
        }
    }

    fn plan() -> PrefixPlan {
        PrefixPlan {
            city: vec![doc("city.md", "be a good city")],
            building: vec![
                doc("b/building.md", "house rules"),
                doc("city.md", "duplicate of the city file"),
                SourceDoc {
                    addr: Address::parse("b/missing.md").unwrap(),
                    bytes: None,
                },
            ],
            resident: vec![doc("b/urbanite.md", "who i am")],
            run: vec![doc("b/room/job.md", "locator line")],
            caps: SegmentCaps::startup_default(),
        }
    }

    #[test]
    fn built_prefix_is_deterministic_and_notes_account_everything() {
        let one = build_prefix(plan()).unwrap();
        let two = build_prefix(plan()).unwrap();
        assert_eq!(one.segment_hashes(), two.segment_hashes());
        let payload = serde_json::to_value(one.prompt_payload().unwrap()).unwrap();
        assert_eq!(
            payload,
            serde_json::to_value(two.prompt_payload().unwrap()).unwrap(),
            "A4: same plan, same payload bytes"
        );
        // Cross-slot dedup: city.md loads once, the second hit is noted.
        let building = &payload["segments"][1];
        let skipped = building["skipped"].as_array().unwrap();
        assert!(
            skipped
                .iter()
                .any(|s| s["addr"] == "city.md" && s["reason"] == "duplicate")
        );
        assert!(
            skipped
                .iter()
                .any(|s| s["addr"] == "b/missing.md" && s["reason"] == "unreadable")
        );
        // Breakpoints: all four segment edges, never more.
        assert_eq!(payload["breakpoints"].as_array().unwrap().len(), 4);
    }

    #[test]
    fn oversized_documents_truncate_with_an_explicit_marker() {
        let mut p = plan();
        p.caps.run = 32;
        let long = "x".repeat(100);
        p.run = vec![SourceDoc {
            addr: Address::parse("b/room/job.md").unwrap(),
            bytes: Some(long.into_bytes()),
        }];
        let prefix = build_prefix(p).unwrap();
        let run_bytes = prefix.segments()[3].bytes().to_vec();
        let text = String::from_utf8(run_bytes).unwrap();
        assert!(text.len() <= 32, "cap holds including the marker");
        assert!(text.contains("[truncated: "), "never a silent tail drop");
        let payload = serde_json::to_value(prefix.prompt_payload().unwrap()).unwrap();
        let source = &payload["segments"][3]["sources"][0];
        assert_eq!(source["marker"], true);
        let kept = source["kept"].as_u64().unwrap();
        let dropped = source["dropped"].as_u64().unwrap();
        assert_eq!(kept + dropped, 100);
    }

    #[test]
    fn multibyte_truncation_lands_on_a_char_boundary() {
        let mut p = plan();
        p.caps.city = 40;
        p.city = vec![SourceDoc {
            addr: Address::parse("city.md").unwrap(),
            bytes: Some(
                "\u{4e00}\u{4e8c}\u{4e09}\u{56db}\u{4e94}\u{516d}\u{4e03}\u{516b}\u{4e5d}\u{5341}"
                    .repeat(3)
                    .into_bytes(),
            ),
        }];
        let prefix = build_prefix(p).unwrap();
        assert!(String::from_utf8(prefix.segments()[0].bytes().to_vec()).is_ok());
    }

    #[test]
    fn system_blocks_are_four_and_all_cache_marked() {
        let prefix = build_prefix(plan()).unwrap();
        let blocks = prefix.system_blocks().unwrap();
        assert_eq!(blocks.len(), 4);
        assert!(blocks.iter().all(|b| b.cache));
        assert_eq!(
            u32::try_from(blocks.iter().filter(|b| b.cache).count()).unwrap(),
            kernel::consts_external::CACHE_BREAKPOINTS_MAX
        );
    }

    #[test]
    fn hash_changes_with_a_single_byte() {
        let one = FrozenSegment::new(SegmentSlot::City, b"abc".to_vec());
        let two = FrozenSegment::new(SegmentSlot::City, b"abd".to_vec());
        assert_ne!(one.hash(), two.hash());
    }
}
