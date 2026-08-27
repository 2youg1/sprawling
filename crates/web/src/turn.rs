// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! A session's events, folded into the rounds a person actually reads.
//!
//! The unit of a session is not an event, it is a **turn**: the model is
//! asked once, it answers, and the tools it asked for run. `web::live`
//! renders the event stream, which is the Ledger's shape rather than a
//! reader's - and the consequence is that everything this product does
//! differently happens inside one turn and arrives on screen as another
//! grey line: a refusal in three parts, a checkpoint fence, a write
//! outside the domain, a compaction that reports what it dropped.
//!
//! **This is a disclosure, not a dump.** A turn is still one row. The
//! difference between `read` and `read crates/parser/src/lex.rs` is not
//! how many bytes are shown but whether the row says what it did, and the
//! bytes stay where they were: in the Ledger, addressed by `seq`.
//!
//! The payload shapes read here are the ones `runtime::turn` writes:
//! `tool_called` carries `{ id, name, args }` and `tool_result` carries
//! `{ tool_use_id, name }` with either `result` or `error`. Nothing else
//! is assumed, and a payload that does not match still produces a row -
//! a client one version behind must not drop a call it cannot parse.

use channels::{EventKind, EventRecord, Seq};

/// What a tool call has come to so far.
///
/// Three states rather than a `bool` and an `Option`: a call still
/// running and a call that failed are different things to a person
/// deciding whether to step in, and the pair could spell a fourth state
/// that cannot happen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Outcome {
    /// Called, and no result has arrived in this window.
    Waiting,
    /// Answered.
    Answered,
    /// Answered with an error. **Not an alert**: one failed call is a
    /// fact, not a request for a person. If it actually stopped the
    /// session, the freeze raises its own card.
    Failed,
}

impl Outcome {
    /// The word for this outcome, as a message rather than a string, so a
    /// state cannot be the one English word left on a Chinese page.
    #[must_use]
    pub fn word(self) -> crate::lang::Msg {
        match self {
            Self::Waiting => crate::lang::Msg::TurnWaiting,
            Self::Answered => crate::lang::Msg::TurnAnswered,
            Self::Failed => crate::lang::Msg::TurnFailed,
        }
    }

    /// The class a row takes, so lightness and a word carry the state
    /// together - colour is a redundant layer here as everywhere.
    #[must_use]
    pub fn class(self) -> &'static str {
        match self {
            Self::Waiting => "out waiting",
            Self::Answered => "out answered",
            Self::Failed => "out failed",
        }
    }
}

/// One tool call inside a turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Call {
    /// The tool's own name, as the Ledger records it.
    pub tool: String,
    /// What it acted on, when the arguments name one thing.
    ///
    /// A display reading rather than a field: the arguments are free JSON
    /// and this picks the one a person recognises the call by. `None`
    /// prints the tool alone, which is what the old line did for every
    /// call.
    pub subject: Option<String>,
    pub outcome: Outcome,
    /// Where in the Ledger the bytes are. The row shows a shape; this is
    /// how somebody reads the rest.
    pub at: Seq,
}

/// One turn: the model was asked, and this is what came of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    /// Counted from one, in the order the turns opened.
    pub number: u32,
    /// The event that opened it.
    pub opened: Seq,
    pub calls: Vec<Call>,
}

/// Argument names that say what a call acted on, in the order they are
/// preferred.
///
/// Taken from the tool definitions rather than guessed: `path` is what
/// twelve of them take, and the rest name their one subject.
const SUBJECT_KEYS: [&str; 4] = ["path", "addr", "program", "arm"];

