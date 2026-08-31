// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The disposal surface. The stall verdict is
//! `kernel::stall`'s alone; this module never re-derives it and never
//! forwards its internals — it answers one question: what happens next.
//! Disposal is graded on purpose: a corrective steer first (name the
//! repetition to the model), freezing only when correction failed.
//! Terminal-only watchdogs kill recoverable sessions; that lesson is the
//! reason this type exists.

use kernel::{AxCode, AxError, Payload, StallVerdict};
use serde_json::{Map, Value};

/// Provider retries before the run freezes; a data-plane engineering
/// constant (changes pass through runtime-SPEC).
pub(crate) const WATCHDOG_PROVIDER_RETRIES: u32 = 2;

/// One watchdog per run: it holds the correction history, nothing else.
#[derive(Debug, Default)]
pub struct Watchdog {
    corrections: u32,
    provider_failures: u32,
}

/// Deliberately exhaustive: a new disposal must force every run loop to
/// decide, not fall through a catch-all.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Disposal {
    Proceed,
    CorrectiveSteer { text: String },
    Freeze { reason: FreezeReason },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FreezeReason {
    Stall,
    ProviderExhausted,
}

impl FreezeReason {
    fn as_str(self) -> &'static str {
        match self {
            FreezeReason::Stall => "stall",
            FreezeReason::ProviderExhausted => "provider_exhausted",
        }
    }
}

impl Watchdog {
    pub fn new() -> Watchdog {
        Watchdog::default()
    }

    /// Consumes the stall verdict verbatim. First hit: a corrective
    /// steer naming the repetition. Second hit: freeze.
    pub fn on_stall(&mut self, verdict: &StallVerdict) -> Disposal {
        match verdict {
            StallVerdict::Ok => Disposal::Proceed,
            StallVerdict::Stall { repeats } => {
                if self.corrections == 0 {
                    self.corrections = 1;
                    Disposal::CorrectiveSteer {
                        text: format!(
                            "You have repeated the same call {repeats} times in a row. \
                             Change the approach or report why the goal cannot be met."
                        ),
                    }
                } else {
                    Disposal::Freeze {
                        reason: FreezeReason::Stall,
                    }
                }
            }
        }
    }

    /// Provider failures retry (the retry itself is the run loop's move)
    /// until the budget is spent, then freeze.
    pub fn on_provider_failure(&mut self) -> Disposal {
        self.provider_failures = self.provider_failures.saturating_add(1);
        if self.provider_failures <= WATCHDOG_PROVIDER_RETRIES {
            Disposal::Proceed
        } else {
            Disposal::Freeze {
                reason: FreezeReason::ProviderExhausted,
            }
        }
    }

    /// The `watchdog_fired` payload (E_LOOP_SUSPECTED's carrier when the
    /// reason is a stall). Proceed never fires.
    pub fn fired_payload(&self, disposal: &Disposal) -> Result<Payload, AxError> {
        let mut map = Map::new();
        match disposal {
            Disposal::Proceed => {
                return Err(AxError::failure(
                    AxCode::InvalidArgs,
                    "encode watchdog_fired",
                    "Proceed does not fire",
                ));
            }
            Disposal::CorrectiveSteer { text } => {
                map.insert("action".to_owned(), Value::String("steer".to_owned()));
                map.insert("text".to_owned(), Value::String(text.clone()));
            }
            Disposal::Freeze { reason } => {
                map.insert("action".to_owned(), Value::String("freeze".to_owned()));
                map.insert(
                    "reason".to_owned(),
                    Value::String(reason.as_str().to_owned()),
                );
            }
        }
        map.insert(
            "corrections".to_owned(),
            Value::Number(self.corrections.into()),
        );
        map.insert(
            "provider_failures".to_owned(),
            Value::Number(self.provider_failures.into()),
        );
        Payload::new(map)
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
    use kernel::{ActionFingerprint, observe};

    #[test]
    fn disposal_is_graded_steer_first_freeze_second() {
        let mut dog = Watchdog::new();
        let same = ActionFingerprint::derive(b"exec identical");
        let sample = vec![same, same, same];
        let verdict = observe(&sample);
        let first = dog.on_stall(&verdict);
        match &first {
            Disposal::CorrectiveSteer { text } => {
                assert!(text.contains("repeated the same call 3 times"))
            }
            other => panic!("first hit must steer, got {other:?}"),
        }
        let second = dog.on_stall(&verdict);
        assert_eq!(
            second,
            Disposal::Freeze {
                reason: FreezeReason::Stall
            }
        );
        // The payload names the action; Proceed refuses to fire.
        let payload = serde_json::to_value(dog.fired_payload(&second).unwrap()).unwrap();
        assert_eq!(payload["action"], "freeze");
        assert_eq!(payload["reason"], "stall");
        assert!(dog.fired_payload(&Disposal::Proceed).is_err());
    }

    #[test]
    fn ok_verdicts_never_dispose() {
        let mut dog = Watchdog::new();
        assert_eq!(dog.on_stall(&StallVerdict::Ok), Disposal::Proceed);
        assert_eq!(dog.on_stall(&StallVerdict::Ok), Disposal::Proceed);
    }

    #[test]
    fn provider_failures_retry_then_freeze() {
        let mut dog = Watchdog::new();
        assert_eq!(dog.on_provider_failure(), Disposal::Proceed);
        assert_eq!(dog.on_provider_failure(), Disposal::Proceed);
        assert_eq!(
            dog.on_provider_failure(),
            Disposal::Freeze {
                reason: FreezeReason::ProviderExhausted
            }
        );
    }
}
