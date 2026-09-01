// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! What a session is doing, in the one vocabulary every surface reads
//! from (web-SPEC.md section 8-53 B2).
//!
//! Four drawings used to say this and none of them agreed: a progress
//! bar's `running`/`done`/`blocked`, `RunPhase::as_str`, an attention
//! row borrowing ALERT for its count, and a cluster borrowing ALERT for
//! its border. This module is the only producer of the mark, so the
//! sessions list, the session page, the nav badge, the waiting page and
//! the city drawing cannot drift apart.
//!
//! **Not the same question as `memory::hot::RunPhase`.** That one
//! answers "is this run still open", which the server needs in order to
//! decide what to schedule. This one answers "what is it doing", which
//! is what a person reads. A run that is open is either `Running` or
//! `Waiting` here, and the split is the whole point: only one of them
//! needs somebody.
//!
//! **Shape carries the meaning; colour is the redundant layer.** The
//! five marks differ by fill, outline and corner in `assets/app.css`, so
//! a person who cannot separate the hues still separates the phases.
//! ALERT is spent on `Waiting` alone, because ALERT means one thing in
//! this interface — a person is needed — and a palette that says it five
//! times says it nowhere.

use crate::lang::Msg;

/// What a session is doing. Exhaustive and closed: a client that cannot
/// name a state draws a blank where a person expected a word, and there
/// is no `Unknown` to fall into because every arrival is classified at
/// the point it is folded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Phase {
    /// The city is working on it right now.
    Running,
    /// It stopped and cannot continue until a person answers.
    Waiting,
    /// It ended on its own, by finishing or by running out of turns.
    Frozen,
    /// A person stopped it.
    Cancelled,
    /// The whole city was stopped, so this stopped with it.
    Halted,
}

/// Every phase, in the order a list reads best: what needs a person, then
/// what is moving, then what has ended.
///
/// Ordering is a display decision and belongs beside the marks rather
/// than in each page that sorts. `Waiting` leads because the one thing a
/// person opened this page to find is the thing that is waiting for them.
pub const READING_ORDER: [Phase; 5] = [
    Phase::Waiting,
    Phase::Running,
    Phase::Frozen,
    Phase::Cancelled,
    Phase::Halted,
];

