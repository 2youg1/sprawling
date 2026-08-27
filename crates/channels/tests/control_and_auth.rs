// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! The two judgements a control surface owns: which frames intervene in a
//! running Run and what an intervention must leave behind, and whether the
//! peer presenting a pairing token is the one we minted it for.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code"
)]

use channels::{Command, ControlVerdict, HaltScope, Intervention, PairingToken};
use channels::{classify, verify};
use kernel::{Address, GitOid, IdemKey, RunId, Seq};

fn key() -> IdemKey {
    IdemKey::derive(&RunId::from_bytes([3u8; 16]), Seq::new(1), b"control")
}

fn run() -> RunId {
    RunId::from_bytes([9u8; 16])
}

// ------------------------------------------------------------------- control

#[test]
fn interrupting_a_live_run_always_owes_a_handoff() {
    // Constitution 1.7: "any interruption ends with a Handoff, so whoever
    // comes next - human or Agent - receives the complete scene."
    let interrupting = [
        Command::Steer {
            run: run(),
            text: "try the other branch".to_owned(),
            idem: key(),
        },
        Command::Cancel {
            run: run(),
            idem: key(),
        },
        Command::Takeover {
            run: run(),
            idem: key(),
        },
        Command::Rollback {
            checkpoint: GitOid::from_bytes([0x5au8; 20]),
            idem: key(),
        },
    ];
    for command in interrupting {
        let name = command.name();
        let ControlVerdict::Intervene {
            must_write_handoff, ..
        } = classify(&command)
        else {
            panic!("`{name}` is one of the five verbs");
        };
        assert!(must_write_handoff, "`{name}` must end with a Handoff");
    }
}

#[test]
fn halting_a_scope_intervenes_without_naming_a_run() {
    for command in [
        Command::Halt {
            scope: HaltScope::City,
            idem: key(),
        },
        Command::Release {
            scope: HaltScope::City,
            idem: key(),
        },
    ] {
        let name = command.name();
        let ControlVerdict::Intervene {
            run,
            must_write_handoff,
            ..
        } = classify(&command)
        else {
            panic!("`{name}` intervenes at the scope level");
        };
        assert!(run.is_none(), "`{name}` stops a slice, not one Run");
        assert!(
            !must_write_handoff,
            "`{name}` interrupts nobody's turn, so it owes no Handoff"
        );
    }
}

#[test]
fn the_verbs_are_exactly_the_ones_the_control_surface_shows() {
    let verbs = [
        Intervention::Steer,
        Intervention::Cancel,
        Intervention::Takeover,
        Intervention::Rollback,
        Intervention::Halt,
        Intervention::Release,
    ];
    // Five verbs plus Release, the return path for Halt.
    assert_eq!(verbs.len(), 6);
    let mut names: Vec<&str> = verbs.iter().map(|v| v.as_str()).collect();
    names.sort_unstable();
    names.dedup();
    assert_eq!(names.len(), 6, "verb names are distinct");
}

#[test]
fn dispatching_work_is_not_an_intervention() {
    let command = Command::BatchByBuilding {
        addr: Address::parse("acme/floor1").unwrap(),
        idem: key(),
    };
    assert!(matches!(
        classify(&command),
        ControlVerdict::NotAnIntervention
    ));
}

#[test]
fn steering_with_nothing_to_say_is_refused_rather_than_recorded() {
    // An empty Steer would append an empty block to the next result
    // envelope: a real event that teaches the model nothing.
    let command = Command::Steer {
        run: run(),
        text: "   ".to_owned(),
        idem: key(),
    };
    let ControlVerdict::Refuse(err) = classify(&command) else {
        panic!("an empty Steer is not a valid intervention");
    };
    assert!(!err.recovery().is_empty());
}

// ---------------------------------------------------------------------- auth

#[test]
fn a_minted_token_verifies_against_its_own_digest_and_nothing_else() {
    let (token, shown) = PairingToken::mint([0x11u8; 32]);
    let digest = token.digest();

    assert!(verify(Some(&shown), &digest), "the code shown must work");
    assert!(!verify(Some("not-the-code"), &digest));
    assert!(
        !verify(None, &digest),
        "a missing token is not an empty token"
    );
}

#[test]
fn minting_is_a_function_of_its_entropy_not_of_the_clock() {
    // Determinism rule 4: seeded RNG is handed down from assembly; this
    // module samples nothing, so the same entropy mints the same token.
    let (left, left_code) = PairingToken::mint([0x22u8; 32]);
    let (right, right_code) = PairingToken::mint([0x22u8; 32]);
    assert_eq!(left.digest(), right.digest());
    assert_eq!(left_code, right_code);
    let (other, _) = PairingToken::mint([0x23u8; 32]);
    assert_ne!(left.digest(), other.digest());
}

#[test]
fn a_configured_token_must_be_long_enough_to_be_worth_checking() {
    assert!(PairingToken::from_configured("short").is_err());
    assert!(PairingToken::from_configured("").is_err());
    let (minted, code) = PairingToken::mint([7u8; 32]);
    let adopted = PairingToken::from_configured(&code).expect("a minted code is long enough");
    assert_eq!(
        adopted.digest(),
        minted.digest(),
        "adopting the code we just showed must reach the same token"
    );
}

#[test]
fn the_display_form_is_readable_by_a_person_reading_it_aloud() {
    let (_token, shown) = PairingToken::mint([0x33u8; 32]);
    assert!(
        shown.chars().all(|c| c.is_ascii_alphanumeric() || c == '-'),
        "a pairing code is typed by hand: {shown}"
    );
    assert!(shown.contains('-'), "grouped so it can be read aloud");
}
