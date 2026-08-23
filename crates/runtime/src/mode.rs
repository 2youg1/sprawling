// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Run modes. A run sits in exactly one mode; the
//! catalog lists only that one (progressive disclosure — the other modes'
//! semantics stay out of the window).
//!
//! One exception, and it is one line long: [`change_modes_line`] names
//! the self-change modes a run is *not* in. Without it an agent working
//! in this city never learns that the city's own code and SPECs are
//! changeable at all, or under what discipline — and a capability nobody
//! is told about is one nobody uses.
//!
//! P3 adds the half that decides: [`admits`] says whether what a run
//! produced may land, given the mode it was in. The evidence arrives as
//! plain answers rather than as an `eval` type, because that crate sits
//! outside this one and the question here is not how evidence was
//! gathered but whether enough of it exists.

use crate::catalog::CatalogEntry;

#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    PlanGoal,
    Up,
    Sc,
    Ud,
    Experiment,
}

/// The name of the catalog row that opens the developer discipline.
pub const DEV_ENTRY: &str = "dev";

/// The row that tells a run this city is changeable, and nothing more.
///
/// One line in the resident segment, and the whole discipline behind an
/// expansion. That split is the point: most sessions never change this
/// city, so they pay a line; a session that is about to change it asks
/// once and gets the modes, the reading order and what to do next. The
/// same progressive disclosure the tool rows use, applied to the one
/// capability an agent would otherwise never learn it has.
#[must_use]
pub fn dev_entry() -> CatalogEntry {
    CatalogEntry {
        name: DEV_ENTRY.to_owned(),
        disclosure: "when the work is to change this city's own code, SPECs or tools, open this \
                     entry before you touch anything"
            .to_owned(),
        expansion: "This city is built from small components, each with its SPEC beside it at \
                    `crates/<crate>/<crate>-SPEC.md`. Read that SPEC, then the code, then the \
                    tests next to the code - in that order, and before you change any of them. \
                    Where the implementation would differ from the SPEC, the SPEC changes first \
                    and says why.\n\nChanging anything here happens under one of three modes, and \
                    the person grants the mode:\n- up: build one asset that has its own tests.\n\
                    - sc: renovate an existing asset without moving its observable contract.\n\
                    - ud: change behaviour, carrying held-in and held-out evidence.\n\nYour next \
                    step: say which of the three this work needs and why, and wait for the \
                    person to grant it. Do not start the change in the mode you are in now."
            .to_owned(),
    }
}

impl Mode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Mode::PlanGoal => "plan_goal",
            Mode::Up => "up",
            Mode::Sc => "sc",
            Mode::Ud => "ud",
            Mode::Experiment => "experiment",
        }
    }

    /// The catalog row for this mode: disclosure one-liner plus the
    /// expansion text (plan_goal carries its four exit conditions).
    pub fn catalog_entry(&self) -> CatalogEntry {
        let (disclosure, expansion) = match self {
            Mode::PlanGoal => (
                "plan first, then execute toward the stated goal; report when the goal is met",
                "Write the plan into Roadmap.md before edits. Exit plan_goal and work \
                 directly when any holds: the task is lightweight; no file changes; no \
                 mechanically verifiable goal; pure conversation. Record the exit reason.",
            ),
            Mode::Up => (
                "utility-production mode: build one reusable asset with tests",
                "Produce one asset, register it, and prove it with its own tests before reporting.",
            ),
            Mode::Sc => (
                "self-check mode: renovate an existing asset without changing its contract",
                "Refresh the asset; its observable contract must not move. Diff and tests are the evidence.",
            ),
            Mode::Ud => (
                "upgrade-with-double-validation mode: change behavior behind held-out evidence",
                "A behavior change needs held-in and held-out evidence before adoption (P3 wires the gates).",
            ),
            Mode::Experiment => (
                "experiment mode: explore without landing anything",
                "Nothing produced here merges; findings go to Memo.md.",
            ),
        };
        CatalogEntry {
            name: format!("mode:{}", self.as_str()),
            disclosure: disclosure.to_owned(),
            expansion: expansion.to_owned(),
        }
    }
}

/// What a run has to show for itself.
///
/// `None` is not `Some(false)`: a suite that was never run and a suite
/// that failed are different facts, and a mode that treated them alike
/// would let "we did not check" pass as "we checked and it was fine".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Produced {
    /// The asset's own tests ran and passed.
    pub tests_passed: Option<bool>,
    /// The observable contract moved under the renovation.
    pub contract_moved: bool,
    /// Held-in evidence: it did not get worse on what it was built from.
    pub held_in: Option<bool>,
    /// Held-out evidence: it stands up away from what it was built from.
    pub held_out: Option<bool>,
}

/// Whether this may land, and if not, what to do about it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Admission {
    Lands,
    Refused {
        because: &'static str,
        alternative: &'static str,
    },
}

