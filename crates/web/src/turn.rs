// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

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

use channels::{AxError, EventKind, EventRecord, GitOid, Seq, Tokens, UsdMicros};

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

/// What a tool said, bounded so a wave of output cannot become the page.
///
/// The cut is counted rather than hinted at: §8-47 admits disclosure and
/// refuses dumping, and a reader who cannot see how much was withheld is
/// being dumped on slowly. Bytes already too large were replaced by
/// `runtime::offload` before they reached the Ledger, and that substitute
/// carries its own line naming the `Locator` - so this shows what it was
/// given and never parses that line, which would be a second authority
/// for the substitute's format.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Output {
    /// The first [`OUTPUT_LINES`] lines of it.
    pub head: String,
    /// How many lines this view cut. The rest is in the Ledger at the
    /// call's own `at`.
    pub cut: usize,
}

/// The most lines of one tool's output a row carries.
pub const OUTPUT_LINES: usize = 12;

/// What a turn came to besides the calls it made.
///
/// **The criterion is closed on purpose**: an event earns a `Note` when
/// it changed what this turn did, or what it is waiting on. Everything
/// else stays in the event stream, which is the Ledger's shape rather
/// than a reader's (§8-6). Without that line this enum would grow to
/// fifty-eight arms and stop meaning anything.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Note {
    /// A door refused something. The error travels whole because
    /// `web::alert::refused` is the one place a refusal becomes the three
    /// parts a person needs; taking it apart here would be the second.
    Refused { error: AxError, at: Seq },
    /// A checkpoint fence went up, and this is the commit it made.
    ///
    /// A `GitOid` and not the string it arrived as: the parse is
    /// fail-closed on exactly forty lowercase hex digits, so a payload
    /// this build cannot read produces no row rather than a row nothing
    /// can be asked about. It is what the change list is addressed by.
    Fenced { oid: GitOid, at: Seq },
    /// This turn stopped for a person. What waits and who answers is
    /// `web::approval`'s; copying it here would be a third authority.
    Waiting { at: Seq },
    /// A word arrived - from the person watching, or from another
    /// address that reached this one.
    Arrived { from: String, said: String, at: Seq },
    /// Files went away. Every one carries its way back, which is the
    /// Recycle Bin's to state.
    Discarded { count: usize, at: Seq },
}

impl Note {
    /// Where in the Ledger this note is, so every row can be read further.
    #[must_use]
    pub fn at(&self) -> Seq {
        match *self {
            Self::Refused { at, .. }
            | Self::Fenced { at, .. }
            | Self::Waiting { at }
            | Self::Arrived { at, .. }
            | Self::Discarded { at, .. } => at,
        }
    }
}

/// What one turn cost in tokens.
///
/// Absolute counts and no ratio: the numerator is on the wire and the
/// denominator - this model's context window - is not. A percentage with
/// no denominator is the thing `UnplannedProgress` already refuses to
/// spell, and it would be no more honest here.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Used {
    pub input: Tokens,
    pub output: Tokens,
    pub cached: Tokens,
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
    /// What it said, bounded. `None` when the call has not answered, or
    /// when the result carried nothing this build can read as text.
    pub output: Option<Output>,
}

/// One turn: the model was asked, and this is what came of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Turn {
    /// Counted from one, in the order the turns opened.
    pub number: u32,
    /// The event that opened it.
    pub opened: Seq,
    /// What the model said in this turn, thinking blocks left out: they
    /// are carried end to end for the provider's signature check and are
    /// not this page's to render.
    pub said: Option<String>,
    /// What this one turn was billed.
    pub spent: Option<UsdMicros>,
    pub used: Option<Used>,
    /// Why the model stopped, in the provider's own word.
    pub stopped: Option<String>,
    pub calls: Vec<Call>,
    /// What else happened inside this turn, oldest first.
    pub notes: Vec<Note>,
}

/// Argument names that say what a call acted on, in the order they are
/// preferred.
///
/// Taken from the tool definitions rather than guessed: `path` is what
/// twelve of them take, and the rest name their one subject.
const SUBJECT_KEYS: [&str; 4] = ["path", "addr", "program", "arm"];

