// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Every command frame this client sends, built in one place.
//!
//! **Why they are together.** A frame constructor is where a control
//! stops being a picture and becomes a verb on the wire, and
//! `xtask wiring` judges exactly that crossing: a verb the city can carry
//! out must be reachable from here, and one it cannot must not be. With
//! the constructors scattered through the views, "does anything send
//! this" was a question you answered by grepping and hoping.
//!
//! Each takes what the control knows and nothing else. The idempotency
//! key is derived from a fixed phrase per verb rather than from a clock,
//! because a client has no clock this city trusts - the one sampling
//! point is `bin::assembly` (ARCHITECTURE.md section 10).

use channels::{Address, ClientFrame, WireCommand};

fn city_key(phrase: &'static [u8]) -> channels::IdemKey {
    channels::IdemKey::derive(&channels::RunId::CITY, channels::Seq::FIRST, phrase)
}

/// Stop the whole city, or let it go again.
///
/// One function for both, because they are one control: the button reads
/// what the city is doing and offers the other thing.
#[must_use]
pub fn halt(halting: bool) -> ClientFrame {
    let scope = channels::HaltScope::City;
    let idem = city_key(b"halt-from-the-control-surface");
    ClientFrame::Command(Box::new(if halting {
        WireCommand::Halt { scope, idem }
    } else {
        WireCommand::Release { scope, idem }
    }))
}

/// Set, pause, resume or clear what a building is working towards.
#[must_use]
pub fn pursue(addr: &Address, step: channels::PursuitStep) -> ClientFrame {
    ClientFrame::Command(Box::new(WireCommand::Pursue {
        addr: addr.clone(),
        step,
        idem: city_key(b"pursue-from-the-building-page"),
    }))
}

/// Say who answers the approvals raised in a scope.
#[must_use]
pub fn set_autonomy(scope: &channels::HaltScope, autonomy: channels::Autonomy) -> ClientFrame {
    ClientFrame::Command(Box::new(WireCommand::SetAutonomy {
        scope: scope.clone(),
        autonomy,
        idem: city_key(b"autonomy-from-the-building-page"),
    }))
}

/// The standing goals the city answer carries, or none when this page
/// never asked the city.
#[must_use]
pub fn pursuits_of(city: Option<&channels::CityAnswer>) -> Vec<channels::PursuitLine> {
    city.map(|held| held.pursuits.clone()).unwrap_or_default()
}
