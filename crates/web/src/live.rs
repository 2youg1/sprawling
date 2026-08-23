// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Watching one session as it happens.
//!
//! A live view is where an interface most easily becomes a firehose, so two
//! decisions are made here and nowhere else:
//!
//! **The window is bounded.** A Run that goes all night would otherwise grow
//! a tab until it dies. Older lines fall out of the view, not out of the
//! history - `ledger_view` still has every one of them, which is the whole
//! reason that module exists.
//!
//! **Following is sticky, not forced.** A person who scrolls back is
//! reading something; yanking them to the bottom on the next event would
//! take it away. Following resumes when they return to the end themselves.

use channels::{ClientFrame, EventKind, EventRecord, RunId, Seq};
use dioxus::prelude::*;

/// How many lines the live view keeps. Chosen to cover the length of a
/// working session on screen, not the length of a Run.
pub const WINDOW: usize = 200;

/// One rendered line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Line {
    pub seq: Seq,
    pub kind: EventKind,
    pub text: String,
    /// Whether the line came from a person rather than from the Run. A human
    /// Steer is prefixed `user`; an Agent's carries its id and name
    ///. Both land in the same place, so the model only
    /// learns one shape - and so does the reader.
    pub from_person: bool,
}

/// The session view: a bounded window plus whether it is following.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Feed {
    lines: Vec<Line>,
    following: bool,
    /// Lines dropped off the top. Shown, so the window never pretends to be
    /// the history.
    dropped: usize,
}

impl Default for Feed {
    fn default() -> Self {
        Self::new()
    }
}

impl Feed {
    #[must_use]
    pub fn new() -> Self {
        Self {
            lines: Vec::new(),
            following: true,
            dropped: 0,
        }
    }

    #[must_use]
    pub fn lines(&self) -> &[Line] {
        &self.lines
    }

    #[must_use]
    pub fn is_following(&self) -> bool {
        self.following
    }

    #[must_use]
    pub fn dropped(&self) -> usize {
        self.dropped
    }

    /// The reader scrolled away from the end.
    pub fn stop_following(&mut self) {
        self.following = false;
    }

    /// The reader came back to the end themselves.
    pub fn follow(&mut self) {
        self.following = true;
    }

    /// Rebuilds the window from the records the client is holding.
    ///
    /// The client keeps records, not rendered lines: one store, and every
    /// page that reads history reads it. Rebuilding here rather than
    /// keeping a second incremental copy is what makes "throw the view
    /// away and fold again" true of this page too.
    #[must_use]
    pub fn replay<'a>(
        records: impl IntoIterator<Item = &'a EventRecord>,
        run: Option<RunId>,
        following: bool,
    ) -> Feed {
        let mut feed = Feed::new();
        if !following {
            feed.stop_following();
        }
        for record in records {
            if run.is_none_or(|wanted| record.run() == wanted) {
                feed.push(record);
            }
        }
        feed
    }

    /// Appends one event, trimming the top if the window is full.
    ///
    /// Returns whether the view should scroll. It never scrolls while the
    /// reader is away, which is the difference between a live view and a
    /// view that fights its reader.
    pub fn push(&mut self, record: &EventRecord) -> bool {
        self.lines.push(Line {
            seq: record.seq(),
            kind: record.kind(),
            text: describe(record),
            from_person: matches!(record.kind(), EventKind::SteerReceived)
                && record.who() == "user",
        });
        if self.lines.len() > WINDOW {
            let excess = self.lines.len().saturating_sub(WINDOW);
            self.lines.drain(..excess);
            self.dropped = self.dropped.saturating_add(excess);
        }
        self.following
    }
}

/// One line of text for an event.
///
/// Deliberately short and deliberately not the payload: a live view that
/// prints raw payloads is a log, and the reason to watch a session is to see
/// its shape, not its bytes. The bytes are one click away in `ledger_view`.
#[must_use]
pub fn describe(record: &EventRecord) -> String {
    let kind = record.kind();
    let who = record.who();
    match record.kind() {
        EventKind::ToolCalled => format!("{who} calls a tool"),
        EventKind::ToolResult => format!("{who} reads the result"),
        EventKind::ModelCalled => format!("{who} asks the model"),
        EventKind::ModelReturned => format!("the model answers {who}"),
        EventKind::SteerReceived => format!("{who} steers"),
        EventKind::GateDenied => format!("a gate refused {who}"),
        EventKind::ApprovalRequested => format!("{who} needs a person"),
        EventKind::RunFrozen => format!("{who} is frozen"),
        _ => format!("{kind:?} · {who}"),
    }
}