/// Folds a session's events into turns, oldest first.
///
/// Events before the first `model_called` belong to no turn and are left
/// out: they are the session opening, which the head of the page already
/// states. A `tool_result` with no call in this window is dropped for the
/// same reason - the window is bounded, so its first rows can be answers
/// to calls nobody here saw.
#[must_use]
pub fn turns<'a>(records: impl IntoIterator<Item = &'a EventRecord>) -> Vec<Turn> {
    let mut folded: Vec<Turn> = Vec::new();
    // Which turn each outstanding call sits in, by the id the runtime
    // gave it. Answers arrive after other calls have been made, so the
    // pairing cannot be positional.
    let mut awaiting: Vec<(String, usize, usize)> = Vec::new();
    for record in records {
        match record.kind() {
            EventKind::ModelCalled => {
                let number = u32::try_from(folded.len().saturating_add(1)).unwrap_or(u32::MAX);
                folded.push(Turn {
                    number,
                    opened: record.seq(),
                    calls: Vec::new(),
                });
            }
            EventKind::ToolCalled => {
                // Nothing open means this is the session's own opening,
                // which belongs to no round.
                let turn_at = match folded.len() {
                    0 => continue,
                    open => open.saturating_sub(1),
                };
                let map = record.data().as_map();
                let call = Call {
                    tool: text(map.get("name")).unwrap_or_else(|| "tool".to_owned()),
                    subject: subject_of(map.get("args")),
                    outcome: Outcome::Waiting,
                    at: record.seq(),
                };
                let Some(turn) = folded.get_mut(turn_at) else {
                    continue;
                };
                if let Some(id) = text(map.get("id")) {
                    awaiting.push((id, turn_at, turn.calls.len()));
                }
                turn.calls.push(call);
            }
            EventKind::ToolResult => {
                let map = record.data().as_map();
                let Some(id) = text(map.get("tool_use_id")) else {
                    continue;
                };
                let Some(at) = awaiting.iter().position(|(held, _, _)| held == &id) else {
                    continue;
                };
                let (_, turn_at, call_at) = awaiting.swap_remove(at);
                let outcome = if map.contains_key("error") {
                    Outcome::Failed
                } else {
                    Outcome::Answered
                };
                if let Some(call) = folded
                    .get_mut(turn_at)
                    .and_then(|turn| turn.calls.get_mut(call_at))
                {
                    call.outcome = outcome;
                }
            }
            _ => {}
        }
    }
    folded
}

/// A JSON string, when the value is one.
fn text(value: Option<&serde_json::Value>) -> Option<String> {
    value.and_then(|held| held.as_str()).map(str::to_owned)
}

