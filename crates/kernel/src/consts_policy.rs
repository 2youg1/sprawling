// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! Policy constants: our choices. Changing one
//! changes behavior and therefore requires EVAL evidence or an
//! explicit ruling. Data only — zero branches by charter.
//!
//! All fifteen policy entries are landed; the three type-bearing
//! ones arrived with their defining cards (kernel-SPEC 8-8):
//! AUTONOMY_DEFAULT (S2.08), CLOCK_STAMP_DEFAULT (S2.09),
//! SUBAGENT_CTX_LOCK_DEFAULT (S2.06).

/// Exact ratio as an integer pair: kernel decision paths never touch
/// floats (determinism rule 6). Kept unreduced so the spelling mirrors the
/// value as it was decided.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ratio {
    pub num: u32,
    pub den: u32,
}

pub const STARTUP_BUDGET_TOKENS: u64 = 2000;

/// 0.5: the past-half context reminder threshold.
pub const CTX_REMINDER_RATIO: Ratio = Ratio { num: 1, den: 2 };

pub const LOOP_REPEAT_THRESHOLD: u32 = 3;

/// Floor, not threshold: the effective offload bound is derived by
/// `pipeline` from window headroom (4.4).
pub const OFFLOAD_MIN_BYTES: u64 = 16_384;

pub const DRAFT_HELD_ESCALATE: u32 = 3;

pub const EDIT_WAR_FREEZE: u32 = 2;

/// 3.5 bits/char. Second constant scheduled for re-estimation; evidence =
/// false-positive rate on repository and fixture corpora (7.1).
pub const SECRET_ENTROPY_MIN: Ratio = Ratio { num: 7, den: 2 };

pub const DISCARD_FILES_MAX: u32 = 16;

pub const DISCARD_BYTES_MAX: u64 = 1_048_576;

pub const DISCARD_RETENTION_DAYS: u32 = 30;

pub const POLICY_IDLE_DAYS: u32 = 90;

pub const CLOCK_ZONES_MAX: u32 = 4;

/// Instruction budget for one sandboxed call when no layer states one
/// (P4.02). Large enough that ordinary work finishes, small enough that
/// a loop stops rather than runs until somebody notices: the point of
/// fuel is that exhaustion is a verdict the city writes down, not a
/// machine that gets slow.
pub const SANDBOX_FUEL_DEFAULT: u64 = 200_000_000;

/// 2 GiB per node's working tree. A ceiling rather than a free-space
/// probe: free space is a moving fact about one machine, while this is
/// the number a refusal can state and a person can raise. Checked
/// before a tree is created, so an over-large city is refused rather
/// than half-copied (11.4).
pub const WORKTREE_MAX_BYTES: u64 = 2_147_483_648;

/// Off by default: zero window bytes until a Building opts in (4.3).
pub const CLOCK_STAMP_DEFAULT: crate::config::ClockStampGranularity =
    crate::config::ClockStampGranularity::Off;

/// 64Ki tokens. Constitution names no value; kernel-SPEC 14 carries the
/// evidence: mainstream windows run 128k-200k, an Ephemeral task profile
/// bounds near 60k, so 2^16 sits above the task ceiling and below half
/// the smallest mainstream window — self-justification loops get cut at a
/// third of the parent's window cost. EVAL re-estimates in P3.
pub const SUBAGENT_CTX_LOCK_DEFAULT: crate::budget::CtxLock =
    crate::budget::CtxLock::new(crate::budget::Tokens::new(65_536));

/// The human answers by default; loosening is an explicit command.
pub const AUTONOMY_DEFAULT: crate::approval::Autonomy = crate::approval::Autonomy::Owner;

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
    fn the_twelve_landed_policies_hold_their_documented_values() {
        assert_eq!(STARTUP_BUDGET_TOKENS, 2000);
        assert_eq!(CTX_REMINDER_RATIO, Ratio { num: 1, den: 2 });
        assert_eq!(LOOP_REPEAT_THRESHOLD, 3);
        assert_eq!(OFFLOAD_MIN_BYTES, 16_384);
        assert_eq!(DRAFT_HELD_ESCALATE, 3);
        assert_eq!(EDIT_WAR_FREEZE, 2);
        assert_eq!(SECRET_ENTROPY_MIN, Ratio { num: 7, den: 2 });
        assert_eq!(DISCARD_FILES_MAX, 16);
        assert_eq!(DISCARD_BYTES_MAX, 1_048_576);
        assert_eq!(DISCARD_RETENTION_DAYS, 30);
        assert_eq!(POLICY_IDLE_DAYS, 90);
        assert_eq!(CLOCK_ZONES_MAX, 4);
        assert_eq!(SANDBOX_FUEL_DEFAULT, 200_000_000);
    }

    #[test]
    fn ratios_never_divide_by_zero() {
        for ratio in [CTX_REMINDER_RATIO, SECRET_ENTROPY_MIN] {
            assert_ne!(ratio.den, 0);
        }
    }
}
