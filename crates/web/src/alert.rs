// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The only module allowed to produce ALERT, and the only one allowed to
//! raise a browser notification.
//!
//! One gate for both, because they are the same decision: *this needs a
//! person*. Scattering that judgement is how an interface ends up with a
//! warm colour on things nobody has to act on, at which point the colour
//! stops meaning anything and the one real alert is lost among them.
//!
//! Three refusals, all from the anti-attention rules of the layout:
//! no unread counts, no dots on things that need no action, and no
//! re-notifying something already raised. A notification is a claim on
//! somebody's attention, and a claim made twice for one fact is a lie about
//! how much is happening.
//!
//! **What is on screen is not this module's to say.** The left nav already
//! states how many things wait for a person, and it is the only place that
//! does; a second mark in the top bar would be the same fact rendered
//! twice, which is how an interface teaches people that its marks mean
//! nothing. What this module owns is the channel nothing else covers: the
//! interruption that reaches somebody who is not looking at the tab, and
//! the memory that keeps one fact from making two of them - including
//! across a reconnect, where the stream re-delivers what it already sent.

use std::collections::BTreeSet;

use channels::{AxError, EventKind, EventRecord};

/// What the city said back to the person who asked for something.
///
/// Deliberately **not** an [`Alert`]: an `Alert` is a standing fact and
/// is raised once however often it is seen, while a refusal answers one
/// action. Pressing the same button twice with the same wrong URL is two
/// questions and deserves two answers, so this never goes through
/// [`Alerts`] and is never deduplicated.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused {
    /// The stable code, which is what a person quotes when they ask.
    pub code: String,
    /// What the city would not do, in its own words.
    pub what: String,
    /// The way out. Never empty on screen: a refusal that leaves a
    /// person with nothing to try is the failure this path exists to
    /// remove, so the absence is stated rather than rendered as a gap.
    pub recovery: String,
}

/// Turns a refusal into the three things a person needs from it.
///
/// Before this existed the client received `ServerFrame::Refusal` and
/// did nothing with it, so a mistyped base URL produced a page that said
/// nothing at all and a line in a log file nobody was reading.
#[must_use]
pub fn refused(error: &AxError) -> Refused {
    let recovery = error.recovery();
    Refused {
        code: error.code().as_str().to_owned(),
        what: format!("cannot {} on {}", error.action(), error.subject()),
        recovery: if recovery.is_empty() {
            "no way out was recorded with this refusal".to_owned()
        } else {
            recovery.to_owned()
        },
    }
}

/// Why a person is needed. Exhaustive: a new reason has to be added here,
/// which is where somebody is forced to ask whether it really requires a
/// human or merely worries the author.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlertKind {
    /// An ApprovalItem is waiting.
    AwaitingApproval,
    /// A Run was frozen: budget, watchdog, or a limit.
    RunFrozen,
    /// A provider is degraded or gone.
    ProviderTrouble,
    /// A gate refused something that cannot proceed without a ruling.
    Refused,
}

impl AlertKind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::AwaitingApproval => "awaiting approval",
            Self::RunFrozen => "run frozen",
            Self::ProviderTrouble => "provider trouble",
            Self::Refused => "refused",
        }
    }

    /// Whether this kind interrupts a person who is not looking at the tab.
    ///
    /// Only two do. A refusal is already visible where the person is
    /// working, and a degraded provider is a condition rather than an
    /// event - interrupting for either teaches people to dismiss
    /// notifications, which costs the two that matter.
    #[must_use]
    pub fn interrupts(self) -> bool {
        matches!(self, Self::AwaitingApproval | Self::RunFrozen)
    }
}

/// One thing needing a person.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Alert {
    pub kind: AlertKind,
    /// Stable across repeats of the same fact. Two alerts with one key are
    /// one fact seen twice.
    pub key: String,
    pub message: String,
}

/// What the interface should do about an alert.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Raise {
    /// Show it in place, and mark the row.
    Mark,
    /// Show it, and interrupt: a browser notification.
    Interrupt,
    /// Already raised; do nothing at all.
    Silent,
}

/// The single alerting authority: remembers what has been raised so nothing
/// is raised twice.
#[derive(Debug, Default)]
pub struct Alerts {
    raised: BTreeSet<String>,
}

impl Alerts {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Decides what to do with one alert.
    ///
    /// The first sighting of a key may interrupt; every later sighting of
    /// the same key is silent, until [`Alerts::clear`] says the fact is
    /// over. This is what keeps a Run that stays frozen for an hour from
    /// notifying once a second.
    pub fn raise(&mut self, alert: &Alert) -> Raise {
        if !self.raised.insert(alert.key.clone()) {
            return Raise::Silent;
        }
        if alert.kind.interrupts() {
            Raise::Interrupt
        } else {
            Raise::Mark
        }
    }

    /// The fact is over: the approval was answered, the Run resumed. The key
    /// may raise again if it comes back, because the second occurrence is a
    /// new fact rather than an echo of the first.
    pub fn clear(&mut self, key: &str) -> bool {
        self.raised.remove(key)
    }
}

