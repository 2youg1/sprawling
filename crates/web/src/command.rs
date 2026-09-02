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

use crate::lang::Msg;
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
pub fn dispatch_to_building(building: &str, task: &str, goal: &str) -> Option<ClientFrame> {
    // Work happens in a room, not at a building's root: living there
    // would hand a run the whole building's write domain. The room used
    // to be `room1` for every dispatch this page sent, so two pieces of
    // work started from the same tower wrote over each other's files.
    // The city opens a room from the name instead, and the name comes
    // from the work rather than from a counter (city-SPEC.md 8-13).
    // What a Dispatch frame looks like is `dispatch_command`'s answer and
    // only its answer - the city page decides the address.
    dispatch_command(
        Sending {
            room: &format!("{}/{}", building.trim(), session_name(task)),
            mode: "plan",
            // This page asks for two lines and a building; how hard to
            // think is chosen where the whole form is, at the bottom of
            // the window.
            effort: None,
        },
        task,
        goal,
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

/// The effort a control's value names. An empty value is no choice,
/// which is not the same as `Effort::None` - that one asks a provider
/// for no reasoning at all.
#[must_use]
pub fn effort_named(value: &str) -> Option<channels::Effort> {
    match value {
        "low" => Some(channels::Effort::Low),
        "medium" => Some(channels::Effort::Medium),
        "high" => Some(channels::Effort::High),
        "xhigh" => Some(channels::Effort::XHigh),
        "max" => Some(channels::Effort::Max),
        _ => None,
    }
}

/// The efforts this bar offers, in the order it offers them: what the
/// city already resolves, plus the one that leaves the answer to the
/// layer above.
pub(crate) const EFFORTS: [(&str, Msg); 6] = [
    ("", Msg::EffortInherited),
    ("low", Msg::EffortLow),
    ("medium", Msg::EffortMedium),
    ("high", Msg::EffortHigh),
    ("xhigh", Msg::EffortXHigh),
    ("max", Msg::EffortMax),
];

/// The modes a person may pick between, in the order the control offers
/// them.
///
/// `runtime::Mode` is the authority for the set and this is the
/// authority for its spelling on the wire: `ModeTag::parse` accepts any
/// string and `mode_of` reads an unknown one as planning, so a typo here
/// would silently change what a run is allowed to do.
pub const MODES: [&str; 5] = ["build", "up", "sc", "ud", "experiment"];

/// Builds one Dispatch. The only place in the client that does.
///
/// No budget travels from a person: `BudgetCap::default()` is what the
/// wire carries, and what a run costs is reported after it runs. This
/// city has no budget lock, so the composer neither asks for a figure
/// nor shows one.
///
/// **`room` is split, not sent whole.** `lab/parser` means the building
/// `lab` and a session a person is calling `parser`, which is exactly
/// what the wire's two fields say: given a session name the city opens a
/// room of that name under the building, and two dispatches naming one
/// room are one session continued. A bare `lab` sends no session name at
/// all, which is the city's cue to work one out from the task.
///
/// **The goal is left empty, and that is a meaning rather than a gap.**
/// A dispatch with no goal is already how this city spells "a person is
/// at the other end": no job file is written and the frozen prefix says
/// so. That is exactly what one sentence typed into the composer is, so
/// the box sends no goal and the city reads it as it always did. A
/// client that copied the task into the goal field would turn every
/// conversation into a task nobody asked for.
/// How a dispatch is to be run: where, in which mode, and how hard to
/// think.
///
/// Three values that always travel together and are never chosen
/// independently, so they travel as one. The composer holds them as one
/// (`sessions::Plan`), `Plan::guessed` fills all three at once, and the
/// city page fixes all three at once.
#[derive(Debug, Clone, Copy)]
pub struct Sending<'a> {
    /// `lab` or `lab/parser`: the building, and optionally the session
    /// name a person is calling this piece of work.
    pub room: &'a str,
    pub mode: &'a str,
    pub effort: Option<channels::Effort>,
}

#[must_use]
pub fn dispatch_command(
    sending: Sending<'_>,
    task: &str,
    goal: &str,
) -> Option<channels::ClientFrame> {
    let Sending { room, mode, effort } = sending;
    let task = task.trim();
    if task.is_empty() {
        return None;
    }
    let (building, session) = match room.trim().split_once('/') {
        Some((building, named)) => (building, Some(channels::SessionName::parse(named).ok()?)),
        None => (room.trim(), None),
    };
    let addr = Address::parse(building).ok()?;
    Some(channels::ClientFrame::Command(Box::new(
        channels::WireCommand::Dispatch {
            idem: channels::IdemKey::derive(
                &RunId::CITY,
                Seq::FIRST,
                format!("{}|{task}", addr.as_str()).as_bytes(),
            ),
            addr,
            task: task.to_owned(),
            goal: goal.trim().to_owned(),
            mode: channels::ModeTag::parse(mode).ok()?,
            budget: channels::BudgetCap::default(),
            session,
            effort,
        },
    )))
}

/// How hard this city thinks when nobody has said otherwise.
///
/// The city's own ladder resolves effort per room, and this is only what
/// the composer offers before a person opens that word - so a wrong
/// guess here costs a click rather than a decision.
pub const DEFAULT_EFFORT: &str = "medium";

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
        assert!(dispatch_to_building("lab", "fix the timer", "the test passes").is_some());
        assert!(
            dispatch_to_building("lab", "  ", "the test passes").is_none(),
            "a run with nothing to do is not a command"
        );
        assert!(
            dispatch_to_building("lab", "fix the timer", "").is_some(),
            "an empty goal is how this city spells a conversation, not a missing field"
        );
        assert!(
            dispatch_to_building("", "fix the timer", "the test passes").is_none(),
            "there is no building called nothing"
        );
    }

    #[test]
    fn work_is_sent_to_a_room_and_never_to_a_buildings_root() {
        let Some(ClientFrame::Command(command)) =
            dispatch_to_building("lab", "fix the timer", "the test passes")
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

        let Some(ClientFrame::Command(second)) = dispatch_to_building(
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
