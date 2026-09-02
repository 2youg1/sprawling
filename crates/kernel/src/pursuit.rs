// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! A pursuit: a goal the city keeps working towards, and the one
//! condition under which a city that holds one stops.
//!
//! The construction plan called this Endless. It is named for what it
//! is rather than for how long it lasts, because three other things in
//! this repository are already called standing something.
//!
//! **A society does not stop because somebody shouts stop. It stops
//! because there is nothing ready to do.** That is the whole criterion:
//! the ready set is empty and no run is still going. Both halves are
//! needed — an empty ready set while four runs are working means the
//! work is in somebody's hands, not that it is finished.
//!
//! Money is deliberately not a criterion. The cost surface in this city
//! is material an agent optimises against, not a brake somebody pulls,
//! and a stop condition that read a budget would answer a question
//! nobody here asked.
//!
//! **A pursuit is the person's, and a delegate cannot declare
//! one.** [`Pursuit::declare`] takes the depth-zero position by
//! reference, and a `Delegate` value has no way to produce one — the
//! same two-layer guard `crate::delegation` uses, for the same reason:
//! a sub-agent that could set the city working until it runs out of work
//! is a sub-agent that can spend the night on its own idea.
//!
//! Pausing and clearing are different actions and both exist. Pause
//! keeps the goal and stops taking work; clearing is dropping the value,
//! so a cleared goal leaves nothing behind to be resumed by accident.
//! Cancelling a *run* is a third thing again, and it lives where runs do.

use crate::delegation::Delegator;
use crate::node_id::NodeId;

/// Whether a pursuit is taking work right now.
///
/// Carries serde because a page renders it. The value that must not be
/// deserialisable is [`Pursuit`] itself — and that still is not.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PursuitState {
    Running,
    Paused,
}

/// A goal that keeps taking work until the work runs out.
///
/// No serde: a pursuit that could be deserialised would be a pursuit a
/// wire frame could declare, and the point of the `Delegator` argument
/// is that declaring one is the person's.
#[derive(Debug)]
pub struct Pursuit {
    goal: String,
    state: PursuitState,
}

impl Pursuit {
    /// Declares one. The `Delegator` is the depth-zero position: holding
    /// one is assembly's discipline, and a `Delegate` cannot produce
    /// one.
    ///
    /// # Errors
    /// Refuses a blank goal. An empty string here would mean "keep
    /// working on nothing in particular", and the city already has a
    /// word for that situation — it is a plan with a ready set.
    pub fn declare(_at: &Delegator, goal: String) -> Result<Pursuit, crate::error::AxError> {
        if goal.trim().is_empty() {
            return Err(crate::error::AxError::failure(
                crate::error::AxCode::InvalidArgs,
                "declare a pursuit",
                "the goal is blank",
            )
            .with_recovery("say what the city is working towards, in one sentence"));
        }
        Ok(Pursuit {
            goal,
            state: PursuitState::Running,
        })
    }

    #[must_use]
    pub fn goal(&self) -> &str {
        &self.goal
    }

    #[must_use]
    pub fn state(&self) -> PursuitState {
        self.state
    }

    /// Stops taking work without forgetting what the work was for.
    pub fn pause(&mut self) {
        self.state = PursuitState::Paused;
    }

    pub fn resume(&mut self) {
        self.state = PursuitState::Running;
    }
}

/// What a city holding a pursuit does next. Exhaustive: every arm is
/// something the caller has to do, and a fifth would be a state nobody
/// wrote an action for.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PursuitVerdict {
    /// Start this node. The first of the ready set, so the order is the
    /// plan's own and two cities with the same plan pick the same node.
    Work { next: NodeId },
    /// Nothing is ready and somebody is still working: the ready set
    /// will change when they are done.
    Waiting { in_flight: u32 },
    /// The pursuit is held but not taking work.
    Paused,
    /// Nothing ready, nobody working. The city has finished, and nobody
    /// had to say so.
    Finished,
}

/// The stop condition, as a pure function of what the plan says and what
/// is running.
///
/// Takes the state rather than the pursuit: the verdict does not depend
/// on what the goal says, and a reader that had to hold a [`Pursuit`] to
/// ask this question would need the depth-zero position to *read* the
/// city. Declaring is the guarded act; looking is not.
///
/// No clock, no disk, no counter of its own: the caller already knows
/// both facts, and a stop condition that went and looked for them could
/// not be replayed.
#[must_use]
pub fn observe(state: PursuitState, ready: &[NodeId], in_flight: u32) -> PursuitVerdict {
    if state == PursuitState::Paused {
        return PursuitVerdict::Paused;
    }
    match ready.first() {
        Some(next) => PursuitVerdict::Work { next: next.clone() },
        None if in_flight > 0 => PursuitVerdict::Waiting { in_flight },
        None => PursuitVerdict::Finished,
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

    fn goal() -> Pursuit {
        Pursuit::declare(&Delegator::root(), "raise the east wing".to_owned()).unwrap()
    }

    fn node(raw: &str) -> NodeId {
        NodeId::parse(raw).unwrap()
    }

    #[test]
    fn work_is_taken_from_the_front_of_the_ready_set() {
        let verdict = observe(goal().state(), &[node("1.2"), node("3")], 0);
        assert_eq!(verdict, PursuitVerdict::Work { next: node("1.2") });
    }

    /// The criterion, both halves. An empty ready set alone does not
    /// mean finished: the work may be in somebody's hands.
    #[test]
    fn a_city_stops_when_nothing_is_ready_and_nobody_is_working() {
        assert_eq!(
            observe(goal().state(), &[], 2),
            PursuitVerdict::Waiting { in_flight: 2 }
        );
        assert_eq!(observe(goal().state(), &[], 0), PursuitVerdict::Finished);
    }

    #[test]
    fn a_paused_goal_takes_nothing_and_keeps_what_it_was_for() {
        let mut pursuit = goal();
        pursuit.pause();
        assert_eq!(
            observe(pursuit.state(), &[node("1")], 0),
            PursuitVerdict::Paused
        );
        assert_eq!(pursuit.goal(), "raise the east wing");
        pursuit.resume();
        assert_eq!(
            observe(pursuit.state(), &[node("1")], 0),
            PursuitVerdict::Work { next: node("1") }
        );
    }

    #[test]
    fn a_blank_goal_is_refused_where_it_is_declared() {
        let refusal = Pursuit::declare(&Delegator::root(), "   ".to_owned()).unwrap_err();
        assert_eq!(refusal.code(), &crate::error::AxCode::InvalidArgs);
        assert!(refusal.recovery().contains("one sentence"));
    }
}