/// One session as it happens, plus the two things a person does while
/// watching: say something into it, or stop it.
#[component]
pub fn LiveView(
    feed: Feed,
    run: Option<RunId>,
    /// Every run the client knows of, newest first, with the word the
    /// page shows for its phase.
    runs: Vec<(RunId, String)>,
    following: bool,
    on_frame: EventHandler<ClientFrame>,
    on_follow: EventHandler<bool>,
    on_watch: EventHandler<Option<RunId>>,
) -> Element {
    let mut steer = use_signal(String::new);
    let lines = feed.lines().to_vec();
    let dropped = feed.dropped();
    let held = lines.len();
    let known = runs.len();
    let last_seq = lines.last().map(|line| line.seq);
    rsx! {
        section { class: "live",
            crate::panel::Panel {
                // Never a claim about the city: this window opens when the
                // page connects, so "no run has been dispatched here" is a
                // sentence it has no standing to say. The overview reads
                // the city's own count for that.
                title: match (known, run) {
                    (0, _) => "nothing has happened since this page connected".to_owned(),
                    (_, Some(_)) => "one session, as it happens".to_owned(),
                    (_, None) => "every run in this city, as it happens".to_owned(),
                },
                figure: (held > 0).then(|| held.to_string()),
                scope: "a bounded window: the figure counts the lines held here, and a line that leaves the window has not left the Ledger"
                    .to_owned(),
                source: "the live event stream, folded one record at a time. Nothing here is re-asked or polled - the same fold the server does, running in this page."
                    .to_owned(),
            // Which session is being watched is a choice, not a guess.
            // With two runs in flight, "the latest one" is a coin toss,
            // and the page was showing one of them without saying so.
            div { class: "runs",
                button {
                    "aria-current": if run.is_none() { "true" } else { "false" },
                    onclick: move |_| on_watch.call(None),
                    "everything"
                }
                for (id, phase) in runs {
                    button {
                        key: "{id}",
                        "aria-current": if run == Some(id) { "true" } else { "false" },
                        onclick: move |_| on_watch.call(Some(id)),
                        "{short_run(id)} · {phase}"
                    }
                }
            }
            header { class: "live-head",
                match run {
                    Some(id) => rsx! { span { class: "run", "run {id}" } },
                    None => rsx! { span { class: "run", "every run in this city" } },
                }
                label { class: "follow",
                    input {
                        r#type: "checkbox",
                        checked: following,
                        onchange: move |event| on_follow.call(event.checked()),
                    }
                    "follow the end"
                }
            }
            if dropped > 0 {
                p { class: "dropped",
                    "{dropped} earlier line(s) left this window; the ledger still has them"
                }
            }
            ol { class: "lines",
                for line in lines {
                    li {
                        key: "{line.seq.value()}",
                        class: if line.from_person { "line person" } else { "line" },
                        span { class: "seq", "{line.seq.value()}" }
                        span { class: "kind", "{line.kind:?}" }
                        span { class: "text", "{line.text}" }
                    }
                }
            }
            if feed.lines().is_empty() {
                crate::panel::Empty {
                    status: if known == 0 { "no run has reported here yet".to_owned() }
                        else { "nothing has happened here since this page connected".to_owned() },
                    what: if known == 0 {
                        "this window holds what arrives from now on, so a run that finished before you opened this page is in the Ledger rather than here. Send work from the bar below and every turn it takes appears as it happens.".to_owned()
                    } else {
                        "this window holds what arrived after the page connected. Earlier lines are in the Ledger, which the record page reads.".to_owned()
                    },
                }
            }
            form {
                class: "steer",
                onsubmit: move |event| {
                    event.prevent_default();
                    let Some(id) = run else { return };
                    let said = steer.read().trim().to_owned();
                    if said.is_empty() {
                        return;
                    }
                    on_frame.call(steer_command(id, &said));
                    steer.set(String::new());
                },
                input {
                    name: "steer",
                    placeholder: "say something into this run",
                    value: "{steer}",
                    oninput: move |event| steer.set(event.value()),
                }
                button {
                    r#type: "submit",
                    disabled: run.is_none() || steer.read().trim().is_empty(),
                    "send at the next safe point"
                }
            }
            // The rest of what a person may do to a run. Each says what it
            // makes rather than how it feels, and none of them acts on a
            // guess about which run was meant.
            if let Some(id) = run {
                div { class: "interventions",
                    button {
                        class: "quiet",
                        onclick: move |_| on_frame.call(takeover_command(id)),
                        "answer for this run from here"
                    }
                    button {
                        class: "quiet",
                        disabled: last_seq.is_none(),
                        onclick: move |_| {
                            if let Some(at) = last_seq {
                                on_frame.call(fork_command(id, at));
                            }
                        },
                        match last_seq {
                            Some(at) => rsx! { "branch a new run from step {at.value()}" },
                            None => rsx! { "nothing to branch from yet" },
                        }
                    }
                    p { class: "note",
                        "A branch records where it came from and does not start working by itself. Taking over answers for this run from here; what it already did is not undone."
                    }
                }
            }
            }
        }
    }
}

