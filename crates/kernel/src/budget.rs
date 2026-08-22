// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Money and quantities as integers (15.3-6) and the three-layer budget
//! ladder with the sub-agent context lock. Exhaustion
//! is an approval, not an error: verdicts here carry no AxError — the
//! gate shapes the escalation.

use serde::{Deserialize, Serialize};

/// One micro-USD. Decimal price lists convert at the single accounting
/// entry point (S3 gateway::cost); decisions never touch floats.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct UsdMicros(u64);

/// Whole tokens.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct Tokens(u64);

/// Whole bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ByteLen(u64);

macro_rules! quantity {
    ($name:ident) => {
        impl $name {
            pub const fn new(value: u64) -> $name {
                $name(value)
            }

            pub const fn get(self) -> u64 {
                self.0
            }

            /// Overflow is `None`; the caller owns the verdict (the spend
            /// door reads it as exhaustion, others as invalid input).
            pub fn checked_add(self, other: $name) -> Option<$name> {
                self.0.checked_add(other.0).map($name)
            }
        }
    };
}

quantity!(UsdMicros);
quantity!(Tokens);
quantity!(ByteLen);

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetCap {
    pub usd: UsdMicros,
    pub tokens: Tokens,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetUse {
    pub usd: UsdMicros,
    pub tokens: Tokens,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetLevel {
    pub cap: BudgetCap,
    pub used: BudgetUse,
}

/// City -> Building -> Run: a lower layer spends inside every upper
/// layer's remainder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetLadder {
    pub city: BudgetLevel,
    pub building: BudgetLevel,
    pub run: BudgetLevel,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetLayer {
    City,
    Building,
    Run,
}

/// Deliberately exhaustive verdict enum (verdicts are not wire enums).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SpendVerdict {
    Admit,
    Exhausted { layer: BudgetLayer },
}

fn level_admits(level: &BudgetLevel, cost: &BudgetUse) -> bool {
    let usd_fits = level
        .used
        .usd
        .checked_add(cost.usd)
        .is_some_and(|total| total <= level.cap.usd);
    let tokens_fit = level
        .used
        .tokens
        .checked_add(cost.tokens)
        .is_some_and(|total| total <= level.cap.tokens);
    usd_fits && tokens_fit
}

/// Total function: u64 overflow means the spend exceeds any representable
/// remainder, hence Exhausted (fail-closed) — never a panic, never an
/// error path. Layers are checked innermost first (Run, Building, City)
/// and the first to exhaust is named.
pub fn admit_spend(ladder: &BudgetLadder, cost: &BudgetUse) -> SpendVerdict {
    let layers = [
        (BudgetLayer::Run, &ladder.run),
        (BudgetLayer::Building, &ladder.building),
        (BudgetLayer::City, &ladder.city),
    ];
    for (layer, level) in layers {
        if !level_admits(level, cost) {
            return SpendVerdict::Exhausted { layer };
        }
    }
    SpendVerdict::Admit
}

/// The sub-agent context lock: an independent
/// token ceiling, not shared with the spawner's remainder.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CtxLock(Tokens);

impl CtxLock {
    pub const fn new(limit: Tokens) -> CtxLock {
        CtxLock(limit)
    }

    pub const fn limit(self) -> Tokens {
        self.0
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CtxVerdict {
    Within,
    Reached,
}

/// Reaching the lock freezes with `Completion::Limit` — a clear anomaly,
/// never a half-product (the wiring is the run loop's business).
pub fn observe_ctx(used: Tokens, lock: CtxLock) -> CtxVerdict {
    if used >= lock.0 {
        CtxVerdict::Reached
    } else {
        CtxVerdict::Within
    }
}

#[cfg(kani)]
mod verification {
    //! V5: the spend decision is total — arbitrary u64 inputs never
    //! panic, never wrap, and Admit implies every layer fits.

    use super::*;

    fn any_level() -> BudgetLevel {
        BudgetLevel {
            cap: BudgetCap {
                usd: UsdMicros::new(kani::any()),
                tokens: Tokens::new(kani::any()),
            },
            used: BudgetUse {
                usd: UsdMicros::new(kani::any()),
                tokens: Tokens::new(kani::any()),
            },
        }
    }

    #[kani::proof]
    fn admit_spend_is_total_and_admit_means_every_layer_fits() {
        let ladder = BudgetLadder {
            city: any_level(),
            building: any_level(),
            run: any_level(),
        };
        let cost = BudgetUse {
            usd: UsdMicros::new(kani::any()),
            tokens: Tokens::new(kani::any()),
        };
        let verdict = admit_spend(&ladder, &cost);
        if verdict == SpendVerdict::Admit {
            for level in [ladder.run, ladder.building, ladder.city] {
                assert!(level_admits(&level, &cost));
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
    use super::*;
    use proptest::prelude::*;

    fn level(cap_usd: u64, cap_tok: u64, used_usd: u64, used_tok: u64) -> BudgetLevel {
        BudgetLevel {
            cap: BudgetCap {
                usd: UsdMicros::new(cap_usd),
                tokens: Tokens::new(cap_tok),
            },
            used: BudgetUse {
                usd: UsdMicros::new(used_usd),
                tokens: Tokens::new(used_tok),
            },
        }
    }

    fn cost(usd: u64, tokens: u64) -> BudgetUse {
        BudgetUse {
            usd: UsdMicros::new(usd),
            tokens: Tokens::new(tokens),
        }
    }

    #[test]
    fn the_innermost_exhausted_layer_is_named_first() {
        let ladder = BudgetLadder {
            city: level(100, 100, 0, 0),
            building: level(100, 100, 99, 0),
            run: level(10, 100, 10, 0),
        };
        assert_eq!(
            admit_spend(&ladder, &cost(1, 1)),
            SpendVerdict::Exhausted {
                layer: BudgetLayer::Run
            }
        );
    }

    #[test]
    fn a_lower_layer_cannot_outspend_an_upper_remainder() {
        let ladder = BudgetLadder {
            city: level(100, 1000, 95, 0),
            building: level(100, 1000, 0, 0),
            run: level(100, 1000, 0, 0),
        };
        assert_eq!(
            admit_spend(&ladder, &cost(10, 1)),
            SpendVerdict::Exhausted {
                layer: BudgetLayer::City
            }
        );
    }

    #[test]
    fn overflow_is_exhaustion_not_a_panic() {
        let ladder = BudgetLadder {
            city: level(u64::MAX, u64::MAX, u64::MAX, 0),
            building: level(u64::MAX, u64::MAX, 0, 0),
            run: level(u64::MAX, u64::MAX, 0, 0),
        };
        assert_eq!(
            admit_spend(&ladder, &cost(1, 0)),
            SpendVerdict::Exhausted {
                layer: BudgetLayer::City
            }
        );
    }

    #[test]
    fn the_context_lock_is_a_closed_boundary() {
        let lock = CtxLock::new(Tokens::new(100));
        assert_eq!(observe_ctx(Tokens::new(99), lock), CtxVerdict::Within);
        assert_eq!(observe_ctx(Tokens::new(100), lock), CtxVerdict::Reached);
        assert_eq!(observe_ctx(Tokens::new(101), lock), CtxVerdict::Reached);
    }

    proptest! {
        /// Kani mirror: total on arbitrary inputs, and Admit implies fit.
        #[test]
        fn admit_spend_never_panics(caps in proptest::collection::vec(any::<u64>(), 6),
                                    used in proptest::collection::vec(any::<u64>(), 6),
                                    c in any::<u64>(), t in any::<u64>()) {
            let ladder = BudgetLadder {
                city: level(caps[0], caps[1], used[0], used[1]),
                building: level(caps[2], caps[3], used[2], used[3]),
                run: level(caps[4], caps[5], used[4], used[5]),
            };
            let spend = cost(c, t);
            let verdict = admit_spend(&ladder, &spend);
            if verdict == SpendVerdict::Admit {
                for lv in [ladder.run, ladder.building, ladder.city] {
                    prop_assert!(level_admits(&lv, &spend));
                }
            }
        }
    }
}
