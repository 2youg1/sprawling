// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! A trace the adversary found, kept here so this repository remembers it.
//!
//! Written by `adversary/src/Sprawling/Regression.hs` and compared against it
//! byte for byte. Change the trace there; changing it here turns the adversary
//! red, which is exactly what should happen when the two disagree.
//!
//! The adversary drives the shipped binary over the wire. This enters by the
//! same door `channels::server` does, so the trace runs without a port.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

use kernel::{Address, AxCode, IdemKey, RunId, Seq};
use sprawling::assembly;

#[test]
fn a_halted_city_names_the_halt_and_not_the_configuration() {
    let dir = tempfile::tempdir().unwrap();
    assembly::init_city(dir.path()).unwrap();

    // The vault is the in-session one: a test that reached the platform
    // credential service would write to the machine running it.
    let mut worker = assembly::RunWorker::new(
        dir.path(),
        gateway::Custodian::in_memory(),
        runtime::diagnostics::Diagnostics::off(),
    )
    .unwrap();

    worker
        .handle(channels::Command::CreateBuilding {
            addr: Address::parse("acme").unwrap(),
            template: channels::TemplateName::parse("minimal").unwrap(),
            idem: IdemKey::derive(&RunId::CITY, Seq::FIRST, b"step1"),
        })
        .unwrap();

    let refused2 = worker
        .handle(channels::Command::Dispatch {
            addr: Address::parse("acme").unwrap(),
            task: "say something".to_owned(),
            goal: "an answer".to_owned(),
            mode: channels::ModeTag::parse("build").unwrap(),
            budget: kernel::BudgetCap {
                usd: kernel::UsdMicros::new(1_000),
                tokens: kernel::Tokens::new(1_000),
            },
            idem: IdemKey::derive(&RunId::CITY, Seq::FIRST, b"step2"),
            session: Some(kernel::SessionName::parse("one").unwrap()),
            effort: None,
        })
        .unwrap_err();
    assert_eq!(*refused2.code(), AxCode::ConfigInvalid);
    // A refusal is a promise in three parts; a code with no way forward
    // keeps only one of them.
    assert!(!refused2.recovery().is_empty());

    worker
        .handle(channels::Command::Halt {
            scope: channels::HaltScope::City,
            idem: IdemKey::derive(&RunId::CITY, Seq::FIRST, b"step3"),
        })
        .unwrap();

    let refused4 = worker
        .handle(channels::Command::Dispatch {
            addr: Address::parse("acme").unwrap(),
            task: "say something".to_owned(),
            goal: "an answer".to_owned(),
            mode: channels::ModeTag::parse("build").unwrap(),
            budget: kernel::BudgetCap {
                usd: kernel::UsdMicros::new(1_000),
                tokens: kernel::Tokens::new(1_000),
            },
            idem: IdemKey::derive(&RunId::CITY, Seq::FIRST, b"step4"),
            session: Some(kernel::SessionName::parse("two").unwrap()),
            effort: None,
        })
        .unwrap_err();
    assert_eq!(*refused4.code(), AxCode::GateDenied);
    // A refusal is a promise in three parts; a code with no way forward
    // keeps only one of them.
    assert!(!refused4.recovery().is_empty());
}
