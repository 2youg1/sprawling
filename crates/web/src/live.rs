// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

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

use crate::lang::{Msg, fill, say};
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
    /// What this line says, as a message; `None` for a kind this build
    /// has no sentence for, which falls back to the kind's own name.
    pub msg: Option<crate::lang::Msg>,
    /// Who or where it happened: an address when the record has one.
    pub who: String,
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
            msg: describe(record).0,
            who: describe(record).1,
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
/// prints raw payloads is a log, and the reason to watch a session is to
/// see its shape, not its bytes. The bytes are one click away in
/// `ledger_view`.
///
/// **Amended (ux-9).** That ruling holds and it was answering the wrong
/// question. What it forbids is a dump; what it was read as forbidding is
/// any disclosure at all, and the cost of the wider reading is that
/// everything this product does differently happens inside a single turn
/// and reaches the screen as another grey line - a refusal in three
/// parts, a checkpoint fence, a write outside its domain, a compaction
/// that reports what it dropped. `read` and `read src/lex.rs` differ by
/// nothing a byte count can measure: the second says what it did.
///
/// So this function keeps its job, which is one short line per event, and
/// `web::turn` folds the same records into the rounds a person reads. The
/// bytes are still only in the Ledger, and a call still carries the `seq`
/// that addresses them.
#[must_use]
pub fn describe(record: &EventRecord) -> (Option<Msg>, String) {
    let who = record
        .addr()
        .map_or_else(|| record.who().to_owned(), |addr| addr.as_str().to_owned());
    let msg = match record.kind() {
        EventKind::ToolCalled => Some(Msg::LineToolCalled),
        EventKind::ToolResult => Some(Msg::LineToolResult),
        EventKind::ModelCalled => Some(Msg::LineModelCalled),
        EventKind::ModelReturned => Some(Msg::LineModelReturned),
        EventKind::SteerReceived => Some(Msg::LineSteered),
        EventKind::GateDenied => Some(Msg::LineGateDenied),
        EventKind::ApprovalRequested => Some(Msg::LineApprovalRequested),
        EventKind::RunFrozen => Some(Msg::LineRunFrozen),
        // A kind this build has no sentence for still produces a line:
        // the kind's own name beside who did it.
        _ => None,
    };
    (msg, who)
}

/// One line, said. `None` from [`describe`] falls back to the event's own
/// kind, which is English in the Ledger and stays English here.
#[must_use]
pub fn describe_in(lang: crate::lang::Lang, record: &EventRecord) -> String {
    match describe(record) {
        (Some(msg), who) => crate::lang::fill(crate::lang::say(lang, msg), &[("who", &who)]),
        (None, who) => format!("{:?} · {who}", record.kind()),
    }
}

