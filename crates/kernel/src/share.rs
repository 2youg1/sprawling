// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! How much of the whole plan one node is, and why nobody can mint more.
//!
//! A share exists in exactly two ways: it is the whole plan, or it is
//! one part of a share that was divided. [`Share::split`] takes its
//! input by value and hands back parts that add up to it exactly, so
//! **conservation is not a rule that is checked — it is the only thing
//! the type can express.** There is no constructor, no arithmetic, and
//! no `Deserialize`: a number read off a file cannot become a share
//! without being divided out of the whole first.
//!
//! That is what settles the estimation problem. A resident that splits
//! its own branch chooses how its own share is divided and cannot reach
//! past it, so an over-eager estimate costs its neighbours nothing and
//! the total stays 1. Capping replaces refereeing.
//!
//! Billionths rather than a fraction: two shares compare and add the
//! same way on every machine, repeated division never grows a
//! denominator, and no float ever touches a decision path
//! (ARCHITECTURE.md section 10 rule 6).

use crate::error::{AxCode, AxError};

/// Billionths of the whole plan.
///
/// Ordered so a renderer can sort branches by weight without unwrapping
/// the number first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Share(u64);

/// The whole plan, in billionths. Public because a reader comparing a
/// share against the total needs the same constant the split used.
pub const WHOLE_PPB: u64 = 1_000_000_000;

impl Share {
    /// The whole plan. The one origin: every other share descends from
    /// this one by division.
    pub const WHOLE: Share = Share(WHOLE_PPB);

    /// Nothing at all — what a branch with no leaves has done.
    ///
    /// Safe to expose because it cannot grow: no addition exists on this
    /// type, so a zero cannot be turned into a claim on the plan.
    pub const NONE: Share = Share(0);

    /// This share in billionths of the whole plan.
    #[must_use]
    pub fn ppb(self) -> u64 {
        self.0
    }

    /// Divides this share among `weights`, consuming it.
    ///
    /// The parts add up to the input exactly. Integer division leaves a
    /// remainder of fewer billionths than there are parts, and it goes
    /// to the earliest parts one at a time — a rule rather than a
    /// rounding, so the same plan divides the same way on every machine.
    ///
    /// # Errors
    /// Refuses an empty list and refuses weights that are all zero:
    /// both ask for a division with no dividend, and answering them
    /// would mean either losing the share or inventing a rule about
    /// where it went.
    pub fn split(self, weights: &[u32]) -> Result<Vec<Share>, AxError> {
        let total: u64 = weights.iter().copied().map(u64::from).sum();
        if weights.is_empty() || total == 0 {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "divide a share",
                format!("{} parts, {total} in weight", weights.len()),
            )
            .with_recovery("give at least one part, and at least one weight above zero"));
        }
        let mut parts = Vec::with_capacity(weights.len());
        let mut handed: u64 = 0;
        for weight in weights {
            let exact = u128::from(self.0)
                .saturating_mul(u128::from(*weight))
                .checked_div(u128::from(total))
                .unwrap_or(0);
            let part = u64::try_from(exact).unwrap_or(self.0);
            parts.push(part);
            handed = handed.saturating_add(part);
        }
        let mut remainder = self.0.saturating_sub(handed);
        for part in &mut parts {
            if remainder == 0 {
                break;
            }
            *part = part.saturating_add(1);
            remainder = remainder.saturating_sub(1);
        }
        Ok(parts.into_iter().map(Share).collect())
    }
}

/// Adds shares that were divided out of one whole.
///
/// A free function rather than an `Add` impl, and it takes the parts
/// together rather than two at a time: this is how a branch reports what
/// its leaves came to, not an arithmetic anyone may reach for. The sum
/// saturates at the whole, which is unreachable for parts of one plan
/// and is the honest answer for a caller that mixed two.
#[must_use]
pub fn gather(parts: &[Share]) -> Share {
    let sum = parts
        .iter()
        .fold(0u64, |held, part| held.saturating_add(part.0));
    Share(sum.min(WHOLE_PPB))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code"
)]
mod tests {
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn the_parts_add_up_to_what_was_divided() {
        let parts = Share::WHOLE.split(&[1, 1, 1]).unwrap();
        assert_eq!(parts.len(), 3);
        assert_eq!(gather(&parts), Share::WHOLE);
        // The remainder rule, stated in numbers: a billion into three is
        // not exact, and the extra billionth goes to the first part.
        assert_eq!(parts[0].ppb(), 333_333_334);
        assert_eq!(parts[1].ppb(), 333_333_333);
        assert_eq!(parts[2].ppb(), 333_333_333);
    }

    #[test]
    fn weights_are_ratios_and_nothing_else() {
        let by_ones = Share::WHOLE.split(&[1, 3]).unwrap();
        let by_hundreds = Share::WHOLE.split(&[100, 300]).unwrap();
        assert_eq!(by_ones, by_hundreds, "a ratio is not a quantity");
        assert_eq!(by_ones[1].ppb(), 750_000_000);
    }

    #[test]
    fn a_zero_weight_part_gets_nothing_and_the_rest_still_add_up() {
        let parts = Share::WHOLE.split(&[0, 1]).unwrap();
        assert_eq!(parts[0], Share::NONE);
        assert_eq!(gather(&parts), Share::WHOLE);
    }

    #[test]
    fn a_division_with_no_dividend_is_refused_rather_than_guessed() {
        let empty = Share::WHOLE.split(&[]).unwrap_err();
        assert_eq!(empty.code(), &AxCode::InvalidArgs);
        let all_zero = Share::WHOLE.split(&[0, 0]).unwrap_err();
        assert!(all_zero.recovery().contains("above zero"));
    }

    /// A share smaller than the number of parts still divides: the
    /// remainder rule hands out what there is and stops, rather than
    /// rounding something into existence.
    #[test]
    fn a_share_too_small_to_go_round_is_still_conserved() {
        let mut held = Share::WHOLE;
        // Ten halvings of a billionth-scaled whole leave a share that
        // cannot be cut three ways evenly.
        for _ in 0..10 {
            held = held.split(&[1, 1]).unwrap()[0];
        }
        let tiny = held.split(&[1, 1, 1]).unwrap();
        assert_eq!(gather(&tiny), held);
    }

    proptest! {
        /// The property the type exists for: however a plan is cut up,
        /// and however deep, the parts still add up to the whole.
        #[test]
        fn any_sequence_of_splits_conserves_the_whole(
            plan in proptest::collection::vec(
                proptest::collection::vec(0u32..8, 1..5),
                1..6,
            ),
        ) {
            let mut open = vec![Share::WHOLE];
            for weights in &plan {
                if weights.iter().copied().map(u64::from).sum::<u64>() == 0 {
                    continue;
                }
                let Some(chosen) = open.pop() else { break };
                let parts = chosen.split(weights).unwrap();
                prop_assert_eq!(gather(&parts), chosen, "one split conserves its input");
                open.extend(parts);
            }
            prop_assert_eq!(gather(&open), Share::WHOLE, "and so does every split together");
        }
    }
}
