// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! What a settled asset is worth, from what it cost and what it was
//! used for.
//!
//! Assets are scored; agents are not. A run freezes and there is no "it"
//! left to keep a reputation for, so the thing that carries value across
//! sessions is what was written down — a document, a skill, a tool — and
//! that is what gets a number.
//!
//! The number is deliberately dull: uses against resident cost, in per
//! mille, with no weighting anyone has to defend. It exists to sort a
//! list so a person can look at the bottom of it. **It never decides
//! anything by itself** — deciding is `metabolism`'s, and adopting is a
//! mode's.

use kernel::{ByteLen, UsdMicros};

/// What is known about one asset over a period.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct AssetUse {
    /// How many runs actually reached for it.
    pub uses: u32,
    /// What it costs to keep in front of a model: the bytes it occupies
    /// in a prefix every time it is disclosed.
    pub resident: ByteLen,
    /// What was billed to the work that used it, over the same period.
    pub billed: UsdMicros,
    /// Days since something used it.
    pub idle_days: u32,
}

/// One asset's standing.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Score {
    /// Uses per thousand resident bytes. An asset that earns its place
    /// is one that gets reached for often enough to pay for the space it
    /// takes in every prompt that discloses it.
    pub per_mille: u32,
    /// Carried alongside rather than folded in: an asset can be cheap
    /// and useless or expensive and indispensable, and a single number
    /// that hid which was which would be worse than two.
    pub idle_days: u32,
}

/// Scores one asset.
///
/// An asset that occupies nothing scores by uses alone — a rule of the
/// city rather than a special case: something that costs no resident
/// bytes cannot be charged for them.
#[must_use]
pub fn score(usage: &AssetUse) -> Score {
    let bytes = usage.resident.get();
    let per_mille = if bytes == 0 {
        usage.uses.saturating_mul(1000)
    } else {
        let scaled = u64::from(usage.uses)
            .saturating_mul(1000)
            .saturating_mul(1000);
        let ratio = scaled.checked_div(bytes).unwrap_or(0);
        u32::try_from(ratio).unwrap_or(u32::MAX)
    };
    Score {
        per_mille,
        idle_days: usage.idle_days,
    }
}

/// Sorts assets worst first, which is the order the list is read in.
///
/// Worst is lowest score, and idleness breaks the tie: between two
/// assets nobody uses, the one nobody has used for longer goes first.
/// Ties beyond that keep the caller's order, which is stable, so the
/// same register renders the same list twice.
#[must_use]
pub fn worst_first<T: Clone>(assets: &[(T, Score)]) -> Vec<(T, Score)> {
    let mut sorted = assets.to_vec();
    sorted.sort_by(|left, right| {
        left.1
            .per_mille
            .cmp(&right.1.per_mille)
            .then(right.1.idle_days.cmp(&left.1.idle_days))
    });
    sorted
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

    #[test]
    fn an_asset_that_is_reached_for_scores_above_one_that_is_not() {
        let used = score(&AssetUse {
            uses: 40,
            resident: ByteLen::new(2_000),
            billed: UsdMicros::new(0),
            idle_days: 0,
        });
        let hoarded = score(&AssetUse {
            uses: 1,
            resident: ByteLen::new(2_000),
            billed: UsdMicros::new(0),
            idle_days: 90,
        });
        assert!(used.per_mille > hoarded.per_mille);
    }

    #[test]
    fn size_is_what_a_use_is_measured_against() {
        let small = score(&AssetUse {
            uses: 10,
            resident: ByteLen::new(500),
            ..AssetUse::default()
        });
        let large = score(&AssetUse {
            uses: 10,
            resident: ByteLen::new(50_000),
            ..AssetUse::default()
        });
        assert!(
            small.per_mille > large.per_mille,
            "the same usefulness costs more when it takes more room in every prompt"
        );
    }

    #[test]
    fn nothing_resident_is_charged_for_nothing() {
        let free = score(&AssetUse {
            uses: 3,
            resident: ByteLen::new(0),
            ..AssetUse::default()
        });
        assert_eq!(free.per_mille, 3_000);
    }

    #[test]
    fn the_list_is_read_worst_first_and_idleness_breaks_the_tie() {
        let sorted = worst_first(&[
            (
                "fresh",
                Score {
                    per_mille: 0,
                    idle_days: 1,
                },
            ),
            (
                "useful",
                Score {
                    per_mille: 900,
                    idle_days: 0,
                },
            ),
            (
                "forgotten",
                Score {
                    per_mille: 0,
                    idle_days: 200,
                },
            ),
        ]);
        let order: Vec<&str> = sorted.iter().map(|(name, _)| *name).collect();
        assert_eq!(order, vec!["forgotten", "fresh", "useful"]);
    }
}
