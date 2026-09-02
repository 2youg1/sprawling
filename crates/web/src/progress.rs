// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The one place a progress bar is drawn. Three callers
//! share it - the city view, the live session, the dashboard - so a bar
//! means the same thing wherever it appears.
//!
//! **A17's type half is already won upstream.** `Progress::Unplanned` has no
//! `ratio` method, so this module cannot paint a percentage it does not
//! know; the compiler enforces that, not a check here. What this module owes
//! is the other half: what to draw *instead*, and three states a person can
//! still tell apart after the colour is removed.
//!
//! Redundancy is per-state and not a promise:
//!
//! | state | colour | what survives desaturation |
//! |---|---|---|
//! | running | G4 track, ACCENT head | darkest of the three |
//! | done | G10 to ACCENT gradient | brightest of the three |
//! | blocked | ALERT segment | a rule at the segment end - a shape |
//!
//! No fill animation. A flowing stripe or a pulsing indeterminate sweep
//! manufactures attention without carrying information; a number that
//! changes eases over 90ms and nothing else moves.

use channels::Progress;
use dioxus::prelude::*;

/// The three states a bar can be in. Exhaustive, and each one carries its
/// own non-hue encoding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BarState {
    Running,
    Done,
    /// Blocked or awaiting a person. Amber is not a new meaning here: ALERT
    /// has always meant "a person is needed", and being blocked on approval
    /// is exactly that.
    Blocked,
}

impl BarState {
    /// The token that fills the completed span.
    #[must_use]
    pub fn fill_token(self) -> &'static str {
        match self {
            Self::Running => "ACCENT",
            Self::Done => "PROGRESS_DONE",
            Self::Blocked => "ALERT",
        }
    }

    /// Lightness of the fill, per mille, once chroma is gone. This is what
    /// the desaturated snapshot compares, so the three must differ.
    #[must_use]
    pub fn desaturated_lightness(self) -> u16 {
        match self {
            Self::Running => 680,
            Self::Done => 930,
            Self::Blocked => 900,
        }
    }

    /// Whether the bar carries a rule at the end of the filled span. Only
    /// the blocked state does; it is the shape that survives both the loss
    /// of hue and a lightness collision.
    #[must_use]
    pub fn has_end_rule(self) -> bool {
        matches!(self, Self::Blocked)
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Done => "done",
            Self::Blocked => "blocked",
        }
    }
}

/// A bar, described rather than drawn: which state, how far along, and the
/// words beside it. Returning a description keeps the decision testable
/// without a renderer, and leaves the markup to the component.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Bar {
    pub state: BarState,
    /// Filled span in per mille. `None` when there is no denominator - the
    /// bar then shows steps and budget instead of a fraction.
    pub filled: Option<u16>,
    /// The status word beside the bar: `4/9`, `7 steps`, `blocked`.
    pub label: String,
}

/// What the bar is about.
///
/// The two subjects differ only where there is no denominator, and there
/// they differ completely: a run without a plan has still walked steps and
/// spent money, while a building whose `Roadmap.md` could not be read has
/// no numbers at all. Rendering the second as "0 steps" would state that
/// nothing has happened there, which is not what is known.
///
/// One parameter rather than two functions, because the words for a plan
/// that *is* readable must not be able to drift apart between callers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Subject {
    /// A plan and its rows: a building's `Roadmap.md`.
    Plan,
    /// One run, in flight or finished.
    Run,
}

