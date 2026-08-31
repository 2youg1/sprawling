// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! S2.13: the refusal conformance matrix. Every Deny the doors can
//! produce carries the three mandatory parts, each non-empty, and the
//! alternative is directive prose (names a next action), because the
//! model is the recovery subject.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

use kernel::{
    Address, ApprovalId, AxError, DelegateKind, Depth, DiscardRequest, EgressOutcome, EgressTarget,
    GateContext, GateOutcome, SecretSpan, TaintSet, TimeMs, WriteDomain,
};

fn ctx() -> GateContext {
    GateContext {
        actor: "worker@sim.1".into(),
        now: TimeMs::new(1),
        item_id: ApprovalId::new("item-1").unwrap(),
    }
}

fn artifact() -> kernel::Locator {
    kernel::Locator::parse(&format!("cas:b3-{}", "aa".repeat(32))).unwrap()
}

/// The matrix row: three parts present, non-empty, alternative directive.
fn assert_complete_refusal(refusal: &AxError, door: &str) {
    let gate = refusal
        .gate()
        .unwrap_or_else(|| panic!("{door}: refusal must carry the three parts"));
    assert!(!gate.rule().is_empty(), "{door}: empty rule");
    assert!(!gate.violation().is_empty(), "{door}: empty violation");
    assert!(
        gate.alternative().len() > 12,
        "{door}: the alternative must direct the next action, not just say no"
    );
}

#[test]
fn every_door_denial_carries_a_complete_teaching_refusal() {
    // Domain door.
    let wd = WriteDomain::new(vec![Address::parse("b1").unwrap()]).unwrap();
    let GateOutcome::Deny { refusal } =
        kernel::domain(&wd, &Address::parse("b2/x.md").unwrap(), &TaintSet::empty())
    else {
        panic!("outside write must deny")
    };
    assert_complete_refusal(&refusal, "domain");

    // Egress door.
    let span = SecretSpan {
        start: 0,
        len: 40,
        provider: Some("anthropic"),
    };
    let EgressOutcome::Deny { refusal } = kernel::egress(
        std::slice::from_ref(&span),
        &EgressTarget::Public {
            host: "x.io".into(),
        },
        false,
    ) else {
        panic!("secret spans must deny")
    };
    assert_complete_refusal(&refusal, "egress");

    // Discard door (unplanned request).
    let unplanned = DiscardRequest::Unplanned {
        paths: vec![Address::parse("b/x.md").unwrap()],
        taint: TaintSet::empty(),
        total_bytes: kernel::ByteLen::new(1),
    };
    let registry = kernel::Registry::new();
    let GateOutcome::Deny { refusal } =
        kernel::gate_discard(&unplanned, &registry, &ctx(), "delete b/x.md", &artifact())
    else {
        panic!("unplanned discard must deny")
    };
    assert_complete_refusal(&refusal, "discard");

    // Spawn admission.
    let GateOutcome::Deny { refusal } = kernel::spawn(Depth::Delegated, &DelegateKind::Resident)
    else {
        panic!("delegated spawn must deny")
    };
    assert_complete_refusal(&refusal, "spawn");
}

#[test]
fn gate_code_carriers_all_point_at_gate_denied() {
    // The refusal codes the doors produce carry into history via
    // gate_denied (C9): the matrix cross-checks the carrier table.
    use kernel::{AxCode, Carrier, EventKind};
    for code in [
        AxCode::OutsideWriteDomain,
        AxCode::SecretEgress,
        AxCode::DiscardIrreversible,
        AxCode::DelegationDepth,
        AxCode::GateDenied,
        AxCode::TaintedAction,
        AxCode::CrossBuildingDenied,
    ] {
        assert_eq!(code.carrier(), Carrier::Event(EventKind::GateDenied));
    }
}