/// What one event asks of a person, if it asks anything.
///
/// Exhaustive by omission on purpose: the default is that an event needs
/// nobody. Adding a kind here is where somebody has to argue that it
/// really requires a human rather than merely worrying the author.
#[must_use]
pub fn alert_for(record: &EventRecord) -> Option<Alert> {
    let map = record.data().as_map();
    match record.kind() {
        EventKind::ApprovalRequested => {
            // The payload is the item, so the key is the item's own id -
            // the same identity the answer will carry.
            let id = map.get("id").and_then(serde_json::Value::as_str)?;
            Some(Alert {
                kind: AlertKind::AwaitingApproval,
                key: format!("approval/{id}"),
                message: map
                    .get("action_desc")
                    .and_then(serde_json::Value::as_str)
                    .unwrap_or("something is waiting for you")
                    .to_owned(),
            })
        }
        EventKind::RunFrozen | EventKind::BudgetLimit => Some(Alert {
            kind: AlertKind::RunFrozen,
            key: format!("run/{}", record.run()),
            message: "a run stopped and will not start itself again".to_owned(),
        }),
        EventKind::ProviderDegraded | EventKind::EndpointLost => Some(Alert {
            kind: AlertKind::ProviderTrouble,
            key: PROVIDER_KEY.to_owned(),
            message: "the provider is not answering as it should".to_owned(),
        }),
        _ => None,
    }
}

/// Which fact one event ends, if it ends one.
///
/// A fact that ends may be raised again later, and the second time is a
/// real second time rather than an echo of the first.
#[must_use]
pub fn cleared_by(record: &EventRecord) -> Option<String> {
    match record.kind() {
        EventKind::ApprovalResolved => record
            .data()
            .as_map()
            .get("id")
            .and_then(serde_json::Value::as_str)
            .map(|id| format!("approval/{id}")),
        EventKind::EndpointAttached => Some(PROVIDER_KEY.to_owned()),
        _ => None,
    }
}

/// The key every provider complaint shares: one sick provider is one fact,
/// however many calls notice it.
const PROVIDER_KEY: &str = "provider";

/// Asks the browser, once, whether it may interrupt.
///
/// Fire and forget: the answer arrives later, and until it is granted
/// [`interrupt`] does nothing. A page that blocked on this would be
/// holding up the city for a permission dialog.
#[cfg(target_arch = "wasm32")]
pub fn ask_to_interrupt() {
    if web_sys::Notification::permission() == web_sys::NotificationPermission::Default {
        let _ = web_sys::Notification::request_permission();
    }
}

/// Raises one browser notification.
///
/// Zero judgement: whether to interrupt was decided by [`Alerts::raise`],
/// which is the only thing that knows whether this fact already made a
/// claim on somebody's attention. Nothing here can fail in a way a person
/// could act on - a browser that refuses notifications has said its piece
/// - so nothing is returned.
#[cfg(target_arch = "wasm32")]
pub fn interrupt(alert: &Alert) {
    if web_sys::Notification::permission() != web_sys::NotificationPermission::Granted {
        return;
    }
    let options = web_sys::NotificationOptions::new();
    options.set_body(&alert.message);
    // The tag is the alert key, so a browser that is still showing this
    // fact replaces it rather than stacking a second copy.
    options.set_tag(&alert.key);
    let _ = web_sys::Notification::new_with_options(alert.kind.as_str(), &options);
}