/// Describes the bar for one item.
///
/// A17: an item with a Roadmap shows its completion fraction; an item
/// without one never shows a percentage. The second half is not a rule this
/// function follows - it is the only thing it *can* do, because
/// `UnplannedProgress` carries no denominator to divide by.
#[must_use]
pub fn bar(progress: &Progress, blocked: bool, subject: Subject, lang: crate::lang::Lang) -> Bar {
    match *progress {
        Progress::Planned(planned) => {
            let (done, total) = planned.ratio();
            let state = if blocked || planned.blocked > 0 {
                BarState::Blocked
            } else if total > 0 && done >= total {
                BarState::Done
            } else {
                BarState::Running
            };
            Bar {
                state,
                filled: Some(per_mille_of(done, total)),
                label: format!("{done}/{total}"),
            }
        }
        Progress::Unplanned(unplanned) => {
            let state = if blocked {
                BarState::Blocked
            } else {
                BarState::Running
            };
            // No denominator, so no fraction. What is shown instead is
            // what is actually known - and for a plan that is nothing at
            // all, because a building's roadmap carries neither steps nor
            // spend, and printing its zeroes would report a run that never
            // happened.
            let label = match subject {
                Subject::Plan => {
                    crate::lang::say(lang, crate::lang::Msg::ProgressNoPlan).to_owned()
                }
                // Money only when there is some: zero and unknown are
                // different, and a subscription reports neither. Same rule
                // the spend line follows.
                Subject::Run if unplanned.budget.usd.get() == 0 => {
                    format!("{} steps", unplanned.steps)
                }
                Subject::Run => format!(
                    "{} steps · {}",
                    unplanned.steps,
                    crate::readout::render_usd(unplanned.budget.usd)
                ),
            };
            Bar {
                state,
                filled: None,
                label,
            }
        }
    }
}

/// The bar, drawn. The one place in the library that draws one.
///
/// A bar with no denominator renders an empty track and no fill: a
/// zero-width fill would claim that nothing is done, which is a different
/// statement from not knowing how much is.
#[component]
pub fn ProgressBar(bar: Bar) -> Element {
    rsx! {
        div { class: "bar {bar.state.as_str()}",
            span { class: "track",
                if let Some(filled) = bar.filled {
                    // Per mille to per cent without leaving integers: the
                    // browser does the division, so no float enters here.
                    span { class: "fill", style: "width: calc({filled} * 0.1%)" }
                }
            }
            span { class: "bar-label", "{bar.label}" }
        }
    }
}

/// Filled span, per mille. Saturates rather than exceeding full: a count
/// beyond the denominator is a bug elsewhere, and a bar past its own end is
/// a lie about it.
#[must_use]
pub fn per_mille_of(done: u32, total: u32) -> u16 {
    if total == 0 {
        return 0;
    }
    let scaled = u64::from(done).saturating_mul(1000);
    let share = scaled.checked_div(u64::from(total)).unwrap_or_default();
    u16::try_from(share.min(1000)).unwrap_or(1000)
}

/// The track behind the fill. One token, every state, so an unfilled span
/// reads the same wherever it appears.
#[must_use]
pub fn track_token() -> &'static str {
    "G4"
}

