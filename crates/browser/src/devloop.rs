// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! The development loop: change something, look at it, decide whether to
//! look again.
//!
//! The decision is here and the waiting is not. What this module answers
//! is "given what the last look showed, what now" — a pure question with
//! an exhaustive answer, so a loop that would never end is a thing the
//! type system can be asked about rather than a thing somebody notices
//! at three in the morning.

use kernel::{AxCode, AxError};

/// What the page looked like after the last change.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Observation {
    /// The snapshot's text. Compared rather than parsed: two identical
    /// looks mean the change did not land, whatever the page contains.
    pub text: String,
    /// Whether the page reported an error to the console or a dialog.
    pub complained: bool,
}

/// What to do next. Exhaustive: every ending is named, so "it just kept
/// going" is not one of them.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Step {
    /// The change landed and the page is quiet.
    Settled { looks: u32 },
    /// Look again; the page has changed since the last look.
    LookAgain { looks: u32 },
    /// The page said something is wrong. Carried out rather than
    /// retried: a loop that retries through a complaint is a loop that
    /// reports success on a broken page.
    Complained { looks: u32 },
    /// The page stopped changing without settling, or the budget ran
    /// out. Both are the same thing to the caller — stop and say why.
    GaveUp { looks: u32, why: String },
}

/// How many looks one loop may take. Small on purpose: a development
/// loop that needs twenty looks is not converging, and the useful thing
/// to do with it is show a person, not keep looking.
pub const LOOKS_MAX: u32 = 8;

/// How many identical looks in a row count as stopped.
pub const QUIET_LOOKS: u32 = 2;

/// The loop's whole state, which is small because the decision is.
#[derive(Debug, Clone, Default)]
pub struct DevLoop {
    looks: u32,
    same_in_a_row: u32,
    last: Option<String>,
}

impl DevLoop {
    #[must_use]
    pub fn new() -> DevLoop {
        DevLoop::default()
    }

    /// Folds one look into the decision.
    ///
    /// # Errors
    /// Refuses a look taken after the loop already ended: continuing
    /// past an ending is the caller's mistake, and answering it would
    /// hide the mistake behind a plausible verdict.
    pub fn observe(&mut self, observation: &Observation) -> Result<Step, AxError> {
        if self.looks >= LOOKS_MAX {
            return Err(AxError::failure(
                AxCode::LoopSuspected,
                "continue a development loop",
                format!("this loop already took its {LOOKS_MAX} looks"),
            )
            .with_recovery("start a new loop, or show the page to a person"));
        }
        self.looks = self.looks.saturating_add(1);
        if observation.complained {
            return Ok(Step::Complained { looks: self.looks });
        }
        let unchanged = self.last.as_deref() == Some(observation.text.as_str());
        self.same_in_a_row = if unchanged {
            self.same_in_a_row.saturating_add(1)
        } else {
            0
        };
        self.last = Some(observation.text.clone());
        if self.same_in_a_row >= QUIET_LOOKS {
            return Ok(Step::Settled { looks: self.looks });
        }
        if self.looks >= LOOKS_MAX {
            return Ok(Step::GaveUp {
                looks: self.looks,
                why: format!("the page was still changing after {LOOKS_MAX} looks"),
            });
        }
        Ok(Step::LookAgain { looks: self.looks })
    }

    #[must_use]
    pub fn looks(&self) -> u32 {
        self.looks
    }
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

    fn look(text: &str) -> Observation {
        Observation {
            text: text.to_owned(),
            complained: false,
        }
    }

    #[test]
    fn a_page_that_stops_changing_settles() {
        let mut loop_ = DevLoop::new();
        assert_eq!(
            loop_.observe(&look("a")).unwrap(),
            Step::LookAgain { looks: 1 }
        );
        assert_eq!(
            loop_.observe(&look("b")).unwrap(),
            Step::LookAgain { looks: 2 }
        );
        assert_eq!(
            loop_.observe(&look("b")).unwrap(),
            Step::LookAgain { looks: 3 }
        );
        assert_eq!(
            loop_.observe(&look("b")).unwrap(),
            Step::Settled { looks: 4 }
        );
    }

    #[test]
    fn a_complaint_ends_the_loop_rather_than_being_retried_through() {
        let mut loop_ = DevLoop::new();
        loop_.observe(&look("a")).unwrap();
        let step = loop_
            .observe(&Observation {
                text: "a".to_owned(),
                complained: true,
            })
            .unwrap();
        assert_eq!(step, Step::Complained { looks: 2 });
    }

    #[test]
    fn a_page_that_never_stops_changing_gives_up_and_says_so() {
        let mut loop_ = DevLoop::new();
        let mut last = Step::LookAgain { looks: 0 };
        for n in 0..LOOKS_MAX {
            last = loop_.observe(&look(&format!("frame {n}"))).unwrap();
        }
        match last {
            Step::GaveUp { looks, why } => {
                assert_eq!(looks, LOOKS_MAX);
                assert!(why.contains("still changing"));
            }
            other => panic!("a loop that cannot converge has to end: {other:?}"),
        }
    }

    #[test]
    fn every_loop_ends_whatever_the_page_does() {
        // The property the type cannot state: for any sequence of looks,
        // the loop reaches an ending inside its budget.
        for pattern in [0u32, 1, 2, 3] {
            let mut loop_ = DevLoop::new();
            let mut ended = false;
            for n in 0..LOOKS_MAX {
                let text = match pattern {
                    0 => "same".to_owned(),
                    1 => format!("{n}"),
                    2 => format!("{}", n % 2),
                    _ => format!("{}", n / 3),
                };
                match loop_.observe(&look(&text)).unwrap() {
                    Step::LookAgain { .. } => {}
                    _ => {
                        ended = true;
                        break;
                    }
                }
            }
            assert!(ended, "pattern {pattern} never reached an ending");
        }
    }

    #[test]
    fn looking_after_the_ending_is_the_callers_mistake_and_is_named() {
        let mut loop_ = DevLoop::new();
        for n in 0..LOOKS_MAX {
            let _ = loop_.observe(&look(&format!("{n}")));
        }
        let err = loop_.observe(&look("again")).unwrap_err();
        assert_eq!(err.code(), &AxCode::LoopSuspected);
        assert!(err.recovery().contains("person"));
    }
}
