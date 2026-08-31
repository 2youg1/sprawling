// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The sole stall criterion. Watchdog consumes the
//! verdict and decides disposal; it never re-derives the criterion —
//! "what counts as stalled" lives here and nowhere else.

use crate::consts_policy::LOOP_REPEAT_THRESHOLD;
use crate::locator::B3Hash;

/// Fingerprint of one action's canonical bytes. The only content-hash
/// producer is `B3Hash::digest`; this newtype names the purpose.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ActionFingerprint(B3Hash);

impl ActionFingerprint {
    pub fn derive(action_canonical: &[u8]) -> ActionFingerprint {
        ActionFingerprint(B3Hash::digest(action_canonical))
    }
}

/// Deliberately exhaustive verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StallVerdict {
    Ok,
    Stall { repeats: u32 },
}

/// The sample is the recent fingerprints in time order. Only the *tail*
/// run of identical prints counts: an older repetition broken by a newer
/// different action already recovered itself. `repeats` saturates by
/// contract — its only consumer compares against the threshold.
pub fn observe(recent: &[ActionFingerprint]) -> StallVerdict {
    let Some(last) = recent.last() else {
        return StallVerdict::Ok;
    };
    let mut repeats: u32 = 0;
    for print in recent.iter().rev() {
        if print == last {
            repeats = repeats.saturating_add(1);
        } else {
            break;
        }
    }
    if repeats >= LOOP_REPEAT_THRESHOLD {
        StallVerdict::Stall { repeats }
    } else {
        StallVerdict::Ok
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

    fn print(label: &str) -> ActionFingerprint {
        ActionFingerprint::derive(label.as_bytes())
    }

    #[test]
    fn empty_and_varied_histories_are_ok() {
        assert_eq!(observe(&[]), StallVerdict::Ok);
        assert_eq!(
            observe(&[print("a"), print("b"), print("c")]),
            StallVerdict::Ok
        );
    }

    #[test]
    fn three_identical_tail_actions_stall() {
        let sample = [print("x"), print("loop"), print("loop"), print("loop")];
        assert_eq!(observe(&sample), StallVerdict::Stall { repeats: 3 });
    }

    #[test]
    fn a_broken_repetition_recovered_itself() {
        // Two repeats, then something new, then two repeats again: the
        // tail run is 2, under the threshold of 3.
        let sample = [
            print("loop"),
            print("loop"),
            print("fresh"),
            print("loop"),
            print("loop"),
        ];
        assert_eq!(observe(&sample), StallVerdict::Ok);
    }

    proptest! {
        /// The verdict only ever reads the tail run length.
        #[test]
        fn stall_iff_tail_run_reaches_threshold(labels in proptest::collection::vec("[ab]", 1..12)) {
            let prints: Vec<ActionFingerprint> =
                labels.iter().map(|l| print(l)).collect();
            let last = prints.last().unwrap();
            let tail = prints.iter().rev().take_while(|p| *p == last).count();
            let verdict = observe(&prints);
            if tail >= usize::try_from(LOOP_REPEAT_THRESHOLD).unwrap() {
                prop_assert_eq!(verdict, StallVerdict::Stall { repeats: u32::try_from(tail).unwrap() });
            } else {
                prop_assert_eq!(verdict, StallVerdict::Ok);
            }
        }
    }
}
