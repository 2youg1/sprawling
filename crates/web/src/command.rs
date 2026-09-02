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

use channels::{Address, ClientFrame, EndpointsAnswer, IdemKey, LoginStep};
use channels::{ProviderName, RunId, Seq, WireCommand};

use crate::settings::{
    AttachForm, AttachReadiness, SelectForm, SelectReadiness, ready, select_ready,
};

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

// The four the settings page sends. They moved here from that page
// for the reason this module exists: a frame constructor is the seam
// `xtask wiring` judges, and a seam spread over two files is a seam
// you check by grepping.

/// The command a filled form asks for, or `None` while it is not ready.
///
/// Returning `None` rather than a half-built command is what keeps
/// [`ready`] the only statement of what a complete form is.
/// One login step, ready to send.
///
/// Pure and separate from the button so both steps are decided in one
/// place: the key is derived from the step itself, so pressing "start"
/// twice begins one login while "finish" carries a key of its own.
#[must_use]
pub fn login_command(provider: &str, step: LoginStep) -> Option<WireCommand> {
    let provider = ProviderName::parse(provider.trim()).ok()?;
    let material = match &step {
        LoginStep::Begin => format!("login-begin:{}", provider.as_str()),
        LoginStep::Code { code } => format!("login-code:{}:{code}", provider.as_str()),
    };
    Some(WireCommand::Login {
        provider,
        step,
        idem: IdemKey::derive(&RunId::CITY, Seq::FIRST, material.as_bytes()),
    })
}

#[must_use]
pub fn attach_command(form: &AttachForm) -> Option<WireCommand> {
    if ready(form) != AttachReadiness::Ready {
        return None;
    }
    let name = ProviderName::parse(form.name.trim()).ok()?;
    let base_url = form.base_url.trim().to_owned();
    Some(WireCommand::AttachEndpoint {
        name,
        // The idempotency key is derived from what the person entered,
        // so pressing the button twice attaches once.
        idem: IdemKey::derive(&RunId::CITY, Seq::FIRST, base_url.as_bytes()),
        base_url,
        dialect: form.dialect?,
        secret: form.secret.clone(),
        auth_header: None,
        admit: form.admit.clone(),
    })
}

/// The command that asks a base URL what it serves, without attaching
/// anything.
///
/// It needs the same filled form an attachment does, because a probe
/// that guessed the dialect would be asking a different question from
/// the one the attachment will ask.
#[must_use]
pub fn probe_command(form: &AttachForm) -> Option<WireCommand> {
    if ready(form) != AttachReadiness::Ready {
        return None;
    }
    let name = ProviderName::parse(form.name.trim()).ok()?;
    let base_url = form.base_url.trim().to_owned();
    Some(WireCommand::ProbeEndpoint {
        name,
        // Distinct from the attach key on the same URL: asking and
        // registering are two acts, and one must not deduplicate the
        // other away.
        idem: IdemKey::derive(
            &RunId::CITY,
            Seq::FIRST,
            format!("probe:{base_url}").as_bytes(),
        ),
        base_url,
        dialect: form.dialect?,
        secret: form.secret.clone(),
        auth_header: None,
    })
}

/// The command a filled model form asks for, or `None` while it is not
/// ready.
#[must_use]
pub fn select_command(form: &SelectForm, answer: &EndpointsAnswer) -> Option<WireCommand> {
    if select_ready(form, answer) != SelectReadiness::Ready {
        return None;
    }
    let endpoint = ProviderName::parse(form.endpoint.trim()).ok()?;
    let model = form.model.trim().to_owned();
    Some(WireCommand::SelectModel {
        idem: IdemKey::derive(
            &RunId::CITY,
            Seq::FIRST,
            format!("{}/{model}", form.endpoint).as_bytes(),
        ),
        endpoint,
        model,
        tag: form.tag?,
        // The model's own facts, not numbers invented on this page. The
        // server holds the catalogue; zero means "take what it says".
        context_tokens: 0,
        max_output_tokens: 0,
    })
}
