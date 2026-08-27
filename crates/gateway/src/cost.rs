// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! Per-call settlement: the authoritative billed
//! amount always wins over price-sheet arithmetic; the sheet is the
//! fallback, not a second opinion. Checked integer math end to end —
//! an overflowing settlement is an error, never a wrapped number.

use kernel::{AxCode, AxError, ModelUsage, UsdMicros};

use crate::market::ModelEntry;

/// Where the settled number came from.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CostSource {
    Authoritative,
    PriceSheet,
}

/// One call's settled cost, ready for the `model_returned` payload.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CallCost {
    pub billed: UsdMicros,
    pub source: CostSource,
    pub usage: ModelUsage,
}

const TOKENS_PER_PRICE_UNIT: u64 = 1_000_000;

fn share(tokens: u64, price_per_mtok: UsdMicros, what: &str) -> Result<u64, AxError> {
    let product = tokens.checked_mul(price_per_mtok.get()).ok_or_else(|| {
        AxError::failure(
            AxCode::InvalidArgs,
            "settle call cost",
            format!("{what}: token-price product overflows"),
        )
    })?;
    Ok(product.div_euclid(TOKENS_PER_PRICE_UNIT))
}

/// Settles one call. `authoritative` is the provider-reported billed
/// amount when present.
pub fn settle(
    usage: &ModelUsage,
    authoritative: Option<UsdMicros>,
    entry: &ModelEntry,
) -> Result<CallCost, AxError> {
    if let Some(billed) = authoritative {
        return Ok(CallCost {
            billed,
            source: CostSource::Authoritative,
            usage: *usage,
        });
    }
    let mut total: u64 = 0;
    for (tokens, price, what) in [
        (usage.input_tokens.get(), entry.input_price, "input"),
        (usage.output_tokens.get(), entry.output_price, "output"),
        (
            usage.cache_read_tokens.get(),
            entry.cache_read_price,
            "cache_read",
        ),
        (
            usage.cache_write_tokens.get(),
            entry.cache_write_price,
            "cache_write",
        ),
    ] {
        let part = share(tokens, price, what)?;
        total = total.checked_add(part).ok_or_else(|| {
            AxError::failure(
                AxCode::InvalidArgs,
                "settle call cost",
                format!("{what}: settlement total overflows"),
            )
        })?;
    }
    Ok(CallCost {
        billed: UsdMicros::new(total),
        source: CostSource::PriceSheet,
        usage: *usage,
    })
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]
mod tests {
    use super::*;
    use crate::market::MarketSnapshot;
    use kernel::Tokens;

    fn usage(input: u64, output: u64, read: u64, write: u64) -> ModelUsage {
        ModelUsage {
            input_tokens: Tokens::new(input),
            output_tokens: Tokens::new(output),
            cache_read_tokens: Tokens::new(read),
            cache_write_tokens: Tokens::new(write),
        }
    }

    #[test]
    fn the_authoritative_amount_always_wins() {
        let market = MarketSnapshot::builtin();
        let entry = market.lookup("claude-sonnet").unwrap();
        let cost = settle(&usage(1_000_000, 0, 0, 0), Some(UsdMicros::new(42)), entry).unwrap();
        assert_eq!(cost.billed, UsdMicros::new(42));
        assert_eq!(cost.source, CostSource::Authoritative);
    }

    #[test]
    fn the_price_sheet_computes_integer_shares() {
        let market = MarketSnapshot::builtin();
        let entry = market.lookup("claude-sonnet").unwrap();
        // 1 Mtok in at $3 + 100k out at $15 + 200k cache-read at $0.30.
        let cost = settle(&usage(1_000_000, 100_000, 200_000, 0), None, entry).unwrap();
        assert_eq!(cost.billed, UsdMicros::new(3_000_000 + 1_500_000 + 60_000));
        assert_eq!(cost.source, CostSource::PriceSheet);
        // A zero-usage call settles to zero, legally.
        let free = settle(&usage(0, 0, 0, 0), None, entry).unwrap();
        assert_eq!(free.billed, UsdMicros::new(0));
    }

    #[test]
    fn overflowing_settlements_are_errors_not_wraps() {
        let market = MarketSnapshot::builtin();
        let entry = market.lookup("claude-sonnet").unwrap();
        let err = settle(&usage(u64::MAX, 0, 0, 0), None, entry).unwrap_err();
        assert!(err.subject().contains("overflow"));
    }
}
