// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! What one run was given: the four frozen blocks of its prompt, and the
//! skills it was admitted (v0.0.3 cards V3.26 and V3.27).
//!
//! **Most harnesses cannot answer this.** Their prompt is a string built
//! at the moment of calling, and once the call returns there is nothing
//! left to look at. Here the prefix is frozen at run start, split into
//! four blocks whose order is the cache economics, and the assembly is
//! itself a ledger line carrying each block's hash and length. So "what
//! exactly went to the model this turn" is a question with a recorded
//! answer, and this page is that answer read back rather than a second
//! computation of it.
//!
//! **Nothing here is recomputed.** Every figure is lifted out of a
//! record this client already folds; a page that hashed the blocks again
//! would be a second authority on what the run was given, and the only
//! interesting case is the one where the two disagree.
//!
//! **A skill is compared against the last time this city looked.** The
//! shelf is hashed when it is read and the hash rides in `run_started`,
//! so an earlier run's line is a reading taken at an earlier time. A
//! document that keeps its name and changes its bytes is what an
//! injection looks like, and it is invisible to everything else in this
//! interface. The comparison is against this city's own history, not
//! against a signature: it says *this changed*, never *this is safe*.

use channels::{EventKind, EventRecord, RunId};
use dioxus::prelude::*;
use serde_json::Value;

use crate::lang::{Lang, Msg, fill, say};

/// One frozen block of the prompt, as the assembling line recorded it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    /// `city`, `building`, `resident` or `run` — the ledger's own word,
    /// never translated, because it is the same word the cost page
    /// divides spend by and the same word the replay rebuilds from.
    pub slot: String,
    pub hash: String,
    pub bytes: u64,
}

/// What has happened to one skill's bytes since the city last read them.
///
/// Exhaustive, and the three cases are genuinely different: a first
/// sighting has nothing to compare against, an unchanged file is a
/// reassurance somebody may want stated, and a changed one is the only
/// case that asks for attention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Sighting {
    First,
    Same,
    Changed { was: String },
}

/// One skill this run was admitted, and what its bytes did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Skill {
    pub name: String,
    pub hash: String,
    pub sighting: Sighting,
}

/// Everything this page says about one run, folded once.
///
/// One value rather than two queries, because the two halves answer one
/// question — what was this run given — and a caller that had to ask
/// twice could draw a prompt from one turn beside skills from another.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Given {
    /// The blocks of the newest prompt this run assembled. Empty until
    /// the first turn begins.
    pub blocks: Vec<Block>,
    /// Which turn that prompt belongs to, counting from one.
    pub turn: usize,
    pub skills: Vec<Skill>,
}

impl Given {
    /// The bytes of the whole prompt, which is the figure a person
    /// arrives wanting and the one nothing else in this client states.
    #[must_use]
    pub fn bytes(&self) -> u64 {
        self.blocks
            .iter()
            .fold(0u64, |sum, block| sum.saturating_add(block.bytes))
    }

    /// Whether any admitted skill's bytes moved. Drawn on the tab
    /// itself: a warning nobody opens the tab to see is a warning that
    /// arrives after the turn it was about.
    #[must_use]
    pub fn disturbed(&self) -> bool {
        self.skills
            .iter()
            .any(|skill| matches!(skill.sighting, Sighting::Changed { .. }))
    }
}

/// How much of a hash a person reads. Twelve hex characters name a blob
/// unambiguously in any city a person will ever open, and a full BLAKE3
/// hash across a card is a wall rather than a fact.
pub const HASH_GLIMPSE: usize = 12;

/// The short form, for a page. The whole hash stays in the ledger, which
/// is where anybody comparing hashes for real should be reading it.
#[must_use]
pub fn glimpse(hash: &str) -> &str {
    hash.get(..HASH_GLIMPSE).unwrap_or(hash)
}

/// What one run was given, from the records this client already holds.
#[must_use]
pub fn given(records: &[EventRecord], run: RunId) -> Given {
    let mut blocks = Vec::new();
    let mut turn = 0usize;
    let mut started = None;
    for record in records.iter().filter(|held| held.run() == run) {
        match record.kind() {
            EventKind::PromptAssembled => {
                // The newest wins, and the count is which turn it is:
                // one `prompt_assembled` opens each turn.
                turn = turn.saturating_add(1);
                blocks = blocks_of(record);
            }
            EventKind::RunStarted => started = Some(record),
            _ => {}
        }
    }
    let skills = match started {
        None => Vec::new(),
        Some(record) => sighted(records, record),
    };
    Given {
        blocks,
        turn,
        skills,
    }
}

/// The blocks one `prompt_assembled` line names.
fn blocks_of(record: &EventRecord) -> Vec<Block> {
    let Some(Value::Array(segments)) = record.data().as_map().get("segments") else {
        return Vec::new();
    };
    segments
        .iter()
        .filter_map(|segment| {
            let held = segment.as_object()?;
            Some(Block {
                slot: held.get("slot")?.as_str()?.to_owned(),
                hash: held.get("hash")?.as_str()?.to_owned(),
                bytes: held.get("len").and_then(Value::as_u64).unwrap_or_default(),
            })
        })
        .collect()
}

