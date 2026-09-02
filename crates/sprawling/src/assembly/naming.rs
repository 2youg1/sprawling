// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The wire's words and the kernel's, translated one way each.

use kernel::EventRecord;
use kernel::{Address, AxCode, AxError};

/// What this agent is called: the last segment of its address, which is
/// the word a person typed into `call it` when they started the session
/// (F2.11). Never the whole address — an agent addressed as its own
/// name reads more like somebody than like a path.
pub(super) fn name_of(addr: &Address) -> &str {
    addr.as_str().rsplit('/').next().unwrap_or(addr.as_str())
}

/// The mode a wire tag names. An unknown tag is the planning mode: a
/// mode nobody implemented must not silently become a stricter or a
/// looser one, and planning is the mode that demands nothing.
pub(super) fn mode_of(tag: &channels::ModeTag) -> runtime::Mode {
    match tag.as_str() {
        "up" => runtime::Mode::Up,
        "sc" => runtime::Mode::Sc,
        "ud" => runtime::Mode::Ud,
        "experiment" => runtime::Mode::Experiment,
        _ => runtime::Mode::PlanGoal,
    }
}

/// The answer to a verb this build spells on the wire and cannot perform.
///
/// One authority for the shape, because the six of them differ only in
/// which action failed, what it named, and what a person can do instead.
/// `WireMismatch` rather than `InvalidArgs`: the frame is well formed and
/// its arguments are sound, and what is absent is the executor.
///
/// A verb answered here must not appear as a control in the client. A
/// refusal is what the city owes a peer that asks anyway; it is not a
/// substitute for the button being gone.
pub(super) fn not_built(action: &'static str, subject: String, instead: &'static str) -> AxError {
    AxError::failure(AxCode::WireMismatch, action, subject).with_recovery(instead)
}

/// The building an address belongs to: its first segment.
pub(super) fn building_of(addr: &Address) -> Option<Address> {
    Address::parse(addr.as_str().split('/').next()?).ok()
}

/// Which plan node a `roadmap_*` record names.
pub(super) fn plan_node_of(record: &EventRecord) -> Option<kernel::NodeId> {
    kernel::NodeId::parse(record.data().as_map().get("node")?.as_str()?).ok()
}

/// Rebuilds the endpoint book from the ledger. Same disposability as the
/// views: nothing about what is attached is stored anywhere else.
///
/// # Errors
/// Propagates chain verification and payload failures.
/// How a scope is written into the ledger. Three shapes, one spelling
/// each; the address rides along because "this building" and "that one"
/// are different scopes.
pub(super) fn scope_name(scope: &channels::HaltScope) -> String {
    match scope {
        channels::HaltScope::City => "city".to_owned(),
        channels::HaltScope::Building(addr) => format!("building:{}", addr.as_str()),
        channels::HaltScope::Workshop(addr) => format!("workshop:{}", addr.as_str()),
    }
}

/// The written form of an autonomy setting, and its reader. One writer
/// and one reader for one spelling: a delegate's address is part of the
/// value, so a replay knows which resident was appointed.
pub(super) fn autonomy_name(autonomy: &kernel::Autonomy) -> String {
    match autonomy {
        kernel::Autonomy::Owner => "owner".to_owned(),
        kernel::Autonomy::Delegate(resident) => format!("delegate:{}", resident.as_str()),
        kernel::Autonomy::Deferred => "deferred".to_owned(),
        // A setting this version cannot spell is recorded as the strict
        // side rather than as a guess: an unreadable autonomy must not
        // read back as a wider one.
        _ => "owner".to_owned(),
    }
}

pub(super) fn read_autonomy(name: &str) -> kernel::Autonomy {
    match name.split_once(':') {
        Some(("delegate", resident)) => match kernel::ResidentId::new(resident) {
            Some(resident) => kernel::Autonomy::Delegate(resident),
            // An unreadable delegate falls back to the person rather than
            // to nobody: the safe side of this setting is the strict one.
            None => kernel::Autonomy::Owner,
        },
        _ => match name {
            "deferred" => kernel::Autonomy::Deferred,
            _ => kernel::Autonomy::Owner,
        },
    }
}
