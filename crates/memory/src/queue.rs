// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! One queue implementation serving three lanes — signals, approvals,
//! repairs.
//!
//! Two orderings carry the whole design. Admission is decided before
//! anything is stored, so a shed item never occupies a slot it was
//! refused; and deduplication happens before consumption, so a key that
//! has already been handed out cannot reach the consumer twice. An
//! effect that runs twice is worse than one that never runs, and this
//! is the module that makes the difference structural rather than
//! careful.
//!
//! Nothing here is persistent. The queue is a projection of enqueued
//! minus consumed — the Ledger is already the history, and a second
//! durable copy would be a second answer.

use std::collections::{BTreeMap, BTreeSet};

use kernel::{Admission, IdemKey, ItemMeta, Payload, QueueStats, TimeMs, admit};

use crate::jsonl::MemoryError;

/// Which lane a queue serves. The lanes differ in their accounting
/// names and nothing else; the day they differ in behaviour is the day
/// they become separate modules, and that exit condition is stated
/// here rather than discovered later.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QueueLane {
    Signal,
    Approval,
    Repair,
}

impl QueueLane {
    pub fn as_str(self) -> &'static str {
        match self {
            QueueLane::Signal => "signal",
            QueueLane::Approval => "approval",
            QueueLane::Repair => "repair",
        }
    }
}

#[derive(Debug, Clone)]
pub struct QueueItem {
    pub id: u64,
    pub key: IdemKey,
    pub payload: Payload,
    pub enqueued_t: TimeMs,
}

pub struct EventQueue {
    lane: QueueLane,
    items: BTreeMap<u64, QueueItem>,
    next_id: u64,
    /// Every key ever admitted. Membership, not occupancy: an item that
    /// has been consumed must still be recognised as a duplicate.
    seen: BTreeSet<IdemKey>,
    capacity: u64,
    shed: u64,
    duplicates: u64,
}

impl EventQueue {
    pub fn new(lane: QueueLane, capacity: u64) -> EventQueue {
        EventQueue {
            lane,
            items: BTreeMap::new(),
            next_id: 0,
            seen: BTreeSet::new(),
            capacity,
            shed: 0,
            duplicates: 0,
        }
    }

    /// Admission first, then storage. A duplicate key is admitted-and-
    /// ignored rather than refused: the sender did its job, and telling
    /// it otherwise would invite a retry that changes nothing.
    pub fn enqueue(
        &mut self,
        key: IdemKey,
        payload: Payload,
        now: TimeMs,
    ) -> Result<Admission, MemoryError> {
        if self.seen.contains(&key) {
            self.duplicates = self.duplicates.saturating_add(1);
            return Ok(Admission::Admit);
        }
        let verdict = admit(&self.stats(), &ItemMeta { cost: 1 });
        match verdict {
            Admission::Shed { .. } => {
                // Shedding refuses the new item and touches nothing that
                // is already queued.
                self.shed = self.shed.saturating_add(1);
                Ok(verdict)
            }
            Admission::Admit => {
                let id = self.next_id;
                self.next_id = self.next_id.saturating_add(1);
                self.seen.insert(key);
                self.items.insert(
                    id,
                    QueueItem {
                        id,
                        key,
                        payload,
                        enqueued_t: now,
                    },
                );
                Ok(Admission::Admit)
            }
        }
    }

    /// Takes the oldest item. The key stays in `seen`, so a late
    /// duplicate arriving after consumption is still recognised.
    pub fn consume(&mut self) -> Option<QueueItem> {
        let id = self.items.keys().next().copied()?;
        self.items.remove(&id)
    }

    pub fn stats(&self) -> QueueStats {
        QueueStats {
            depth: self.len(),
            capacity: self.capacity,
        }
    }

    pub fn len(&self) -> u64 {
        u64::try_from(self.items.len()).unwrap_or(u64::MAX)
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn lane(&self) -> QueueLane {
        self.lane
    }

    /// Counters for the accounting payloads: how many were refused, how
    /// many arrived twice.
    pub fn shed_count(&self) -> u64 {
        self.shed
    }

    pub fn duplicate_count(&self) -> u64 {
        self.duplicates
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
    use kernel::{RunId, Seq};

    fn key(n: u8) -> IdemKey {
        IdemKey::derive(&RunId::from_bytes([n; 16]), Seq::new(u64::from(n)), b"act")
    }

    fn payload(tag: &str) -> Payload {
        let mut map = serde_json::Map::new();
        map.insert("tag".to_owned(), serde_json::Value::String(tag.to_owned()));
        Payload::new(map).unwrap()
    }

    #[test]
    fn dedup_happens_before_the_consumer_ever_sees_it() {
        let mut queue = EventQueue::new(QueueLane::Signal, 8);
        let k = key(1);
        queue.enqueue(k, payload("first"), TimeMs::new(0)).unwrap();
        queue.enqueue(k, payload("again"), TimeMs::new(1)).unwrap();
        assert_eq!(queue.len(), 1, "the second never became an item");
        assert_eq!(queue.duplicate_count(), 1);
        let item = queue.consume().unwrap();
        assert_eq!(item.key, k);
        assert!(queue.consume().is_none());
        // A duplicate arriving after consumption is still a duplicate:
        // the effect already ran once.
        queue.enqueue(k, payload("late"), TimeMs::new(2)).unwrap();
        assert!(queue.is_empty(), "a consumed key cannot come back");
        assert_eq!(queue.duplicate_count(), 2);
    }

    #[test]
    fn shedding_refuses_the_newcomer_and_keeps_what_is_queued() {
        let mut queue = EventQueue::new(QueueLane::Approval, 2);
        for n in 1..=2u8 {
            assert_eq!(
                queue
                    .enqueue(key(n), payload("in"), TimeMs::new(0))
                    .unwrap(),
                Admission::Admit
            );
        }
        let verdict = queue
            .enqueue(key(3), payload("over"), TimeMs::new(0))
            .unwrap();
        assert!(matches!(verdict, Admission::Shed { .. }));
        assert_eq!(queue.len(), 2, "nothing already queued was dropped");
        assert_eq!(queue.shed_count(), 1);
        // The shed key was never admitted, so a retry is not a duplicate.
        queue.consume();
        assert_eq!(
            queue
                .enqueue(key(3), payload("retry"), TimeMs::new(1))
                .unwrap(),
            Admission::Admit
        );
        assert_eq!(queue.duplicate_count(), 0);
    }

    #[test]
    fn consumption_is_arrival_ordered() {
        let mut queue = EventQueue::new(QueueLane::Repair, 8);
        for n in 1..=4u8 {
            queue
                .enqueue(
                    key(n),
                    payload(&format!("item{n}")),
                    TimeMs::new(u64::from(n)),
                )
                .unwrap();
        }
        let ids: Vec<u64> = std::iter::from_fn(|| queue.consume())
            .map(|item| item.id)
            .collect();
        assert_eq!(ids, vec![0, 1, 2, 3]);
    }

    #[test]
    fn three_lanes_one_implementation() {
        for lane in [QueueLane::Signal, QueueLane::Approval, QueueLane::Repair] {
            let mut queue = EventQueue::new(lane, 4);
            queue.enqueue(key(9), payload("x"), TimeMs::new(0)).unwrap();
            assert_eq!(queue.len(), 1);
            assert_eq!(queue.lane(), lane);
            assert!(!lane.as_str().is_empty());
        }
    }
}
