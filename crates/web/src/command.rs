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

/// What a person typed into the building form, and whether it is a
/// command yet.
///
/// A building is a top-level address, so an address with a slash in it
/// is refused here as well as at the server — shown before the person
/// presses anything rather than after.
#[must_use]
pub fn create_command(addr: &str, template: &str) -> Option<ClientFrame> {
    let addr = Address::parse(addr.trim()).ok()?;
    if addr.as_str().contains('/') {
        return None;
    }
    let template = channels::TemplateName::parse(template.trim()).ok()?;
    Some(ClientFrame::Command(Box::new(
        channels::WireCommand::CreateBuilding {
            idem: channels::IdemKey::derive(
                &channels::RunId::CITY,
                channels::Seq::FIRST,
                addr.as_str().as_bytes(),
            ),
            addr,
            template,
        },
    )))
}

/// What a person typed into the selected building's form, and whether it
/// is a dispatch yet.
///
/// A run needs somewhere to work and something that counts as done, so
/// both are required here rather than defaulted: a dispatch with an
/// invented goal is a run that cannot report it finished.
#[must_use]
pub fn dispatch_command(building: &str, task: &str, goal: &str) -> Option<ClientFrame> {
    // Work happens in a room, not at a building's root: living there
    // would hand a run the whole building's write domain. The room used
    // to be `room1` for every dispatch this page sent, so two pieces of
    // work started from the same tower wrote over each other's files.
    // The city opens a room from the name instead, and the name comes
    // from the work rather than from a counter (city-SPEC.md 8-13).
    // What a Dispatch frame looks like is `app::dispatch_command`'s
    // answer and only its answer - this page decides the address.
    crate::app::dispatch_command(
        &format!("{}/{}", building.trim(), session_name(task)),
        task,
        goal,
        "plan",
        // This page asks for two lines and a building; how hard to think
        // is chosen where the whole form is, at the bottom of the window.
        None,
    )
}

/// The name this page gives a session it starts: the first few words of
/// the task, which is what the person just wrote and will recognise in a
/// list of folders an hour later.
fn session_name(task: &str) -> String {
    let head: Vec<&str> = task.split_whitespace().take(4).collect();
    let joined = head.join(" ");
    // A name is one segment; anything the segment rules refuse is left
    // to `SessionName::parse`, which refuses the whole command rather
    // than inventing a spelling nobody typed.
    joined.replace(['/', '\\', ':'], "-")
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

    #[test]
    fn sending_work_needs_a_room_and_a_task_and_nothing_else() {
        assert!(dispatch_command("lab", "fix the timer", "the test passes").is_some());
        assert!(
            dispatch_command("lab", "  ", "the test passes").is_none(),
            "a run with nothing to do is not a command"
        );
        assert!(
            dispatch_command("lab", "fix the timer", "").is_some(),
            "an empty goal is how this city spells a conversation, not a missing field"
        );
        assert!(
            dispatch_command("", "fix the timer", "the test passes").is_none(),
            "there is no building called nothing"
        );
    }

    #[test]
    fn work_is_sent_to_a_room_and_never_to_a_buildings_root() {
        let Some(ClientFrame::Command(command)) =
            dispatch_command("lab", "fix the timer", "the test passes")
        else {
            panic!("a complete form is a command");
        };
        let channels::WireCommand::Dispatch { addr, session, .. } = *command else {
            panic!("the send-work form makes a dispatch");
        };
        assert_eq!(addr.as_str(), "lab");
        // The room is opened by the city from this name, and the name is
        // the work rather than a counter. Every dispatch from this page
        // used to go to `room1`, so the second piece of work started
        // from a tower wrote over the first one's files.
        let named = session.expect(
            "without a name the city has nothing to open a room from, and the run would hold the \
             whole building's write domain",
        );
        assert_eq!(named.as_str(), "fix the timer");

        let Some(ClientFrame::Command(second)) = dispatch_command(
            "lab",
            "fix the timer again, and this time read the failing case first",
            "the test passes",
        ) else {
            panic!("a complete form is a command");
        };
        let channels::WireCommand::Dispatch { session, .. } = *second else {
            panic!("the send-work form makes a dispatch");
        };
        assert_eq!(
            session.map(|name| name.as_str().to_owned()),
            Some("fix the timer again,".to_owned()),
            "a long task still yields a name short enough to be a folder"
        );
    }
}