/// One session as it happens, plus the two things a person does while
/// watching: say something into it, or stop it.
#[component]
pub fn LiveView(
    feed: Feed,
    /// The same records the feed holds, folded into rounds.
    ///
    /// Passed rather than folded here because `Feed` keeps lines and not
    /// records: it is the window, and the window's job is to say what it
    /// dropped. Two readings of one list, never two lists.
    turns: Vec<crate::turn::Turn>,
    run: Option<RunId>,
    /// Every run the client knows of, newest first, with the word the
    /// page shows for its phase.
    runs: Vec<(RunId, String)>,
    following: bool,
    on_frame: EventHandler<ClientFrame>,
    on_follow: EventHandler<bool>,
    on_watch: EventHandler<Option<RunId>>,
) -> Element {
    let lang = use_context::<Signal<crate::lang::Lang>>();
    let word = move |msg: Msg| say(lang(), msg);
    let mut steer = use_signal(String::new);
    let lines = feed.lines().to_vec();
    let dropped = feed.dropped();
    let held = lines.len();
    let known = runs.len();
    let last_seq = lines.last().map(|line| line.seq);
    let dropped_line = fill(word(Msg::LiveDropped), &[("dropped", &dropped.to_string())]);
    rsx! {
        section { class: "live",
            crate::panel::Panel {
                // Never a claim about the city: this window opens when the
                // page connects, so "no run has been dispatched here" is a
                // sentence it has no standing to say. The overview reads
                // the city's own count for that.
                title: match (known, run) {
                    (0, _) => word(Msg::LiveNothingSinceConnected).to_owned(),
                    (_, Some(_)) => word(Msg::LiveOneSession).to_owned(),
                    (_, None) => word(Msg::LiveEveryRun).to_owned(),
                },
                figure: (held > 0).then(|| held.to_string()),
                scope: word(Msg::LiveScope).to_owned(),
                source: word(Msg::LiveSource).to_owned(),
            // Which session is being watched is a choice, not a guess.
            // With two runs in flight, "the latest one" is a coin toss,
            // and the page was showing one of them without saying so.
            div { class: "runs",
                button {
                    "aria-current": if run.is_none() { "true" } else { "false" },
                    onclick: move |_| on_watch.call(None),
                    "{word(Msg::LiveEverything)}"
                }
                for (id, said) in runs.clone() {
                    button {
                        key: "{id}",
                        "aria-current": if run == Some(id) { "true" } else { "false" },
                        onclick: move |_| on_watch.call(Some(id)),
                        "{said}"
                    }
                }
            }
            header { class: "live-head",
                match run {
                    // The session first, because that is what the person
                    // called it; the run identifier stays on the page
                    // because it is what the Ledger and `sprawling fork`
                    // are addressed by.
                    Some(id) => rsx! {
                        span { class: "run", "{named(&runs, id)}" }
                        span { class: "run-id", "{run_id_line(lang(), id)}" }
                    },
                    None => rsx! { span { class: "run", "{word(Msg::LiveEveryRun)}" } },
                }
                label { class: "follow",
                    input {
                        r#type: "checkbox",
                        checked: following,
                        onchange: move |event| on_follow.call(event.checked()),
                    }
                    "{word(Msg::LiveFollowEnd)}"
                }
            }
            if dropped > 0 {
                p { class: "dropped", "{dropped_line}" }
            }
            // A turn is one row, and what it did is inside it. The event
            // stream is the Ledger's shape; this is the reader's, and
            // both are readings of the same records.
            ol { class: "turns",
                for round in turns {
                    li { key: "{round.opened.value()}", class: "turn",
                        header { class: "turn-head",
                            span { class: "n",
                                "{fill(word(Msg::TurnNumber), &[(\"n\", &round.number.to_string())])}"
                            }
                            span { class: "seq", "{round.opened.value()}" }
                            span { class: "count",
                                if round.calls.is_empty() {
                                    "{word(Msg::TurnNoTools)}"
                                } else {
                                    "{fill(word(Msg::TurnTools), &[(\"count\", &round.calls.len().to_string())])}"
                                }
                            }
                            // What this one turn cost. Both numbers are
                            // in `model_returned`; neither is derived
                            // here, and the token figure carries no
                            // denominator because the wire has none.
                            if let Some(spent) = round.spent {
                                span { class: "spent", "{crate::app::render_usd(spent)}" }
                            }
                            if let Some(used) = round.used {
                                span { class: "used", "{tokens_line(lang(), used)}" }
                            }
                            if let Some(ref why) = round.stopped {
                                span { class: "stopped",
                                    "{fill(word(Msg::TurnStopped), &[(\"why\", why)])}"
                                }
                            }
                        }
                        // What the model actually said. The reason this
                        // page exists rather than a tool log, and the
                        // one thing the fold used to drop wholesale.
                        if let Some(ref said) = round.said {
                            p { class: "said", "{said}" }
                        }
                        for call in round.calls {
                            div { key: "{call.at.value()}", class: "call",
                                span { class: "tool", "{call.tool}" }
                                // What it acted on. Absent when the
                                // arguments name no one thing, which is
                                // what every row used to look like.
                                if let Some(ref on) = call.subject {
                                    span { class: "arg", "{on}" }
                                }
                                span { class: "{call.outcome.class()}",
                                    "{word(call.outcome.word())}"
                                }
                                // Where the bytes are. The row shows a
                                // shape; this addresses the rest of it.
                                span { class: "seq", "{call.at.value()}" }
                                // A disclosure, not a dump: one click,
                                // bounded, and it states what it cut.
                                if let Some(ref said) = call.output {
                                    details { class: "output",
                                        summary { "{word(Msg::TurnOutput)}" }
                                        pre { "{said.head}" }
                                        if said.cut > 0 {
                                            p { class: "cut", "{cut_line(lang(), said.cut, call.at)}" }
                                        }
                                    }
                                }
                            }
                        }
                        // What else happened in this turn. Everything
                        // here changed what the turn did or what it is
                        // waiting on; the rest stays in the stream.
                        for note in round.notes {
                            div { key: "note-{note.at().value()}", class: "note",
                                match note {
                                    crate::turn::Note::Refused { ref error, .. } => {
                                        let said = crate::alert::refused(lang(), error);
                                        rsx! {
                                            span { class: "code", "{said.code}" }
                                            span { class: "what", "{said.what}" }
                                            span { class: "recovery", "{said.recovery}" }
                                        }
                                    }
                                    crate::turn::Note::Fenced { ref oid, .. } => rsx! {
                                        span { class: "what",
                                            "{fill(word(Msg::NoteFenced), &[(\"oid\", oid)])}"
                                        }
                                    },
                                    crate::turn::Note::Waiting { .. } => rsx! {
                                        span { class: "what", "{word(Msg::NoteWaiting)}" }
                                    },
                                    crate::turn::Note::Arrived { ref from, ref said, .. } => rsx! {
                                        span { class: "what",
                                            "{fill(word(Msg::NoteArrived), &[(\"from\", from)])}"
                                        }
                                        if !said.is_empty() {
                                            span { class: "arg", "{said}" }
                                        }
                                    },
                                    crate::turn::Note::Discarded { count, .. } => rsx! {
                                        span { class: "what",
                                            "{fill(word(Msg::NoteDiscarded), &[(\"count\", &count.to_string())])}"
                                        }
                                    },
                                }
                                span { class: "seq", "{note.at().value()}" }
                            }
                        }
                    }
                }
            }
            // The event stream stays, one click down: it is what the
            // Ledger holds, and a reader who wants the raw order should
            // not have to leave the page to see it. Absent when there is
            // nothing behind it, because a control that opens onto an
            // empty list teaches the reader it is not worth opening.
            details { class: "stream", hidden: held == 0,
                summary { "{word(Msg::LiveEveryEvent)}" }
                ol { class: "lines",
                    for line in lines {
                        li {
                            key: "{line.seq.value()}",
                            class: if line.from_person { "line person" } else { "line" },
                            span { class: "seq", "{line.seq.value()}" }
                            span { class: "kind", "{line.kind:?}" }
                            span { class: "text", "{line_text(lang(), &line)}" }
                        }
                    }
                }
            }
            if feed.lines().is_empty() {
                crate::panel::Empty {
                    status: if known == 0 {
                        word(Msg::LiveNoRunYet).to_owned()
                    } else {
                        word(Msg::LiveNothingSince).to_owned()
                    },
                    what: if known == 0 {
                        word(Msg::LiveNoRunYetWhat).to_owned()
                    } else {
                        word(Msg::LiveNothingSinceWhat).to_owned()
                    },
                }
            }
            // Speaking into a run needs a run. With none chosen this was
            // an input box beside a button that could never fire, which
            // reads as a broken control rather than as a missing choice.
            match run {
                None => rsx! {
                    p { class: "note",
                        "{word(Msg::LivePickASession)}"
                    }
                },
                Some(id) => rsx! {
                    form {
                        class: "steer",
                        onsubmit: move |event| {
                            event.prevent_default();
                            let said = steer.read().trim().to_owned();
                            if said.is_empty() {
                                return;
                            }
                            on_frame.call(steer_command(id, &said));
                            steer.set(String::new());
                        },
                        input {
                            name: "steer",
                            placeholder: "{word(Msg::LiveSteerPlaceholder)}",
                            value: "{steer}",
                            oninput: move |event| steer.set(event.value()),
                        }
                        button {
                            r#type: "submit",
                            disabled: steer.read().trim().is_empty(),
                            "{word(Msg::LiveSteerSend)}"
                        }
                    }
                },
            }
            // The rest of what a person may do to a run. Each says what it
            // makes rather than how it feels, and none of them acts on a
            // guess about which run was meant.
            if let Some(id) = run {
                div { class: "interventions",
                    button {
                        class: "quiet",
                        onclick: move |_| on_frame.call(takeover_command(id)),
                        "{word(Msg::LiveTakeover)}"
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
                            Some(at) => rsx! { "{fork_line(lang(), at)}" },
                            None => rsx! { "{word(Msg::LiveNothingToBranch)}" },
                        }
                    }
                    p { class: "note",
                        "{word(Msg::LiveInterventionNote)}"
                    }
                }
            }
            }
        }
    }
}