impl Phase {
    /// The class this phase's mark is drawn with.
    ///
    /// The token is the whole contract with `assets/app.css`: the sheet
    /// declares `.phase.running` and the four beside it, and a token
    /// with no rule renders as an unstyled empty span. The assertion
    /// below reads the shipped sheet rather than a copy of its names.
    #[must_use]
    pub fn token(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Waiting => "waiting",
            Self::Frozen => "frozen",
            Self::Cancelled => "cancelled",
            Self::Halted => "halted",
        }
    }

    /// What a person reads, and what a screen reader is given.
    ///
    /// A `Msg` rather than a `&str`, so a phase cannot reach a Chinese
    /// page in English. The mark itself carries no text, so this is also
    /// its accessible name rather than a decoration beside one.
    #[must_use]
    pub fn word(self) -> Msg {
        match self {
            Self::Running => Msg::PhaseRunning,
            Self::Waiting => Msg::PhaseWaiting,
            Self::Frozen => Msg::PhaseFrozen,
            Self::Cancelled => Msg::PhaseCancelled,
            Self::Halted => Msg::PhaseHalted,
        }
    }

    /// Whether this session is still the city's to move.
    ///
    /// The one authority for a rule that was written three times as
    /// `matches!(row.phase, Running | AwaitingApproval)` — twice in the
    /// overview and once in the app. Three copies of a rule is three
    /// chances to answer a question two ways, and this one decides both
    /// what the first screen counts and what the nav badge shows.
    ///
    /// `Waiting` is in flight: the work is not over, it is stopped on a
    /// person. Counting it as finished is how a queue of approvals
    /// becomes invisible.
    #[must_use]
    pub fn in_flight(self) -> bool {
        match self {
            Self::Running | Self::Waiting => true,
            Self::Frozen | Self::Cancelled | Self::Halted => false,
        }
    }

    /// Whether this session is stopped on a person rather than on the
    /// city. The one input to the waiting count, kept here so the badge
    /// and the waiting page cannot disagree about what they are counting.
    #[must_use]
    pub fn needs_a_person(self) -> bool {
        matches!(self, Self::Waiting)
    }

    /// The phase a `run_frozen` record names, read from its `completion`
    /// field.
    ///
    /// The field is written by `kernel::completion::Completion::name`,
    /// and the three spellings there are the three answered here.
    /// Anything else is `Frozen`: the run did end, and inventing a
    /// fourth ending from an unread word would be worse than reporting
    /// the ending that certainly happened.
    #[must_use]
    pub fn ended_as(completion: Option<&str>) -> Self {
        match completion {
            Some("cancelled") => Self::Cancelled,
            _ => Self::Frozen,
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
    use super::{Phase, READING_ORDER};

    /// The shipped stylesheet, so the assertions below read the bytes a
    /// browser gets rather than a second copy of them kept in this file.
    const SHEET: &str = include_str!("../assets/app.css");

    /// Every phase, proven exhaustive by a match that stops compiling
    /// when a variant is added without a decision about where it sorts.
    fn every_phase() -> [Phase; 5] {
        for phase in READING_ORDER {
            match phase {
                Phase::Running
                | Phase::Waiting
                | Phase::Frozen
                | Phase::Cancelled
                | Phase::Halted => {}
            }
        }
        READING_ORDER
    }

    #[test]
    fn no_two_phases_share_a_mark() {
        let mut seen = Vec::new();
        for phase in every_phase() {
            assert!(
                !seen.contains(&phase.token()),
                "{phase:?} draws the same mark as an earlier phase"
            );
            seen.push(phase.token());
        }
        assert_eq!(seen.len(), 5);
    }

    /// A token with no rule is an empty span: the mark disappears and the
    /// row silently loses the only thing that said what it was doing.
    #[test]
    fn the_shipped_sheet_draws_every_mark() {
        for phase in every_phase() {
            let rule = format!(".phase.{}", phase.token());
            assert!(
                SHEET.contains(&rule),
                "{rule} is emitted by this module and styled by nothing"
            );
        }
    }

    /// The mechanical form of "colour is the redundant layer": ALERT is
    /// spent once, on the one phase that means a person is needed.
    #[test]
    fn alert_is_spent_on_the_one_phase_that_needs_a_person() {
        let mut coloured = Vec::new();
        for phase in every_phase() {
            let head = format!(".phase.{}", phase.token());
            let Some(at) = SHEET.find(&head) else {
                panic!("{head} is not in the sheet");
            };
            let rest = SHEET.get(at..).unwrap_or_default();
            let Some(end) = rest.find('}') else {
                panic!("{head} has no closing brace");
            };
            if rest.get(..end).unwrap_or_default().contains("--ALERT") {
                coloured.push(phase);
            }
        }
        assert_eq!(
            coloured,
            vec![Phase::Waiting],
            "ALERT means one thing in this interface, and these phases all claim it"
        );
    }

    /// The rule three modules used to keep their own copy of.
    #[test]
    fn what_counts_as_in_flight_is_stopped_work_plus_running_work() {
        assert!(Phase::Running.in_flight());
        assert!(
            Phase::Waiting.in_flight(),
            "stopped on a person is not over"
        );
        assert!(!Phase::Frozen.in_flight());
        assert!(!Phase::Cancelled.in_flight());
        assert!(!Phase::Halted.in_flight());
    }

    /// Exactly one phase asks for a person, so the badge and the waiting
    /// page count the same set.
    #[test]
    fn one_phase_asks_for_a_person() {
        let asking: Vec<Phase> = every_phase()
            .into_iter()
            .filter(|phase| phase.needs_a_person())
            .collect();
        assert_eq!(asking, vec![Phase::Waiting]);
    }

    /// The word `kernel::completion` writes is the word read back here.
    /// A run a person stopped must not read as one that ran out of turns:
    /// the two call for different next actions.
    #[test]
    fn a_cancelled_run_is_not_read_as_a_finished_one() {
        assert_eq!(Phase::ended_as(Some("cancelled")), Phase::Cancelled);
        assert_eq!(Phase::ended_as(Some("done")), Phase::Frozen);
        assert_eq!(Phase::ended_as(Some("limit")), Phase::Frozen);
        assert_eq!(Phase::ended_as(None), Phase::Frozen);
        assert_eq!(Phase::ended_as(Some("something new")), Phase::Frozen);
    }

    /// What waits on a person is read first, because it is the reason the
    /// page was opened.
    #[test]
    fn the_reading_order_leads_with_what_waits() {
        assert_eq!(READING_ORDER[0], Phase::Waiting);
        assert!(READING_ORDER[0].needs_a_person());
    }
}
