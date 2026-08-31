// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Admission control: the city-wide shedding posture.
//! One function serves every queue-shaped resource — signal queues and fd
//! headroom alike; capacity semantics belong to the caller. Queues and
//! counters live in memory::queue (S3), never here.

/// A queue's occupancy snapshot.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct QueueStats {
    pub depth: u64,
    pub capacity: u64,
}

/// What admitting this item would cost: one slot for a Signal, more for
/// a new-Run fd reservation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ItemMeta {
    pub cost: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShedReason {
    CapacityExhausted,
}

/// Deliberately exhaustive: a new admission outcome must force every
/// caller to decide.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Admit,
    Shed { reason: ShedReason },
}

/// Decides whether the queue admits one more item. Pure and total:
/// `depth + cost <= capacity` admits; checked arithmetic, overflow sheds
/// (fail-closed). Shedding refuses new items only — starving what is
/// already queued is a liveness property and citysim's to assert (P2).
pub fn admit(stats: &QueueStats, item: &ItemMeta) -> Admission {
    match stats.depth.checked_add(item.cost) {
        Some(total) if total <= stats.capacity => Admission::Admit,
        _ => Admission::Shed {
            reason: ShedReason::CapacityExhausted,
        },
    }
}

#[cfg(kani)]
mod verification {
    //! V5: admission is total and monotone — a shallower queue never
    //! makes the same item harder to admit.

    use super::*;

    #[kani::proof]
    fn admit_is_total_and_monotone_in_depth() {
        let capacity: u64 = kani::any();
        let deep: u64 = kani::any();
        let shallow: u64 = kani::any();
        kani::assume(shallow <= deep);
        let item = ItemMeta { cost: kani::any() };
        let deep_verdict = admit(
            &QueueStats {
                depth: deep,
                capacity,
            },
            &item,
        );
        let shallow_verdict = admit(
            &QueueStats {
                depth: shallow,
                capacity,
            },
            &item,
        );
        if deep_verdict == Admission::Admit {
            assert_eq!(shallow_verdict, Admission::Admit);
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
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn admits_up_to_capacity_and_sheds_past_it() {
        let stats = QueueStats {
            depth: 9,
            capacity: 10,
        };
        assert_eq!(admit(&stats, &ItemMeta { cost: 1 }), Admission::Admit);
        assert_eq!(
            admit(&stats, &ItemMeta { cost: 2 }),
            Admission::Shed {
                reason: ShedReason::CapacityExhausted
            }
        );
    }

    #[test]
    fn overflow_sheds_instead_of_wrapping() {
        let stats = QueueStats {
            depth: u64::MAX,
            capacity: u64::MAX,
        };
        assert_eq!(
            admit(&stats, &ItemMeta { cost: 1 }),
            Admission::Shed {
                reason: ShedReason::CapacityExhausted
            }
        );
        // Zero-cost probes still fit a full-but-not-overflowing queue.
        assert_eq!(admit(&stats, &ItemMeta { cost: 0 }), Admission::Admit);
    }

    proptest! {
        /// Kani mirror: monotone in depth.
        #[test]
        fn shallower_is_never_harder(capacity in any::<u64>(), deep in any::<u64>(),
                                     delta in any::<u64>(), cost in any::<u64>()) {
            let shallow = deep.saturating_sub(delta);
            let item = ItemMeta { cost };
            let deep_verdict = admit(&QueueStats { depth: deep, capacity }, &item);
            let shallow_verdict = admit(&QueueStats { depth: shallow, capacity }, &item);
            if deep_verdict == Admission::Admit {
                prop_assert_eq!(shallow_verdict, Admission::Admit);
            }
        }
    }
}