/// A run's identifier, shortened for a button. The full one is in the
/// header of the page it opens, so nothing is lost by shortening here.
#[must_use]
pub fn short_run(run: RunId) -> String {
    let full = run.to_string();
    full.split('-').next().unwrap_or(&full).to_owned()
}

/// Taking a run over: the person answers for it from here.
///
/// One of the five interventions `channels::control` classifies, and one
/// of the two this client had no way to send at all. An interface for
/// delegated work whose only verbs are "say something" and "stop" is not
/// a control surface; it is a transcript with a kill switch.
#[must_use]
pub fn takeover_command(run: RunId) -> ClientFrame {
    ClientFrame::Command(Box::new(channels::WireCommand::Takeover {
        idem: channels::IdemKey::derive(&run, Seq::FIRST, b"takeover"),
        run,
    }))
}

/// Branching a new run from a point in this one's history.
///
/// `at_seq` is the last record this page has seen for the run, which is
/// the only point a person watching it can actually mean. A fork records
/// a lineage and does not start driving by itself, so the button says
/// what it makes rather than what it starts.
#[must_use]
pub fn fork_command(run: RunId, at_seq: Seq) -> ClientFrame {
    ClientFrame::Command(Box::new(channels::WireCommand::Fork {
        idem: channels::IdemKey::derive(&run, at_seq, b"fork"),
        run,
        at_seq,
        addr: None,
    }))
}

/// A person's word into a running session. It arrives at the next safe
/// point rather than immediately, which is the difference between
/// steering a run and corrupting one.
#[must_use]
fn steer_command(run: RunId, text: &str) -> ClientFrame {
    ClientFrame::Command(Box::new(channels::WireCommand::Steer {
        idem: channels::IdemKey::derive(&run, Seq::FIRST, text.as_bytes()),
        run,
        text: text.to_owned(),
    }))
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code"
)]
mod tests {
    use super::*;
    use channels::{B3Hash, EventDraft, Payload, RunId, TimeMs};

    fn record(seq: u64, kind: EventKind, who: &str) -> EventRecord {
        EventRecord::from_draft(
            EventDraft {
                run: RunId::from_bytes([1u8; 16]),
                t: TimeMs::new(seq),
                who: who.to_owned(),
                addr: None,
                kind,
                data: Payload::empty(),
                ig: false,
            },
            Seq::new(seq),
            B3Hash::digest(b"prev"),
        )
    }

    #[test]
    fn the_window_is_bounded_and_says_what_it_dropped() {
        let mut feed = Feed::new();
        let overflow = u64::try_from(WINDOW).unwrap() + 50;
        for seq in 1..=overflow {
            feed.push(&record(seq, EventKind::ToolResult, "resident"));
        }
        assert_eq!(feed.lines().len(), WINDOW);
        assert_eq!(feed.dropped(), 50);
        // The oldest line still on screen is the 51st, and the history has
        // all of them - that is what ledger_view is for.
        assert_eq!(feed.lines()[0].seq, Seq::new(51));
    }

    #[test]
    fn a_reader_who_scrolled_back_is_not_yanked_to_the_bottom() {
        let mut feed = Feed::new();
        assert!(feed.push(&record(1, EventKind::ToolCalled, "r")));
        feed.stop_following();
        assert!(!feed.push(&record(2, EventKind::ToolResult, "r")));
        assert!(!feed.push(&record(3, EventKind::ToolResult, "r")));
        // The lines still arrive; only the scroll is withheld.
        assert_eq!(feed.lines().len(), 3);
        feed.follow();
        assert!(feed.push(&record(4, EventKind::ToolResult, "r")));
    }

    #[test]
    fn a_persons_steer_is_marked_and_an_agents_is_not() {
        let mut feed = Feed::new();
        feed.push(&record(1, EventKind::SteerReceived, "user"));
        feed.push(&record(2, EventKind::SteerReceived, "@7 (auditor)"));
        assert!(feed.lines()[0].from_person);
        assert!(!feed.lines()[1].from_person);
    }

    #[test]
    fn a_line_describes_the_shape_and_not_the_bytes() {
        let text = describe(&record(1, EventKind::ToolCalled, "resident"));
        assert_eq!(text, "resident calls a tool");
        // An unmodelled kind still produces a line rather than a blank.
        let fallback = describe(&record(2, EventKind::PromptAssembled, "resident"));
        assert!(fallback.contains("resident"));
        assert!(!fallback.is_empty());
    }
}
