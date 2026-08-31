// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Clearing out.
//!
//! Accumulation without metabolism is debt: every settled asset costs
//! resident bytes in the prompts that disclose it, disk in every export,
//! and attention in every list it appears in. So the city has a cycle
//! that says what stops being carried.
//!
//! Two rules keep this from being destructive. **Nothing is deleted** —
//! the strongest verdict is `Retire`, which stops an asset being
//! disclosed and leaves it exactly where it is; the Discard register is
//! the only thing that removes anything, and it is reversible by
//! construction. And **the reason travels with the verdict**, because a
//! list of things that vanished with no explanation teaches people to
//! stop trusting the cycle that vanished them.

use crate::score::{AssetUse, Score};

/// How long an asset may sit unused before the cycle notices it. Stated
/// here rather than taken from a caller, so two cities run the same
/// cycle; `kernel::consts_policy::POLICY_IDLE_DAYS` governs the policy
/// half and this governs assets.
pub const ASSET_IDLE_DAYS: u32 = 90;

/// The score below which an asset is not paying for the room it takes.
/// One use per thousand resident bytes over the period.
pub const ASSET_FLOOR_PER_MILLE: u32 = 1_000;

/// What the cycle decided about one asset.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disposal {
    /// It is carrying its weight; nothing happens.
    Keep,
    /// Nobody has used it for a season. It stays disclosed for one more
    /// cycle and is named in the report, which is how a person gets the
    /// chance to say it matters before it stops being offered.
    Warn { because: String },
    /// It stops being disclosed. The bytes stay on disk and in history;
    /// what ends is its place in every prompt.
    Retire { because: String },
}

impl Disposal {
    #[must_use]
    pub fn because(&self) -> &str {
        match self {
            Disposal::Keep => "it is used often enough for what it costs",
            Disposal::Warn { because } | Disposal::Retire { because } => because,
        }
    }
}

/// One asset, one verdict.
///
/// The order is the point. Idleness is read before value, because an
/// asset nobody has touched for a season is a fact that needs no
/// arithmetic; and a warning always precedes a retirement, so nothing
/// stops being offered in the same cycle that first noticed it.
#[must_use]
pub fn dispose(usage: &AssetUse, score: Score, warned_already: bool) -> Disposal {
    let idle = usage.idle_days >= ASSET_IDLE_DAYS;
    let thin = score.per_mille < ASSET_FLOOR_PER_MILLE;
    if !idle && !thin {
        return Disposal::Keep;
    }
    let because = if idle && thin {
        format!(
            "nothing has used it for {} days, and it was reached for {} times for the room it takes",
            usage.idle_days, usage.uses
        )
    } else if idle {
        format!("nothing has used it for {} days", usage.idle_days)
    } else {
        format!(
            "it was reached for {} times for the room it takes in every prompt that offers it",
            usage.uses
        )
    };
    if warned_already {
        Disposal::Retire { because }
    } else {
        Disposal::Warn { because }
    }
}

/// One pass of the cycle over a register.
///
/// Returns the verdicts in the caller's order, so a report reads the way
/// the register does. Nothing here writes anything: the cycle decides,
/// and `kernel::registry` is the only place an asset's standing changes.
#[must_use]
pub fn sweep<T: Clone>(assets: &[(T, AssetUse, Score, bool)]) -> Vec<(T, Disposal)> {
    assets
        .iter()
        .map(|(subject, usage, score, warned)| (subject.clone(), dispose(usage, *score, *warned)))
        .collect()
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
    use crate::score::score;
    use kernel::ByteLen;

    fn usage(uses: u32, bytes: u64, idle_days: u32) -> AssetUse {
        AssetUse {
            uses,
            resident: ByteLen::new(bytes),
            idle_days,
            ..AssetUse::default()
        }
    }

    #[test]
    fn a_full_cycle_warns_first_and_retires_second() {
        let forgotten = usage(0, 4_000, 120);
        let standing = score(&forgotten);
        let first = dispose(&forgotten, standing, false);
        assert!(matches!(first, Disposal::Warn { .. }));
        let second = dispose(&forgotten, standing, true);
        assert!(matches!(second, Disposal::Retire { .. }));
        assert!(
            second.because().contains("120 days"),
            "the reason travels with the verdict: {}",
            second.because()
        );
    }

    #[test]
    fn an_asset_that_pays_for_its_room_is_left_alone() {
        let busy = usage(60, 3_000, 2);
        assert_eq!(dispose(&busy, score(&busy), true), Disposal::Keep);
    }

    #[test]
    fn a_heavy_asset_that_is_barely_used_is_noticed_even_while_it_is_fresh() {
        let bloated = usage(1, 200_000, 0);
        let verdict = dispose(&bloated, score(&bloated), false);
        assert!(matches!(verdict, Disposal::Warn { .. }));
        assert!(verdict.because().contains("room it takes"));
    }

    #[test]
    fn a_sweep_reports_in_the_registers_own_order() {
        let a = usage(0, 4_000, 200);
        let b = usage(50, 1_000, 0);
        let verdicts = sweep(&[("old", a, score(&a), true), ("busy", b, score(&b), false)]);
        assert_eq!(verdicts.len(), 2);
        assert_eq!(verdicts[0].0, "old");
        assert!(matches!(verdicts[0].1, Disposal::Retire { .. }));
        assert_eq!(verdicts[1].1, Disposal::Keep);
    }
}