/// The checkpoint a session's changes are measured from.
///
/// The first fence of the session, because that is the tree as the work
/// found it; measuring from the latest one would answer "what moved in
/// the last wave", which is a different question and not the one a
/// person opening a session is asking.
#[must_use]
pub fn opened_at(turns: &[Turn]) -> Option<GitOid> {
    turns
        .iter()
        .flat_map(|turn| turn.notes.iter())
        .find_map(|note| match *note {
            Note::Fenced { oid, .. } => Some(oid),
            _ => None,
        })
}

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
                    said: None,
                    spent: None,
                    used: None,
                    stopped: None,
                    calls: Vec::new(),
                    notes: Vec::new(),
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
                    output: None,
                };
                let Some(turn) = folded.get_mut(turn_at) else {
                    continue;
                };
                if let Some(id) = text(map.get("id")) {
                    awaiting.push((id, turn_at, turn.calls.len()));
                }
                turn.calls.push(call);
            }
            EventKind::ModelReturned => {
                // The answer belongs to the turn the question opened.
                // Nothing open means this record is the session's own
                // opening, which no round owns.
                let Some(turn) = folded.last_mut() else {
                    continue;
                };
                let map = record.data().as_map();
                turn.said = map.get("message").and_then(said_in);
                turn.spent = map
                    .get("billed_usd_micros")
                    .and_then(serde_json::Value::as_u64)
                    .map(UsdMicros::new);
                turn.used = map.get("usage").and_then(used_in);
                turn.stopped = text(map.get("stop"));
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
                let (outcome, said) = match map.get("error") {
                    Some(failed) => (Outcome::Failed, Some(failed)),
                    None => (Outcome::Answered, map.get("result")),
                };
                if let Some(call) = folded
                    .get_mut(turn_at)
                    .and_then(|turn| turn.calls.get_mut(call_at))
                {
                    call.outcome = outcome;
                    call.output = said.and_then(output_in);
                }
            }
            kind => {
                // Everything else is a note when it changed what this
                // turn did or what it waits on, and nothing otherwise.
                let Some(note) = note_of(kind, record) else {
                    continue;
                };
                if let Some(turn) = folded.last_mut() {
                    turn.notes.push(note);
                }
            }
        }
    }
    folded
}

/// A JSON string, when the value is one.
fn text(value: Option<&serde_json::Value>) -> Option<String> {
    value.and_then(|held| held.as_str()).map(str::to_owned)
}

/// What the model said, out of the message `runtime::turn` recorded.
///
/// Text blocks only. `Thinking` and `RedactedThinking` are carried end to
/// end so the provider can verify the signature it issued; relaying them
/// is this city's job and publishing them is not.
///
/// Visible to the crate because the sessions list needs the same
/// sentence the session page shows: a row saying one thing and the page
/// under it saying another would be two readings of one record.
pub(crate) fn said_in(message: &serde_json::Value) -> Option<String> {
    let blocks = message.as_object()?.get("content")?.as_array()?;
    let said: Vec<&str> = blocks
        .iter()
        .filter_map(|block| {
            let map = block.as_object()?;
            (map.get("kind")?.as_str()? == "text").then(|| map.get("text")?.as_str())?
        })
        .collect();
    (!said.is_empty()).then(|| said.join("\n"))
}

/// The four counters `ModelUsage` carries. Absent when the provider sent
/// no usage, which is a different fact from having spent nothing.
fn used_in(usage: &serde_json::Value) -> Option<Used> {
    let map = usage.as_object()?;
    let counter = |name: &str| {
        map.get(name)
            .and_then(serde_json::Value::as_u64)
            .map(Tokens::new)
    };
    let input = counter("input_tokens")?;
    let output = counter("output_tokens")?;
    Some(Used {
        input,
        output,
        cached: counter("cache_read_tokens").unwrap_or_default(),
    })
}

/// What a tool said, cut to [`OUTPUT_LINES`] with the remainder counted.
///
/// A result too large to carry was replaced upstream by
/// `runtime::offload`, whose substitute already states its own size and
/// names the `Locator` holding the rest. This never reads that line: the
/// substitute's format has one authority and it is not this module.
fn output_in(said: &serde_json::Value) -> Option<Output> {
    let whole = match said {
        serde_json::Value::String(text) => text.clone(),
        other => other.to_string(),
    };
    if whole.trim().is_empty() {
        return None;
    }
    let head: Vec<&str> = whole.lines().take(OUTPUT_LINES).collect();
    let cut = whole.lines().count().saturating_sub(head.len());
    Some(Output {
        head: head.join("\n"),
        cut,
    })
}