/// Folds one event into the alert state, and says what to do about it.
///
/// Called beside `Snapshot::apply`, from the one place events arrive, so
/// "what needs a person" is decided in the same pass as "what happened"
/// rather than by a second reader of the stream.
pub fn absorb(alerts: &mut Alerts, record: &EventRecord) -> Raise {
    if let Some(key) = cleared_by(record) {
        alerts.clear(&key);
    }
    match alert_for(record) {
        Some(alert) => alerts.raise(&alert),
        None => Raise::Silent,
    }
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

    fn alert(kind: AlertKind, key: &str) -> Alert {
        Alert {
            kind,
            key: key.to_owned(),
            message: format!("{} at {key}", kind.as_str()),
        }
    }

    #[test]
    fn one_fact_interrupts_once_however_often_it_is_seen() {
        let mut alerts = Alerts::new();
        let frozen = alert(AlertKind::RunFrozen, "run/7");
        assert_eq!(alerts.raise(&frozen), Raise::Interrupt);
        for _ in 0..100 {
            assert_eq!(alerts.raise(&frozen), Raise::Silent);
        }
    }

    #[test]
    fn a_fact_that_ends_and_returns_is_a_new_fact() {
        let mut alerts = Alerts::new();
        let waiting = alert(AlertKind::AwaitingApproval, "ap/1");
        assert_eq!(alerts.raise(&waiting), Raise::Interrupt);
        assert!(alerts.clear("ap/1"));
        assert_eq!(
            alerts.raise(&waiting),
            Raise::Interrupt,
            "the second time is a real second time"
        );
    }

    #[test]
    fn only_the_two_kinds_that_need_somebody_now_interrupt() {
        // Teaching people to dismiss notifications costs the two that
        // matter, so a refusal - already visible where they are working -
        // and a degraded provider only mark.
        let mut alerts = Alerts::new();
        assert_eq!(
            alerts.raise(&alert(AlertKind::Refused, "gate/1")),
            Raise::Mark
        );
        assert_eq!(
            alerts.raise(&alert(AlertKind::ProviderTrouble, "prov/1")),
            Raise::Mark
        );
        assert_eq!(
            alerts.raise(&alert(AlertKind::AwaitingApproval, "ap/2")),
            Raise::Interrupt
        );
        assert_eq!(
            alerts.raise(&alert(AlertKind::RunFrozen, "run/2")),
            Raise::Interrupt
        );
    }

    fn recorded(kind: EventKind, data: serde_json::Map<String, serde_json::Value>) -> EventRecord {
        EventRecord::from_draft(
            channels::EventDraft {
                run: channels::RunId::from_bytes([4u8; 16]),
                t: channels::TimeMs::new(1),
                who: "gate".to_owned(),
                addr: None,
                kind,
                data: channels::Payload::new(data).unwrap(),
                ig: false,
            },
            channels::Seq::new(1),
            channels::B3Hash::digest(b"prev"),
        )
    }

    fn with_id(id: &str) -> serde_json::Map<String, serde_json::Value> {
        let mut map = serde_json::Map::new();
        map.insert("id".to_owned(), serde_json::Value::String(id.to_owned()));
        map.insert(
            "action_desc".to_owned(),
            serde_json::Value::String("push to the remote".to_owned()),
        );
        map
    }

    #[test]
    fn an_answered_question_stops_asking_and_a_new_one_asks_again() {
        let mut alerts = Alerts::new();
        let asked = recorded(EventKind::ApprovalRequested, with_id("item-7"));
        assert_eq!(absorb(&mut alerts, &asked), Raise::Interrupt);
        // A reconnect re-delivers what it already sent; one fact, one
        // interruption.
        assert_eq!(absorb(&mut alerts, &asked), Raise::Silent);

        let answered = recorded(EventKind::ApprovalResolved, with_id("item-7"));
        assert_eq!(absorb(&mut alerts, &answered), Raise::Silent);
        assert_eq!(
            absorb(&mut alerts, &asked),
            Raise::Interrupt,
            "asked again after being answered is a new fact"
        );
    }

    #[test]
    fn a_sick_provider_is_one_fact_however_many_calls_notice_it() {
        let mut alerts = Alerts::new();
        let degraded = recorded(EventKind::ProviderDegraded, serde_json::Map::new());
        let lost = recorded(EventKind::EndpointLost, serde_json::Map::new());
        assert_eq!(absorb(&mut alerts, &degraded), Raise::Mark);
        assert_eq!(absorb(&mut alerts, &lost), Raise::Silent);
        assert_eq!(
            absorb(
                &mut alerts,
                &recorded(EventKind::EndpointAttached, serde_json::Map::new())
            ),
            Raise::Silent
        );
        assert_eq!(
            absorb(&mut alerts, &degraded),
            Raise::Mark,
            "a provider that goes bad again is a second fact"
        );
    }

    #[test]
    fn most_of_what_happens_needs_nobody() {
        let mut alerts = Alerts::new();
        for kind in [
            EventKind::ToolCalled,
            EventKind::ToolResult,
            EventKind::ModelReturned,
            EventKind::RunStarted,
            EventKind::SignalEnqueued,
        ] {
            assert_eq!(
                absorb(&mut alerts, &recorded(kind, serde_json::Map::new())),
                Raise::Silent,
                "{kind:?} interrupted somebody"
            );
        }
    }

    #[test]
    fn clearing_something_never_raised_is_not_an_error() {
        let mut alerts = Alerts::new();
        assert!(!alerts.clear("nothing"));
    }

    #[test]
    fn a_refusal_reaches_the_person_with_its_way_out_attached() {
        let told = refused(
            &AxError::failure(
                channels::AxCode::ConfigInvalid,
                "attach an endpoint",
                "modelscope",
            )
            .with_recovery("the base url needs its /v1"),
        );
        assert_eq!(told.code, "E_CONFIG_INVALID");
        assert_eq!(told.what, "cannot attach an endpoint on modelscope");
        assert_eq!(told.recovery, "the base url needs its /v1");
    }

    /// A refusal with an empty recovery says so rather than rendering a
    /// blank line, because a blank line reads as "nothing is wrong".
    #[test]
    fn a_refusal_with_no_way_out_says_that_much() {
        let told = refused(&AxError::failure(
            channels::AxCode::StorageFatal,
            "append to the ledger",
            "the disk is full",
        ));
        assert!(!told.recovery.is_empty());
    }

    /// Two identical refusals are two answers. The dedup that protects a
    /// person from a run frozen for an hour would, applied here, swallow
    /// the answer to their second attempt.
    #[test]
    fn the_same_refusal_twice_is_two_answers_not_one_fact() {
        let err = AxError::failure(channels::AxCode::InvalidArgs, "select a model", "nowhere");
        assert_eq!(refused(&err), refused(&err));
    }
}
