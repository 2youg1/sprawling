// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! What a building is working towards on its own, and who answers for it.
//!
//! **Two verbs that had no control at all.** `Command::Pursue` and
//! `Command::SetAutonomy` were on the wire, matched by the worker, and
//! covered by tests, and no button in this client sent either — which
//! `xtask wiring` now refuses. `Pursue` is the whole of "the city keeps
//! working until the work runs out", so with no control the feature
//! existed everywhere except where a person could reach it.
//!
//! **The state is read, never held.** `CityAnswer.pursuits` already
//! carries the goal, its state and the city's own verdict on whether
//! anything is left to do. This module renders that and sends steps; it
//! works out neither the stop condition nor the next node, because both
//! already have an authority (`kernel::pursuit`) and a second reading
//! here would be a second answer to "is this finished".
//!
//! **The four steps are one command, and that is deliberate**
//! (`channels::PursuitStep`): a person who can set a goal can pause it,
//! and a client that offered one without the other would be a client
//! that could start something nobody could stop.

use channels::{Address, PursuitLine, PursuitState};
use dioxus::prelude::*;

use crate::lang::{Lang, Msg, say};

/// The pursuit this building has, picked out of the city's list.
///
/// Returned rather than filtered in place so the caller states which
/// building it is drawing; a component that searched for "the first
/// pursuit" would draw another building's goal on a quiet page.
#[must_use]
pub fn standing<'a>(pursuits: &'a [PursuitLine], addr: &Address) -> Option<&'a PursuitLine> {
    pursuits.iter().find(|line| &line.addr == addr)
}

/// The step a button sends, given what the building is doing now.
///
/// Exhaustive over the state rather than a pair of booleans: a paused
/// pursuit resumes and a running one pauses, and there is no third
/// reading of the same button.
#[must_use]
pub fn toggle(state: PursuitState) -> (Msg, channels::PursuitStep) {
    match state {
        PursuitState::Running => (Msg::PursuitPause, channels::PursuitStep::Pause),
        PursuitState::Paused => (Msg::PursuitResume, channels::PursuitStep::Resume),
    }
}

/// The standing goal panel: what it is, what the city says about it, and
/// the two things a person may do to it.
#[component]
pub fn PursuitView(
    addr: Address,
    pursuits: Vec<PursuitLine>,
    on_frame: EventHandler<channels::ClientFrame>,
) -> Element {
    let lang = use_context::<Signal<Lang>>();
    let word = move |msg: Msg| say(lang(), msg);
    let mut goal = use_signal(String::new);
    let standing = standing(&pursuits, &addr).cloned();

    rsx! {
        crate::panel::Panel {
            title: word(Msg::PursuitTitle).to_owned(),
            scope: Some(word(Msg::PursuitScope).to_owned()),
            source: word(Msg::PursuitSource).to_owned(),
            match standing {
                None => rsx! {
                    crate::panel::Empty {
                        status: word(Msg::PursuitEmpty).to_owned(),
                        what: word(Msg::PursuitEmptyWhat).to_owned(),
                    }
                    form {
                        class: "pursuit-form",
                        onsubmit: {
                            let addr = addr.clone();
                            move |event: FormEvent| {
                                event.prevent_default();
                                let wanted = goal().trim().to_owned();
                                if wanted.is_empty() {
                                    return;
                                }
                                on_frame
                                    .call(crate::command::pursue(&addr, channels::PursuitStep::Set { goal: wanted }));
                                goal.set(String::new());
                            }
                        },
                        label { r#for: "pursuit-goal", "{word(Msg::PursuitGoalLabel)}" }
                        input {
                            id: "pursuit-goal",
                            class: "pursuit-goal",
                            value: "{goal}",
                            oninput: move |event| goal.set(event.value()),
                        }
                        button { r#type: "submit", class: "pursue", "{word(Msg::PursuitSet)}" }
                    }
                },
                Some(line) => {
                    let (label, step) = toggle(line.state);
                    rsx! {
                        p { class: "pursuit-goal-said", "{line.goal}" }
                        p { class: "pursuit-verdict", "{line.verdict}" }
                        div { class: "pursuit-controls",
                            button {
                                class: "pursue",
                                onclick: {
                                    let addr = addr.clone();
                                    move |_| on_frame.call(crate::command::pursue(&addr, step.clone()))
                                },
                                "{word(label)}"
                            }
                            button {
                                class: "pursue-clear",
                                onclick: {
                                    let addr = addr.clone();
                                    move |_| {
                                        on_frame.call(crate::command::pursue(&addr, channels::PursuitStep::Clear))
                                    }
                                },
                                "{word(Msg::PursuitClear)}"
                            }
                        }
                    }
                }
            }
        }
    }
}

/// Who answers the approvals this building raises.
///
/// Two rungs and not three: `Autonomy::Delegate` names a resident, and a
/// control that could name one would need a list of who lives here that
/// this page does not have. Offering it as an empty box would let a
/// person send a name the city will refuse.
#[component]
pub fn AutonomyView(
    scope: channels::HaltScope,
    on_frame: EventHandler<channels::ClientFrame>,
) -> Element {
    let lang = use_context::<Signal<Lang>>();
    let word = move |msg: Msg| say(lang(), msg);
    let owner = scope.clone();

    rsx! {
        crate::panel::Panel {
            title: word(Msg::AutonomyTitle).to_owned(),
            scope: Some(word(Msg::AutonomyScope).to_owned()),
            source: word(Msg::AutonomySource).to_owned(),
            div { class: "autonomy-controls",
                button {
                    class: "admit",
                    onclick: move |_| {
                        on_frame.call(crate::command::set_autonomy(&owner, channels::Autonomy::Owner))
                    },
                    "{word(Msg::AutonomyOwner)}"
                }
                button {
                    class: "admit",
                    onclick: move |_| {
                        on_frame.call(crate::command::set_autonomy(&scope, channels::Autonomy::Deferred))
                    },
                    "{word(Msg::AutonomyDeferred)}"
                }
            }
        }
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, reason = "test code")]
mod tests {
    use super::*;

    fn line(addr: &str, state: PursuitState) -> PursuitLine {
        PursuitLine {
            addr: Address::parse(addr).unwrap(),
            goal: "empty the tree".to_owned(),
            state,
            verdict: "working on 2.3".to_owned(),
        }
    }

    /// A page draws its own building's goal, never the first one in the
    /// list. With one pursuit running in another building, a search that
    /// took the head would put that goal on this page.
    #[test]
    fn a_building_reads_its_own_goal_and_not_the_first_one() {
        let all = [
            line("lab", PursuitState::Running),
            line("shop", PursuitState::Paused),
        ];
        let shop = Address::parse("shop").unwrap();
        assert_eq!(
            standing(&all, &shop).map(|found| found.state),
            Some(PursuitState::Paused)
        );
        let quiet = Address::parse("attic").unwrap();
        assert!(standing(&all, &quiet).is_none());
    }

    /// The one button reads the state it is standing on, so a running
    /// pursuit can always be stopped and a paused one can always be
    /// restarted.
    #[test]
    fn the_button_offers_the_step_the_state_does_not_already_have() {
        assert!(matches!(
            toggle(PursuitState::Running),
            (Msg::PursuitPause, channels::PursuitStep::Pause)
        ));
        assert!(matches!(
            toggle(PursuitState::Paused),
            (Msg::PursuitResume, channels::PursuitStep::Resume)
        ));
    }
}