/// The one argument a person recognises a call by.
fn subject_of(args: Option<&serde_json::Value>) -> Option<String> {
    let map = args?.as_object()?;
    for key in SUBJECT_KEYS {
        if let Some(named) = text(map.get(key)) {
            return Some(named);
        }
    }
    // A tool this build has no preferred key for still says something,
    // rather than falling back to the bare tool name.
    map.values().find_map(|value| text(Some(value)))
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
    use super::{Outcome, turns};
    use channels::{B3Hash, EventDraft, EventKind, EventRecord, Payload, RunId, Seq, TimeMs};

    fn record(seq: u64, kind: EventKind, data: serde_json::Value) -> EventRecord {
        let map = data.as_object().expect("a payload is an object").clone();
        EventRecord::from_draft(
            EventDraft {
                run: RunId::from_bytes([7u8; 16]),
                t: TimeMs::new(seq),
                who: "lab/parser".to_owned(),
                addr: None,
                kind,
                data: Payload::new(map).expect("a payload"),
                ig: false,
            },
            Seq::new(seq),
            B3Hash::digest(b"prev"),
        )
    }

    fn called(seq: u64, id: &str, name: &str, path: &str) -> EventRecord {
        record(
            seq,
            EventKind::ToolCalled,
            serde_json::json!({ "id": id, "name": name, "args": { "path": path } }),
        )
    }

    fn answered(seq: u64, id: &str) -> EventRecord {
        record(
            seq,
            EventKind::ToolResult,
            serde_json::json!({ "tool_use_id": id, "name": "read", "result": { "lines": 412 } }),
        )
    }

    fn failed(seq: u64, id: &str) -> EventRecord {
        record(
            seq,
            EventKind::ToolResult,
            serde_json::json!({ "tool_use_id": id, "name": "exec", "error": { "code": 101 } }),
        )
    }

    fn asked(seq: u64) -> EventRecord {
        record(seq, EventKind::ModelCalled, serde_json::json!({}))
    }

    #[test]
    fn a_turn_opens_when_the_model_is_asked_and_gathers_what_followed() {
        let events = [
            asked(1),
            called(2, "a", "read", "src/lex.rs"),
            answered(3, "a"),
        ];
        let folded = turns(&events);
        assert_eq!(folded.len(), 1);
        assert_eq!(folded[0].number, 1);
        assert_eq!(folded[0].opened, Seq::new(1));
        assert_eq!(folded[0].calls.len(), 1);
        assert_eq!(folded[0].calls[0].tool, "read");
        assert_eq!(folded[0].calls[0].subject.as_deref(), Some("src/lex.rs"));
        assert_eq!(folded[0].calls[0].outcome, Outcome::Answered);
    }

    #[test]
    fn turns_are_numbered_in_the_order_they_opened() {
        let events = [
            asked(1),
            called(2, "a", "read", "x"),
            asked(3),
            called(4, "b", "edit", "y"),
        ];
        let folded = turns(&events);
        assert_eq!(folded.len(), 2);
        assert_eq!((folded[0].number, folded[1].number), (1, 2));
        assert_eq!(folded[1].calls[0].tool, "edit");
    }

    #[test]
    fn an_answer_finds_its_own_call_and_not_the_nearest_one() {
        // Two calls go out before either answers, and the second answers
        // first. Pairing by position would mark the wrong one failed.
        let events = [
            asked(1),
            called(2, "a", "read", "x"),
            called(3, "b", "exec", "cargo test"),
            failed(4, "b"),
            answered(5, "a"),
        ];
        let folded = turns(&events);
        assert_eq!(folded[0].calls[0].outcome, Outcome::Answered, "read");
        assert_eq!(folded[0].calls[1].outcome, Outcome::Failed, "exec");
    }

    #[test]
    fn a_call_still_running_says_so_rather_than_looking_finished() {
        let events = [asked(1), called(2, "a", "exec", "cargo build")];
        assert_eq!(turns(&events)[0].calls[0].outcome, Outcome::Waiting);
    }

    #[test]
    fn an_answer_to_a_call_this_window_never_saw_is_dropped_not_guessed() {
        // The window is bounded, so its first rows can answer calls that
        // scrolled out. Attaching one to whatever call is nearest would
        // report an outcome that never happened.
        let events = [asked(1), called(2, "a", "read", "x"), answered(3, "gone")];
        let folded = turns(&events);
        assert_eq!(folded[0].calls.len(), 1);
        assert_eq!(folded[0].calls[0].outcome, Outcome::Waiting);
    }

    #[test]
    fn work_before_the_first_turn_belongs_to_no_turn() {
        // A result arriving before any model call has no round to sit in,
        // and inventing turn zero would put the session's opening inside
        // a turn nobody took.
        let events = [answered(1, "a"), asked(2)];
        let folded = turns(&events);
        assert_eq!(folded.len(), 1);
        assert!(folded[0].calls.is_empty());
    }

    #[test]
    fn a_call_whose_arguments_this_build_cannot_read_still_gets_a_row() {
        // Fail-open for a view: a client one version behind must show the
        // call it cannot parse, not hide it.
        let odd = record(
            2,
            EventKind::ToolCalled,
            serde_json::json!({ "id": "a", "name": "future", "args": { "shape": 3 } }),
        );
        let events = [asked(1), odd];
        let folded = turns(&events);
        assert_eq!(folded[0].calls[0].tool, "future");
        assert_eq!(folded[0].calls[0].subject, None);
    }

    #[test]
    fn a_tool_with_no_preferred_key_is_still_named_by_what_it_acted_on() {
        let events = [
            asked(1),
            record(
                2,
                EventKind::ToolCalled,
                serde_json::json!({ "id": "a", "name": "note", "args": { "body": "ship it" } }),
            ),
        ];
        assert_eq!(
            turns(&events)[0].calls[0].subject.as_deref(),
            Some("ship it")
        );
    }

    #[test]
    fn the_bytes_stay_addressable_because_every_call_carries_its_seq() {
        let events = [asked(1), called(9, "a", "read", "x")];
        assert_eq!(turns(&events)[0].calls[0].at, Seq::new(9));
    }
}