/// One line of the feed, said. The kind stays as the Ledger spells it -
/// it is an identifier, not a sentence - and only the description is
/// translated.
fn line_text(lang: crate::lang::Lang, line: &Line) -> String {
    match line.msg {
        Some(msg) => fill(say(lang, msg), &[("who", &line.who)]),
        None => format!("{:?} \u{b7} {}", line.kind, line.who),
    }
}

/// The run identifier, said.
fn run_id_line(lang: crate::lang::Lang, run: RunId) -> String {
    fill(say(lang, Msg::LiveRunId), &[("id", &run.to_string())])
}

/// What one turn spent in tokens, said. Absolute counts: the wire
/// carries no context window, so there is no denominator to divide by.
fn tokens_line(lang: crate::lang::Lang, used: crate::turn::Used) -> String {
    fill(
        say(lang, Msg::TurnTokens),
        &[
            ("input", &used.input.get().to_string()),
            ("output", &used.output.get().to_string()),
        ],
    )
}

/// How much of a tool's output this row did not show, and where the rest
/// is. A window that hides without saying so is worse than no window.
fn cut_line(lang: crate::lang::Lang, cut: usize, at: Seq) -> String {
    fill(
        say(lang, Msg::TurnOutputCut),
        &[("cut", &cut.to_string()), ("seq", &at.value().to_string())],
    )
}

/// Where a branch would be taken from, said.
fn fork_line(lang: crate::lang::Lang, at: Seq) -> String {
    fill(
        say(lang, Msg::LiveForkFrom),
        &[("seq", &at.value().to_string())],
    )
}

/// What this page calls the session being watched, taken from the same
/// list the picker renders so the heading and the button cannot disagree.
fn named(runs: &[(RunId, String)], run: RunId) -> String {
    runs.iter()
        .find(|(id, _)| *id == run)
        .map_or_else(|| short_run(run), |(_, said)| said.clone())
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
        let text = describe_in(
            crate::lang::Lang::En,
            &record(1, EventKind::ToolCalled, "resident"),
        );
        assert_eq!(text, "resident calls a tool");
        // An unmodelled kind still produces a line rather than a blank,
        // and the kind keeps the spelling the Ledger uses.
        let fallback = describe_in(
            crate::lang::Lang::Zh,
            &record(2, EventKind::PromptAssembled, "resident"),
        );
        assert!(fallback.contains("resident"));
        assert!(fallback.contains("PromptAssembled"));
    }
}
