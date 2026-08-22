// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Repair leases: when the environment itself is
//! broken, repair is serialized — one live lease per scope subtree, so
//! two runs can never "fix" the same breakage into a bigger one. The
//! active table is the caller's state; kernel only judges.

use std::collections::BTreeMap;

use crate::address::Address;
use crate::event::RunId;

/// Deliberately exhaustive verdict. Queued is a legal outcome, not an
/// error — E_REPAIR_BUSY only carries it across the tool surface (S3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RepairVerdict {
    Lease,
    Queued { holder: RunId },
}

/// Overlap either way queues (same rule as goal resources); the same
/// holder re-requesting its exact scope re-leases (idempotent recovery).
/// First overlapping holder wins the report, in BTreeMap order.
pub fn request(active: &BTreeMap<Address, RunId>, scope: &Address, who: &RunId) -> RepairVerdict {
    for (held, holder) in active {
        let overlaps = scope.is_within(held) || held.is_within(scope);
        if !overlaps {
            continue;
        }
        if held == scope && holder == who {
            return RepairVerdict::Lease;
        }
        return RepairVerdict::Queued { holder: *holder };
    }
    RepairVerdict::Lease
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

    fn addr(raw: &str) -> Address {
        Address::parse(raw).unwrap()
    }

    fn run(n: u8) -> RunId {
        RunId::parse(&format!("0198f6a2-7c4a-7bbb-9d1e-0000000000{n:02x}")).unwrap()
    }

    #[test]
    fn a_free_scope_leases() {
        let active = BTreeMap::new();
        assert_eq!(
            request(&active, &addr("b/env"), &run(1)),
            RepairVerdict::Lease
        );
    }

    #[test]
    fn overlap_queues_either_way_and_names_the_holder() {
        let mut active = BTreeMap::new();
        active.insert(addr("b/env"), run(1));
        assert_eq!(
            request(&active, &addr("b/env/tools"), &run(2)),
            RepairVerdict::Queued { holder: run(1) }
        );
        assert_eq!(
            request(&active, &addr("b"), &run(2)),
            RepairVerdict::Queued { holder: run(1) }
        );
        assert_eq!(
            request(&active, &addr("other"), &run(2)),
            RepairVerdict::Lease
        );
    }

    #[test]
    fn the_holder_re_requesting_its_exact_scope_re_leases() {
        let mut active = BTreeMap::new();
        active.insert(addr("b/env"), run(1));
        assert_eq!(
            request(&active, &addr("b/env"), &run(1)),
            RepairVerdict::Lease
        );
        // But a narrower scope by the same holder still queues: one lease,
        // one scope — narrowing is a release-and-reacquire, not a nest.
        assert_eq!(
            request(&active, &addr("b/env/x"), &run(1)),
            RepairVerdict::Queued { holder: run(1) }
        );
    }
}