/// The skills one `run_started` line pins, each compared with the newest
/// earlier line that pinned the same name.
///
/// Earlier is by `seq`, which is the one order this city has, and it is
/// chosen by comparing sequence numbers rather than by trusting the
/// order the slice arrived in: a page that read backfill and live
/// records in the order they were handed to it would pick its
/// comparison from whichever request answered last.
fn sighted(records: &[EventRecord], started: &EventRecord) -> Vec<Skill> {
    pins_of(started)
        .into_iter()
        .map(|(name, hash)| {
            let before = records
                .iter()
                .filter(|held| held.kind() == EventKind::RunStarted && held.seq() < started.seq())
                .filter_map(|held| {
                    pins_of(held)
                        .into_iter()
                        .find(|(earlier, _)| earlier == &name)
                        .map(|(_, was)| (held.seq(), was))
                })
                .max_by_key(|(seq, _)| *seq)
                .map(|(_, was)| was);
            let sighting = match before {
                None => Sighting::First,
                Some(was) if was == hash => Sighting::Same,
                Some(was) => Sighting::Changed { was },
            };
            Skill {
                name,
                hash,
                sighting,
            }
        })
        .collect()
}

/// The `(name, hash)` pairs one `run_started` line carries.
fn pins_of(record: &EventRecord) -> Vec<(String, String)> {
    let Some(Value::Array(skills)) = record.data().as_map().get("skills") else {
        return Vec::new();
    };
    skills
        .iter()
        .filter_map(|pin| {
            let held = pin.as_object()?;
            Some((
                held.get("name")?.as_str()?.to_owned(),
                held.get("hash")?.as_str()?.to_owned(),
            ))
        })
        .collect()
}

