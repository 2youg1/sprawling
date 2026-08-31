// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The instrument, run twice.
//!
//! A suite is only useful if it says the same thing about the same work
//! on two different days. These scenarios drive `eval` the way the city
//! will: build a corpus, run it, compare, and check that the two halves
//! stay apart — because a held-out half that leaked is a measurement
//! that flatters itself and cannot be caught by looking at the number.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code"
)]

use eval::{Half, Outcome, Suite, Task};
use kernel::Locator;

fn locator(seed: u8) -> Locator {
    Locator::parse(&format!("cas:b3-{}", format!("{seed:02x}").repeat(32))).unwrap()
}

/// A corpus drawn from work the city did: six tasks, four held in, two
/// held out.
fn corpus() -> Vec<Task> {
    let mut tasks = Vec::new();
    for n in 0..6u8 {
        tasks.push(Task {
            id: format!("task-{n}"),
            at: locator(n),
            half: if n < 4 { Half::HeldIn } else { Half::HeldOut },
        });
    }
    tasks
}

fn outcomes(passing: &[&str]) -> Vec<Outcome> {
    corpus()
        .into_iter()
        .map(|task| Outcome {
            passed: passing.contains(&task.id.as_str()),
            id: task.id,
        })
        .collect()
}

#[test]
fn the_same_corpus_measured_twice_says_the_same_thing() {
    let suite = Suite::new(corpus()).unwrap();
    let once = suite.report(&outcomes(&["task-0", "task-1", "task-4"]));
    let twice = suite.report(&outcomes(&["task-0", "task-1", "task-4"]));
    assert_eq!(once, twice, "an instrument that drifts measures nothing");
    assert_eq!(once.held_in.per_mille(), 500);
    assert_eq!(once.held_out.per_mille(), 500);
}

#[test]
fn the_two_halves_are_counted_apart() {
    let suite = Suite::new(corpus()).unwrap();
    // Everything held-in passes, nothing held-out does: the shape of an
    // asset that learned its own corpus.
    let report = suite.report(&outcomes(&["task-0", "task-1", "task-2", "task-3"]));
    assert_eq!(report.held_in.per_mille(), 1000);
    assert_eq!(
        report.held_out.per_mille(),
        0,
        "the number that catches an asset fitted to what it was built from"
    );
}

#[test]
fn a_task_in_both_halves_is_refused_when_the_suite_is_built() {
    let mut leaking = corpus();
    leaking.push(Task {
        id: "task-0".to_owned(),
        at: locator(9),
        half: Half::HeldOut,
    });
    let Err(refusal) = Suite::new(leaking) else {
        panic!("the same id twice is leakage, whichever half the copy landed in");
    };
    assert!(
        refusal.subject().contains("task-0"),
        "the refusal names the task that appears twice: {}",
        refusal.subject()
    );
}

#[test]
fn answers_to_questions_this_suite_did_not_ask_are_reported_not_ignored() {
    let suite = Suite::new(corpus()).unwrap();
    let mut answers = outcomes(&["task-0"]);
    answers.push(Outcome {
        id: "task-from-another-run".to_owned(),
        passed: true,
    });
    let report = suite.report(&answers);
    assert_eq!(
        report.unknown, 1,
        "a run that answered questions nobody asked was not this suite's run"
    );
    assert_eq!(report.held_in.tried, 4, "and it did not inflate the tally");
}

#[test]
fn a_corpus_with_no_held_out_half_reports_zero_rather_than_a_full_mark() {
    let all_in: Vec<Task> = corpus()
        .into_iter()
        .map(|task| Task {
            half: Half::HeldIn,
            ..task
        })
        .collect();
    let suite = Suite::new(all_in).unwrap();
    let report = suite.report(&outcomes(&["task-0", "task-1", "task-2", "task-3"]));
    assert_eq!(report.held_out.tried, 0);
    assert_eq!(
        report.held_out.per_mille(),
        0,
        "nothing measured is not a perfect score; a ratio over nothing is zero here on purpose"
    );
}