/// Whether this kind changed what the turn did or what it waits on, and
/// what to say about it if so.
///
/// Exhaustive over the kinds that earn a note and closed against the
/// rest: the criterion is in web-SPEC.md section 8-48, and an enum that
/// grew an arm per event would be the event stream with extra steps.
fn note_of(kind: EventKind, record: &EventRecord) -> Option<Note> {
    let at = record.seq();
    let map = record.data().as_map();
    match kind {
        // The carrier table in `kernel::error` decides which codes land
        // under which kind, and `runtime::run` writes the error flat into
        // the payload. A payload that will not read back as one is left
        // to the event stream rather than rendered as a refusal this
        // build invented.
        EventKind::GateDenied
        | EventKind::BudgetLimit
        | EventKind::WatchdogFired
        | EventKind::ProviderDegraded => {
            let value = serde_json::Value::Object(map.clone());
            serde_json::from_value(value)
                .ok()
                .map(|error| Note::Refused { error, at })
        }
        EventKind::ApprovalRequested => Some(Note::Waiting { at }),
        EventKind::CheckpointCommitted => text(map.get("oid"))
            .as_deref()
            .and_then(GitOid::parse)
            .map(|oid| Note::Fenced { oid, at }),
        EventKind::SteerReceived | EventKind::SignalConsumed => Some(Note::Arrived {
            from: text(map.get("source"))
                .or_else(|| text(map.get("from")))
                .unwrap_or_else(|| record.who().to_owned()),
            said: text(map.get("text")).unwrap_or_default(),
            at,
        }),
        EventKind::FileDiscarded => Some(Note::Discarded {
            count: map
                .get("paths")
                .and_then(serde_json::Value::as_array)
                .map_or(1, Vec::len),
            at,
        }),
        _ => None,
    }
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
    use super::{Note, OUTPUT_LINES, Outcome, turns};
    use channels::{
        AxCode, AxError, B3Hash, EventDraft, EventKind, EventRecord, GitOid, Payload, RunId, Seq,
        TimeMs, Tokens, UsdMicros,
    };

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

    /// The payload `runtime::turn` writes when the model answers. Five
    /// fields, and this fold used to read none of them.
    fn answered_model(seq: u64, text: &str) -> EventRecord {
        record(
            seq,
            EventKind::ModelReturned,
            serde_json::json!({
                "message": {
                    "role": "assistant",
                    "content": [
                        { "kind": "thinking", "thinking": "weighing it", "signature": "sig" },
                        { "kind": "text", "text": text }
                    ]
                },
                "calls": 0,
                "usage": {
                    "input_tokens": 1200,
                    "output_tokens": 340,
                    "cache_read_tokens": 800,
                    "cache_write_tokens": 0
                },
                "stop": "end_turn",
                "billed_usd_micros": 3340
            }),
        )
    }

    #[test]
    fn a_turn_says_what_the_model_said_and_what_it_cost() {
        let folded = turns(&[asked(1), answered_model(2, "I read the lexer.")]);
        assert_eq!(folded[0].said.as_deref(), Some("I read the lexer."));
        assert_eq!(folded[0].spent, Some(UsdMicros::new(3340)));
        assert_eq!(folded[0].stopped.as_deref(), Some("end_turn"));
        let used = folded[0].used.expect("usage is on the wire");
        assert_eq!(used.input, Tokens::new(1200));
        assert_eq!(used.output, Tokens::new(340));
        assert_eq!(used.cached, Tokens::new(800));
    }

    /// Thinking blocks are carried end to end so the provider can verify
    /// their signature. Rendering them would be this page publishing
    /// something it was only ever asked to relay.
    #[test]
    fn what_the_model_was_thinking_is_relayed_not_rendered() {
        let folded = turns(&[asked(1), answered_model(2, "done")]);
        let said = folded[0].said.clone().unwrap_or_default();
        assert!(!said.contains("weighing it"), "{said}");
    }

    /// A client one version behind must still show the turn.
    #[test]
    fn a_model_return_this_build_cannot_read_still_leaves_the_turn_standing() {
        let odd = record(
            2,
            EventKind::ModelReturned,
            serde_json::json!({ "shape": 3 }),
        );
        let folded = turns(&[asked(1), odd]);
        assert_eq!(folded.len(), 1);
        assert_eq!(folded[0].said, None);
        assert_eq!(folded[0].spent, None);
        assert_eq!(folded[0].used, None);
    }

    #[test]
    fn a_call_that_answered_says_what_it_said() {
        let events = [
            asked(1),
            called(2, "a", "exec", "cargo test"),
            record(
                3,
                EventKind::ToolResult,
                serde_json::json!({ "tool_use_id": "a", "name": "exec",
                                    "result": "test result: ok. 1162 passed" }),
            ),
        ];
        let folded = turns(&events);
        let output = folded[0].calls[0]
            .output
            .clone()
            .expect("an answered call says what it said");
        assert!(output.head.contains("1162 passed"), "{}", output.head);
        assert_eq!(output.cut, 0);
    }

    /// The disclosure has a bound, and the bound reports itself. A wave of
    /// output that silently became the page is the dump section 8-47
    /// refuses.
    #[test]
    fn a_long_output_is_cut_and_says_how_much_it_cut() {
        let long: String = (0..40)
            .map(|n| format!("line {n}\n"))
            .collect::<Vec<String>>()
            .join("");
        let events = [
            asked(1),
            called(2, "a", "exec", "cargo build"),
            record(
                3,
                EventKind::ToolResult,
                serde_json::json!({ "tool_use_id": "a", "name": "exec", "result": long }),
            ),
        ];
        let output = turns(&events)[0].calls[0]
            .output
            .clone()
            .expect("an answered call says what it said");
        assert_eq!(output.head.lines().count(), OUTPUT_LINES);
        assert_eq!(output.cut, 40 - OUTPUT_LINES);
    }

    /// The headline case: a three-part refusal used to render as one grey
    /// line indistinguishable from a successful read.
    #[test]
    fn a_door_that_refused_lands_in_the_turn_whole() {
        let refusal = AxError::failure(
            AxCode::OutsideWriteDomain,
            "write",
            "crates/kernel/src/gate.rs",
        )
        .with_recovery("write under lab/ instead");
        let payload = serde_json::to_value(&refusal).expect("an error serialises");
        let events = [asked(1), record(2, EventKind::GateDenied, payload)];
        let folded = turns(&events);
        match folded[0].notes.first() {
            Some(Note::Refused { error, at }) => {
                assert_eq!(*error, refusal, "the error travels whole");
                assert_eq!(*at, Seq::new(2));
            }
            other => panic!("a refusal is a note on the turn, got {other:?}"),
        }
    }

    #[test]
    fn a_checkpoint_inside_a_turn_names_the_commit_it_made() {
        let spelled = "3f9a1c00112233445566778899aabbccddeeff00";
        let events = [
            asked(1),
            record(
                2,
                EventKind::CheckpointCommitted,
                serde_json::json!({ "oid": spelled, "scope": "lab", "files": [] }),
            ),
        ];
        assert_eq!(
            turns(&events)[0].notes,
            vec![Note::Fenced {
                oid: GitOid::parse(spelled).expect("forty hex digits"),
                at: Seq::new(2)
            }]
        );
    }

    /// The oid is what a change list is addressed by, so a spelling this
    /// build cannot parse leaves no row: a checkpoint nothing can be
    /// asked about is worse than a checkpoint that is not shown, because
    /// the first one looks like a working control.
    #[test]
    fn a_checkpoint_whose_oid_will_not_parse_leaves_no_row_to_click() {
        let events = [
            asked(1),
            record(
                2,
                EventKind::CheckpointCommitted,
                serde_json::json!({ "oid": "3f9a1c", "scope": "lab", "files": [] }),
            ),
        ];
        assert!(turns(&events)[0].notes.is_empty());
    }

    #[test]
    fn a_word_from_a_person_lands_in_the_turn_it_reached() {
        let events = [
            asked(1),
            record(
                2,
                EventKind::SteerReceived,
                serde_json::json!({ "source": "user", "said": "ignore the cache" }),
            ),
        ];
        // `runtime::turn` writes `text`, not `said`: the fold reads the
        // field the producer writes, and an unknown shape still leaves a
        // note rather than dropping the fact that somebody spoke.
        match turns(&events)[0].notes.first() {
            Some(Note::Arrived { from, at, .. }) => {
                assert_eq!(from, "user");
                assert_eq!(*at, Seq::new(2));
            }
            other => panic!("a steer is a note on the turn, got {other:?}"),
        }
    }

    #[test]
    fn a_turn_waiting_on_a_person_says_so_without_copying_the_queue() {
        let events = [
            asked(1),
            record(2, EventKind::ApprovalRequested, serde_json::json!({})),
        ];
        assert_eq!(
            turns(&events)[0].notes,
            vec![Note::Waiting { at: Seq::new(2) }]
        );
    }

    /// A note is only a note when it belongs to a turn. The session's own
    /// opening is not a round somebody took.
    #[test]
    fn a_note_before_the_first_turn_belongs_to_no_turn() {
        let events = [
            record(
                1,
                EventKind::CheckpointCommitted,
                serde_json::json!({ "oid": "8c22de", "scope": "lab", "files": [] }),
            ),
            asked(2),
        ];
        let folded = turns(&events);
        assert_eq!(folded.len(), 1);
        assert!(folded[0].notes.is_empty());
    }

    /// The event stream keeps its own shape: a kind that changed neither
    /// what the turn did nor what it waits on is not a note.
    #[test]
    fn an_event_that_changed_nothing_about_this_turn_is_not_a_note() {
        let events = [
            asked(1),
            record(2, EventKind::GateChecked, serde_json::json!({})),
            record(3, EventKind::PromptAssembled, serde_json::json!({})),
        ];
        assert!(turns(&events)[0].notes.is_empty());
    }
}