/// What this run was sent, and what it was allowed to reach.
#[component]
pub fn PromptView(given: Given) -> Element {
    let lang = use_context::<Signal<Lang>>();
    let word = move |msg: Msg| say(lang(), msg);

    rsx! {
        crate::panel::Panel {
            title: word(Msg::PromptTitle).to_owned(),
            scope: Some(word(Msg::PromptScope).to_owned()),
            figure: Some(given.bytes().to_string()),
            source: word(Msg::PromptSource).to_owned(),
            if given.blocks.is_empty() {
                crate::panel::Empty {
                    status: word(Msg::PromptNone).to_owned(),
                    what: word(Msg::PromptNoneWhat).to_owned(),
                }
            } else {
                p { class: "panel-scope",
                    {fill(word(Msg::PromptAtTurn), &[("n", &given.turn.to_string())])}
                }
                for block in given.blocks.clone() {
                    div { key: "{block.slot}", class: "prompt-block",
                        span { class: "prompt-slot", "{block.slot}" }
                        span { class: "prompt-hash", "{glimpse(&block.hash)}" }
                        span { class: "prompt-bytes",
                            {fill(word(Msg::PromptBytes), &[("n", &block.bytes.to_string())])}
                        }
                    }
                }
            }
        }
        crate::panel::Panel {
            title: word(Msg::PromptSkillsTitle).to_owned(),
            scope: Some(word(Msg::PromptSkillsScope).to_owned()),
            figure: Some(given.skills.len().to_string()),
            source: word(Msg::PromptSkillsSource).to_owned(),
            if given.skills.is_empty() {
                crate::panel::Empty {
                    status: word(Msg::PromptNoSkills).to_owned(),
                    what: word(Msg::PromptNoSkillsWhat).to_owned(),
                }
            } else {
                for skill in given.skills.clone() {
                    div {
                        key: "{skill.name}",
                        class: if matches!(skill.sighting, Sighting::Changed { .. }) {
                            "prompt-skill alert"
                        } else {
                            "prompt-skill"
                        },
                        span { class: "prompt-name", "{skill.name}" }
                        span { class: "prompt-hash", "{glimpse(&skill.hash)}" }
                        match skill.sighting.clone() {
                            Sighting::First => rsx! {
                                span { class: "prompt-said", "{word(Msg::PromptSkillFirst)}" }
                            },
                            Sighting::Same => rsx! {
                                span { class: "prompt-said", "{word(Msg::PromptSkillSame)}" }
                            },
                            Sighting::Changed { was } => rsx! {
                                span { class: "prompt-said",
                                    {
                                        fill(
                                            word(Msg::PromptSkillChanged),
                                            &[("was", glimpse(&was))],
                                        )
                                    }
                                }
                            },
                        }
                    }
                }
            }
        }
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
    use super::{Given, Sighting, given, glimpse};
    use channels::{EventKind, EventRecord, RunId, Seq};
    use serde_json::json;

    fn record(seq: u64, run: RunId, kind: EventKind, data: serde_json::Value) -> EventRecord {
        let line = json!({
            "v": 1,
            "run": run,
            "seq": Seq::new(seq),
            "prev": "0".repeat(64),
            "t": 1_000_u64,
            "who": "resident",
            "kind": kind,
            "data": data,
        });
        EventRecord::parse_line(line.to_string().as_bytes()).unwrap()
    }

    fn started(seq: u64, run: RunId, skills: &[(&str, &str)]) -> EventRecord {
        let pins: Vec<serde_json::Value> = skills
            .iter()
            .map(|(name, hash)| json!({ "name": name, "hash": hash }))
            .collect();
        record(
            seq,
            run,
            EventKind::RunStarted,
            json!({ "task": "close the loop", "skills": pins }),
        )
    }

    fn assembled(seq: u64, run: RunId, city: &str, bytes: u64) -> EventRecord {
        record(
            seq,
            run,
            EventKind::PromptAssembled,
            json!({
                "segments": [
                    { "slot": "city", "hash": city, "len": bytes },
                    { "slot": "building", "hash": "b".repeat(64), "len": 20_u64 },
                    { "slot": "resident", "hash": "c".repeat(64), "len": 30_u64 },
                    { "slot": "run", "hash": "d".repeat(64), "len": 40_u64 },
                ]
            }),
        )
    }

    fn run(byte: u8) -> RunId {
        RunId::from_bytes([byte; 16])
    }

    /// The card's closing assertion: change one byte of a skill and the
    /// page says so. The hash the city recorded moves, and the run that
    /// was given the new bytes reports what the old ones were.
    #[test]
    fn a_skill_whose_bytes_moved_is_named_against_what_it_was() {
        let old = "a".repeat(64);
        let new = "e".repeat(64);
        let records = vec![
            started(1, run(1), &[("review", &old)]),
            started(2, run(2), &[("review", &new)]),
        ];
        let held = given(&records, run(2));
        assert_eq!(held.skills.len(), 1);
        assert_eq!(held.skills[0].sighting, Sighting::Changed { was: old });
        assert!(held.disturbed(), "the tab itself has to say so");
    }

    /// An unchanged shelf must not read as a disturbance: a warning that
    /// fires every time is a warning nobody reads.
    #[test]
    fn a_shelf_nobody_touched_reports_no_change() {
        let same = "a".repeat(64);
        let records = vec![
            started(1, run(1), &[("review", &same)]),
            started(2, run(2), &[("review", &same)]),
        ];
        let held = given(&records, run(2));
        assert_eq!(held.skills[0].sighting, Sighting::Same);
        assert!(!held.disturbed());
    }

    /// Which earlier reading to compare against is decided by `seq`, not
    /// by the order the slice arrived in: backfill and the live stream
    /// reach a page as two deliveries, and picking the last one handed
    /// over would compare against whichever request answered last.
    #[test]
    fn the_comparison_is_the_newest_earlier_reading_whatever_order_it_arrived_in() {
        let first = "a".repeat(64);
        let middle = "b".repeat(64);
        let now = "c".repeat(64);
        let records = vec![
            started(3, run(3), &[("review", &now)]),
            started(1, run(1), &[("review", &first)]),
            started(2, run(2), &[("review", &middle)]),
        ];
        assert_eq!(
            given(&records, run(3)).skills[0].sighting,
            Sighting::Changed { was: middle }
        );
    }

    /// The first time this city sees a skill there is nothing to compare
    /// against, and saying "unchanged" there would be a claim the city
    /// cannot support.
    #[test]
    fn a_skill_this_city_has_not_recorded_before_says_so() {
        let records = vec![started(1, run(1), &[("review", &"a".repeat(64))])];
        assert_eq!(given(&records, run(1)).skills[0].sighting, Sighting::First);
    }

    /// The hashes are read out of the record, never recomputed here: a
    /// page that hashed the blocks again would be a second authority on
    /// what the run was given.
    #[test]
    fn the_blocks_are_the_newest_assembly_and_the_turn_is_how_many() {
        let first = "a".repeat(64);
        let latest = "f".repeat(64);
        let records = vec![
            started(1, run(1), &[]),
            assembled(2, run(1), &first, 10),
            assembled(3, run(1), &latest, 11),
        ];
        let held = given(&records, run(1));
        assert_eq!(held.turn, 2, "one assembly opens each turn");
        assert_eq!(held.blocks.len(), 4);
        assert_eq!(held.blocks[0].hash, latest);
        assert_eq!(held.bytes(), 11 + 20 + 30 + 40);
    }

    /// Another session's records are another session's. Folding them in
    /// would draw one run's prompt on another run's page.
    #[test]
    fn only_this_run_is_read() {
        let records = vec![
            assembled(1, run(1), &"a".repeat(64), 10),
            started(2, run(2), &[]),
        ];
        assert_eq!(given(&records, run(2)), Given::default());
    }

    /// A run that has started and not yet assembled anything has no
    /// prompt to show, which the page states rather than drawing an
    /// empty frame.
    #[test]
    fn a_session_before_its_first_turn_has_nothing_to_show() {
        let records = vec![started(1, run(1), &[])];
        assert!(given(&records, run(1)).blocks.is_empty());
    }

    #[test]
    fn a_hash_is_glimpsed_rather_than_walled() {
        assert_eq!(glimpse(&"a".repeat(64)), "aaaaaaaaaaaa");
        assert_eq!(glimpse("short"), "short");
    }
}
