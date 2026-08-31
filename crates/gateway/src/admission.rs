// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Provider-side admission: one concurrency cap and
//! one deterministic minimum launch interval per provider. AIMD without
//! clocks of its own: `now` is always a parameter, so replaying the same
//! arrival sequence yields the same verdict sequence, verbatim.

use kernel::{AxCode, AxError, TimeMs};

/// Engineering parameters of the admission ladder (provider-side facts,
/// not city policy; changes pass through gateway-SPEC).
pub(crate) const ADMISSION_MAX_IN_FLIGHT: u32 = 4;
pub(crate) const ADMISSION_MIN_INTERVAL_MS: u64 = 250;
pub(crate) const ADMISSION_MAX_INTERVAL_MS: u64 = 60_000;
pub(crate) const ADMISSION_OK_STREAK: u32 = 8;

/// Per-provider admission state. Fields private: the ladder moves only
/// through `on_dispatch`/`on_outcome`.
#[derive(Debug)]
pub struct AdmissionState {
    in_flight: u32,
    interval_ms: u64,
    consecutive_ok: u32,
    next_allowed_at: TimeMs,
}

impl Default for AdmissionState {
    fn default() -> Self {
        AdmissionState {
            in_flight: 0,
            interval_ms: ADMISSION_MIN_INTERVAL_MS,
            consecutive_ok: 0,
            next_allowed_at: TimeMs::new(0),
        }
    }
}

/// Deliberately exhaustive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AdmissionVerdict {
    Admit,
    Hold { until: TimeMs },
}

/// What one provider round reported back.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderOutcome {
    Ok,
    RateLimited { retry_after_ms: Option<u64> },
    Failed,
}

impl AdmissionState {
    pub fn new() -> AdmissionState {
        AdmissionState::default()
    }

    /// Pure verdict: does a call launch now?
    pub fn admit(&self, now: TimeMs) -> AdmissionVerdict {
        if self.in_flight >= ADMISSION_MAX_IN_FLIGHT || now < self.next_allowed_at {
            AdmissionVerdict::Hold {
                until: self.next_allowed_at,
            }
        } else {
            AdmissionVerdict::Admit
        }
    }

    /// Records a launch; refuses to launch past the cap (the caller must
    /// consult `admit` first — this is the fail-closed backstop).
    pub fn on_dispatch(&mut self, now: TimeMs) -> Result<(), AxError> {
        if let AdmissionVerdict::Hold { until } = self.admit(now) {
            return Err(AxError::failure(
                AxCode::BudgetExhausted,
                "dispatch provider call",
                format!("admission holds until {}", until.value()),
            )
            .with_recovery("wait for the interval or an in-flight slot"));
        }
        self.in_flight = self.in_flight.saturating_add(1);
        let next = now.value().saturating_add(self.interval_ms);
        self.next_allowed_at = TimeMs::new(next);
        Ok(())
    }

    /// Records an outcome: rate limits double the interval (or take the
    /// provider's retry-after when larger); a streak of clean rounds
    /// halves it back down to the floor.
    pub fn on_outcome(&mut self, outcome: ProviderOutcome, now: TimeMs) {
        self.in_flight = self.in_flight.saturating_sub(1);
        match outcome {
            ProviderOutcome::Ok => {
                self.consecutive_ok = self.consecutive_ok.saturating_add(1);
                if self.consecutive_ok >= ADMISSION_OK_STREAK {
                    self.consecutive_ok = 0;
                    self.interval_ms =
                        (self.interval_ms.saturating_div(2)).max(ADMISSION_MIN_INTERVAL_MS);
                }
            }
            ProviderOutcome::RateLimited { retry_after_ms } => {
                self.consecutive_ok = 0;
                let doubled = self
                    .interval_ms
                    .saturating_mul(2)
                    .min(ADMISSION_MAX_INTERVAL_MS);
                self.interval_ms = doubled.max(retry_after_ms.unwrap_or(0));
                let next = now.value().saturating_add(self.interval_ms);
                self.next_allowed_at = TimeMs::new(next);
            }
            ProviderOutcome::Failed => {
                self.consecutive_ok = 0;
            }
        }
    }

    pub fn interval_ms(&self) -> u64 {
        self.interval_ms
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

    /// The determinism acceptance: replaying one arrival sequence yields
    /// the same verdicts, element by element.
    #[test]
    fn the_same_arrival_sequence_replays_verbatim() {
        let run = || {
            let mut state = AdmissionState::new();
            let mut verdicts = Vec::new();
            let mut t = 0u64;
            for step in 0..24u64 {
                t += 100;
                let now = TimeMs::new(t);
                let verdict = state.admit(now);
                verdicts.push(verdict);
                if verdict == AdmissionVerdict::Admit {
                    state.on_dispatch(now).unwrap();
                    let outcome = if step % 5 == 4 {
                        ProviderOutcome::RateLimited {
                            retry_after_ms: Some(700),
                        }
                    } else {
                        ProviderOutcome::Ok
                    };
                    state.on_outcome(outcome, TimeMs::new(t + 50));
                }
            }
            verdicts
        };
        assert_eq!(run(), run());
    }

    #[test]
    fn rate_limits_widen_the_interval_and_streaks_narrow_it() {
        let mut state = AdmissionState::new();
        state.on_dispatch(TimeMs::new(1_000)).unwrap();
        state.on_outcome(
            ProviderOutcome::RateLimited {
                retry_after_ms: None,
            },
            TimeMs::new(1_100),
        );
        assert_eq!(state.interval_ms(), 500, "doubled from the floor");
        assert_eq!(
            state.admit(TimeMs::new(1_200)),
            AdmissionVerdict::Hold {
                until: TimeMs::new(1_600)
            }
        );
        // Eight clean rounds halve it back to the floor.
        let mut t = 2_000u64;
        for _ in 0..8 {
            t += 1_000;
            state.on_dispatch(TimeMs::new(t)).unwrap();
            state.on_outcome(ProviderOutcome::Ok, TimeMs::new(t + 10));
        }
        assert_eq!(state.interval_ms(), ADMISSION_MIN_INTERVAL_MS);
    }

    #[test]
    fn the_provider_retry_after_wins_when_larger() {
        let mut state = AdmissionState::new();
        state.on_dispatch(TimeMs::new(0)).unwrap();
        state.on_outcome(
            ProviderOutcome::RateLimited {
                retry_after_ms: Some(30_000),
            },
            TimeMs::new(100),
        );
        assert_eq!(state.interval_ms(), 30_000);
    }

    #[test]
    fn the_concurrency_cap_holds_and_dispatch_backstops() {
        let mut state = AdmissionState::new();
        let mut t = 0u64;
        for _ in 0..ADMISSION_MAX_IN_FLIGHT {
            t += 1_000;
            state.on_dispatch(TimeMs::new(t)).unwrap();
        }
        assert!(matches!(
            state.admit(TimeMs::new(t + 1_000)),
            AdmissionVerdict::Hold { .. }
        ));
        assert!(state.on_dispatch(TimeMs::new(t + 1_000)).is_err());
    }
}