/// The admission each mode asks for. Exhaustive on both the mode and
/// the evidence, so a new mode has to say what it demands.
///
/// The three that matter are the three the design names. UP wants the
/// asset proven by its own tests. SC wants the contract to have stayed
/// where it was, because a renovation that moves it is not a renovation.
/// UD wants both halves of the double validation, and it is the only one
/// that does: it changes how the city behaves, so confidence is not the
/// currency — evidence is.
#[must_use]
pub fn admits(mode: Mode, produced: &Produced) -> Admission {
    match mode {
        Mode::PlanGoal => Admission::Lands,
        Mode::Up => match produced.tests_passed {
            Some(true) => Admission::Lands,
            Some(false) => Admission::Refused {
                because: "the asset's own tests did not pass",
                alternative: "fix the asset or the test, then offer it again",
            },
            None => Admission::Refused {
                because: "the asset has no tests of its own",
                alternative: "write the test that would fail if this asset broke",
            },
        },
        Mode::Sc => {
            if produced.contract_moved {
                Admission::Refused {
                    because: "the asset's observable contract moved",
                    alternative: "keep the contract and renovate behind it, or do this in ud mode \
                                 with held-out evidence",
                }
            } else {
                Admission::Lands
            }
        }
        Mode::Ud => match (produced.held_in, produced.held_out) {
            (Some(true), Some(true)) => Admission::Lands,
            (Some(false), _) => Admission::Refused {
                because: "it got worse on the held-in set",
                alternative: "a change that costs more than it buys is not adopted; revise it",
            },
            (_, Some(false)) => Admission::Refused {
                because: "it did not stand up on the held-out set",
                alternative: "a gain that only shows where it was built is a gain in fitting, \
                              not in ability",
            },
            _ => Admission::Refused {
                because: "one half of the double validation is missing",
                alternative: "run the suite on both sets; an unmeasured change is not adopted on \
                              confidence",
            },
        },
        Mode::Experiment => Admission::Refused {
            because: "nothing produced in experiment mode lands",
            alternative: "write what you learned into Memo.md, then do it again in up or ud mode",
        },
    }
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

    #[test]
    fn up_wants_the_asset_proven_and_says_which_way_it_failed() {
        let untested = Produced::default();
        let Admission::Refused { because, .. } = admits(Mode::Up, &untested) else {
            panic!("an asset with no tests does not land");
        };
        assert!(because.contains("no tests"));
        let failed = Produced {
            tests_passed: Some(false),
            ..Produced::default()
        };
        let Admission::Refused { because, .. } = admits(Mode::Up, &failed) else {
            panic!("a failing asset does not land");
        };
        assert!(
            because.contains("did not pass"),
            "not the same sentence as untested"
        );
        assert_eq!(
            admits(
                Mode::Up,
                &Produced {
                    tests_passed: Some(true),
                    ..Produced::default()
                }
            ),
            Admission::Lands
        );
    }

    #[test]
    fn sc_refuses_a_renovation_that_moved_the_contract() {
        assert_eq!(admits(Mode::Sc, &Produced::default()), Admission::Lands);
        let moved = Produced {
            contract_moved: true,
            ..Produced::default()
        };
        let Admission::Refused { alternative, .. } = admits(Mode::Sc, &moved) else {
            panic!("a moved contract is not a renovation");
        };
        assert!(
            alternative.contains("ud mode"),
            "the refusal names the mode that would take it"
        );
    }

    #[test]
    fn ud_takes_nothing_on_confidence() {
        let both = Produced {
            held_in: Some(true),
            held_out: Some(true),
            ..Produced::default()
        };
        assert_eq!(admits(Mode::Ud, &both), Admission::Lands);
        for missing in [
            Produced {
                held_in: Some(true),
                ..Produced::default()
            },
            Produced {
                held_out: Some(true),
                ..Produced::default()
            },
            Produced::default(),
        ] {
            assert!(
                matches!(admits(Mode::Ud, &missing), Admission::Refused { .. }),
                "half of a double validation is not a double validation"
            );
        }
        let regressed = Produced {
            held_in: Some(true),
            held_out: Some(false),
            ..Produced::default()
        };
        let Admission::Refused { because, .. } = admits(Mode::Ud, &regressed) else {
            panic!("a change that only holds where it was built does not land");
        };
        assert!(because.contains("held-out"));
    }

    #[test]
    fn experiment_lands_nothing_however_well_it_went() {
        let excellent = Produced {
            tests_passed: Some(true),
            contract_moved: false,
            held_in: Some(true),
            held_out: Some(true),
        };
        assert!(matches!(
            admits(Mode::Experiment, &excellent),
            Admission::Refused { .. }
        ));
    }

    #[test]
    fn names_are_stable_and_entries_carry_both_levels() {
        assert_eq!(Mode::PlanGoal.as_str(), "plan_goal");
        let entry = Mode::PlanGoal.catalog_entry();
        assert_eq!(entry.name, "mode:plan_goal");
        assert!(entry.expansion.contains("Exit plan_goal"));
        assert!(!Mode::Experiment.catalog_entry().disclosure.is_empty());
    }
}