/// Whether the three states remain distinguishable with chroma at zero.
///
/// This is A17's last clause as a function rather than as a screenshot: two
/// states may share a lightness only if one of them also carries a shape.
#[must_use]
pub fn distinguishable_without_colour() -> bool {
    let states = [BarState::Running, BarState::Done, BarState::Blocked];
    states.iter().enumerate().all(|(index, left)| {
        states.iter().skip(index.saturating_add(1)).all(|right| {
            left.desaturated_lightness() != right.desaturated_lightness()
                || left.has_end_rule() != right.has_end_rule()
        })
    })
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
    use channels::{BudgetUse, PlannedProgress, Tokens, UnplannedProgress, UsdMicros};

    fn planned(done: u32, blocked: u32, total: u32) -> Progress {
        Progress::Planned(PlannedProgress {
            done,
            blocked,
            total,
            done_ppb: 0,
            blocked_ppb: 0,
        })
    }

    fn unplanned(steps: u32) -> Progress {
        Progress::Unplanned(UnplannedProgress {
            steps,
            budget: BudgetUse {
                usd: UsdMicros::new(420_000),
                tokens: Tokens::new(1_234),
            },
        })
    }

    #[test]
    fn a17_an_item_without_a_roadmap_never_shows_a_percentage() {
        let Bar { filled, label, .. } =
            bar(&unplanned(7), false, Subject::Run, crate::lang::Lang::En);
        assert!(filled.is_none(), "there is no denominator to divide by");
        assert!(!label.contains('%'));
        assert!(!label.contains('/'), "a fraction would imply a total");
        assert!(label.contains("7 steps"));
        assert!(label.contains("$0.42"), "budget burned stands in for it");
    }

    #[test]
    fn a_plan_that_could_not_be_read_reports_nothing_rather_than_zeroes() {
        // A building carries neither steps nor spend, so the same
        // `Unplanned` value means "no numbers" here and "these numbers"
        // for a run. Printing "0 steps" under a building would report a
        // run that never happened.
        let Bar { filled, label, .. } =
            bar(&unplanned(0), false, Subject::Plan, crate::lang::Lang::En);
        assert!(filled.is_none());
        assert_eq!(label, "no plan");
        assert!(!label.contains('$'), "a plan has no money of its own");
    }

    #[test]
    fn a_run_that_cost_nothing_yet_says_nothing_about_money() {
        // Zero and unknown are different, and a subscription reports
        // neither price nor bill.
        let free = Progress::Unplanned(UnplannedProgress {
            steps: 3,
            budget: BudgetUse::default(),
        });
        let label = bar(&free, false, Subject::Run, crate::lang::Lang::En).label;
        assert_eq!(label, "3 steps");
        assert!(!label.contains('$'));
    }

    #[test]
    fn a17_an_item_with_a_roadmap_shows_its_fraction() {
        let Bar {
            filled,
            label,
            state,
        } = bar(
            &planned(4, 0, 9),
            false,
            Subject::Plan,
            crate::lang::Lang::En,
        );
        assert_eq!(label, "4/9");
        assert_eq!(filled, Some(444));
        assert_eq!(state, BarState::Running);
    }

    #[test]
    fn a17_the_three_states_survive_the_loss_of_colour() {
        assert!(distinguishable_without_colour());
        // Concretely: done is the brightest, running the darkest, and
        // blocked - which sits close to done in lightness - is the only one
        // carrying a rule.
        assert!(BarState::Done.desaturated_lightness() > BarState::Running.desaturated_lightness());
        assert!(BarState::Blocked.has_end_rule());
        assert!(!BarState::Done.has_end_rule());
        assert!(!BarState::Running.has_end_rule());
    }

    #[test]
    fn blocked_wins_over_finished_because_a_person_is_still_needed() {
        // A row can be complete on paper and still be waiting on somebody.
        // Painting it done would hide the thing the interface exists to
        // surface.
        assert_eq!(
            bar(
                &planned(9, 1, 9),
                false,
                Subject::Plan,
                crate::lang::Lang::En
            )
            .state,
            BarState::Blocked
        );
        assert_eq!(
            bar(
                &planned(9, 0, 9),
                true,
                Subject::Plan,
                crate::lang::Lang::En
            )
            .state,
            BarState::Blocked
        );
        assert_eq!(
            bar(
                &planned(9, 0, 9),
                false,
                Subject::Plan,
                crate::lang::Lang::En
            )
            .state,
            BarState::Done
        );
    }

    #[test]
    fn a_bar_never_runs_past_its_own_end() {
        assert_eq!(per_mille_of(10, 9), 1000);
        assert_eq!(per_mille_of(0, 0), 0, "no denominator, no fill");
        assert_eq!(per_mille_of(u32::MAX, 1), 1000);
        assert_eq!(per_mille_of(1, 3), 333);
    }

    #[test]
    fn every_token_the_bar_names_exists_in_the_theme() {
        let known: Vec<&str> = crate::theme::GRAY_RAMP
            .iter()
            .map(|(name, _)| *name)
            .chain(crate::theme::COLOUR_TOKENS.iter().map(|(name, ..)| *name))
            .chain(["PROGRESS_DONE"])
            .collect();
        for state in [BarState::Running, BarState::Done, BarState::Blocked] {
            assert!(
                known.contains(&state.fill_token()),
                "{} names an unknown token",
                state.as_str()
            );
        }
        assert!(known.contains(&track_token()));
    }
}
