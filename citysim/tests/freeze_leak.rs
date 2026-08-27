// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! A thousand sessions, and what is left afterwards.
//!
//! The claim a long-running city makes is that freezing a run returns
//! everything it held. That is not a claim about speed, so it is not
//! tested by timing anything: it is a claim about what a handoff keeps
//! and what a resume asks for, and the way it fails is that one of them
//! grows with the number of sessions that came before.
//!
//! The invariant is therefore a shape, not a number: the thousandth
//! resume must ask for exactly what the first one did.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code"
)]

use kernel::{Locator, RunId};
use runtime::handoff::{Handoff, resume};

const SESSIONS: u32 = 1_000;

fn locator(seed: u8) -> Locator {
    Locator::parse(&format!("cas:b3-{}", format!("{seed:02x}").repeat(32))).unwrap()
}

fn handoff_of(session: u32) -> Handoff {
    Handoff::new(
        vec![locator(0), locator(1)],
        format!("session {session}"),
        "one row claimed, one row closed".to_owned(),
        "the kiln takes six hours to cool".to_owned(),
        "close the next row".to_owned(),
    )
    .unwrap()
}

#[test]
fn a_thousand_freezes_leave_the_same_thing_behind_as_the_first() {
    let first = resume(&handoff_of(0), RunId::from_bytes([1u8; 16]));
    let mut last = first.clone();
    for session in 1..SESSIONS {
        let handoff = handoff_of(session);
        last = resume(&handoff, RunId::from_bytes([1u8; 16]));
        // Checked inside the loop rather than only at the end: a leak
        // that grew and then was trimmed would pass an assertion made
        // once, and that is exactly the shape of a leak with a cache in
        // front of it.
        assert_eq!(
            last.must_read.len(),
            first.must_read.len(),
            "session {session} asks its successor to read more than session 0 did"
        );
    }
    assert_eq!(
        last.must_read, first.must_read,
        "what a resume asks for is a property of the handoff, not of how many came before it"
    );
}

#[test]
fn a_resume_is_a_new_run_every_time_and_never_the_old_one() {
    // Rebirth, not revival. The frozen run is not a parameter, so there
    // is nothing to leak across the boundary in the first place - this
    // asserts that the identity the caller supplies is the one that
    // comes back, a thousand times over.
    let handoff = handoff_of(0);
    for session in 0..SESSIONS {
        let mut bytes = [0u8; 16];
        bytes[0] = u8::try_from(session % 256).unwrap();
        bytes[1] = u8::try_from(session / 256).unwrap();
        let seed = resume(&handoff, RunId::from_bytes(bytes));
        assert_eq!(seed.run, RunId::from_bytes(bytes));
    }
}

#[test]
fn a_handoff_that_asks_for_nothing_cannot_be_built_however_many_times_it_is_tried() {
    // The one refusal that keeps the must-read list from decaying into
    // an empty formality after a long night of sessions.
    for _ in 0..SESSIONS {
        assert!(
            Handoff::new(
                Vec::new(),
                "overview".to_owned(),
                "progress".to_owned(),
                "context".to_owned(),
                "next".to_owned(),
            )
            .is_err()
        );
    }
}
