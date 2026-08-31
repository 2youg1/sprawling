// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The diagnostic log: write-only, five levels, three required fields.
//! The design this implements is `docs/logging.md`.
//!
//! **Decision and recovery logic reads no log output.** That rule is held
//! by shape rather than by discipline: this surface has write methods and
//! no read method, so reading a line back inside the code cannot be
//! spelled. The consequence is the one the design cares about — deleting
//! every log leaves behaviour, replay and totals byte-identical, because
//! nothing downstream could have depended on them.
//!
//! A line carries `seq` and never a timestamp. The Ledger position is
//! what anchors a log line to the only history, and it is an integer
//! two timelines can be lined up on; a wall clock sampled here would
//! also be a second sampling point in a library that is not allowed one.
//! A sink that wants a clock is free to add one — the assembly layer is
//! where sampling is sanctioned.

use kernel::{RunId, Seq};

/// Who reads a line, and when. Ordered from the widest audience to the
/// narrowest: a floor admits every level at or before it.
///
/// The names answer "who reads this" rather than "how bad is this",
/// because that is the question with a checkable answer. A severity
/// ladder would invite the other one.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Level {
    /// The person, live: what a door refused, in three parts.
    Refuse,
    /// The person, afterwards: which file was written, which provider
    /// was called.
    Effect,
    /// The builder, when behaviour is wrong: why a verdict took the
    /// value it did.
    Decide,
    /// The builder, when reproducing a defect: phase changes, locks,
    /// retries.
    Trace,
    /// The builder, when a protocol does not connect: the bytes.
    Wire,
}

impl Level {
    /// The default floor. `refuse` and `effect` together answer nine of
    /// ten questions a person has, and are small enough to leave on.
    pub const DEFAULT: Level = Level::Effect;

    /// Every level, in declaration order, for a command line to offer.
    pub const ALL: [Level; 5] = [
        Level::Refuse,
        Level::Effect,
        Level::Decide,
        Level::Trace,
        Level::Wire,
    ];

    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            Level::Refuse => "refuse",
            Level::Effect => "effect",
            Level::Decide => "decide",
            Level::Trace => "trace",
            Level::Wire => "wire",
        }
    }

    /// Reads a level by name, for `--log <level>`. `off` is not a level
    /// here: turning logging off is [`Diagnostics::off`], and letting
    /// one word mean both a floor and the absence of one is how a
    /// setting becomes ambiguous.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Level> {
        Level::ALL.into_iter().find(|level| level.as_str() == raw)
    }
}

impl std::fmt::Display for Level {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The three fields every line carries. Required rather than optional:
/// a line that cannot be placed in the history is a line nobody can act
/// on later.
#[derive(Debug, Clone, Copy)]
pub struct Site<'a> {
    pub run: RunId,
    /// The Ledger position at the time. The load-bearing field.
    pub seq: Seq,
    pub module: &'a str,
}

/// Where rendered lines go. A closure rather than a trait: this crate
/// opens no files, and one implementation is not a seam.
pub type Sink = Box<dyn FnMut(&str) + Send>;

/// The write-only logging surface.
///
/// Holding one is the only way to write a log line in this crate, and it
/// offers no way to read one back.
pub struct Diagnostics {
    floor: Option<Level>,
    sink: Sink,
}

impl std::fmt::Debug for Diagnostics {
    /// The sink is not printable, and the floor is the only state worth
    /// seeing. Manual rather than derived so a future field does not
    /// quietly acquire a `Debug` that prints what it logged.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Diagnostics")
            .field("floor", &self.floor)
            .finish_non_exhaustive()
    }
}

impl Diagnostics {
    /// Writes every level at or before `floor` to `sink`.
    #[must_use]
    pub fn new(floor: Level, sink: Sink) -> Diagnostics {
        Diagnostics {
            floor: Some(floor),
            sink,
        }
    }

    /// Writes nothing at all. The state the deletion-invariance test
    /// runs in, and a legitimate way to run the city.
    #[must_use]
    pub fn off() -> Diagnostics {
        Diagnostics {
            floor: None,
            sink: Box::new(|_line: &str| {}),
        }
    }

    /// The current floor, for `status` to report. `None` is off.
    #[must_use]
    pub fn floor(&self) -> Option<Level> {
        self.floor
    }

    /// Whether a level would be written. Offered so a caller can skip
    /// building an expensive message, not so it can branch on it.
    #[must_use]
    pub fn admits(&self, level: Level) -> bool {
        self.floor.is_some_and(|floor| level <= floor)
    }

    /// Writes one line, if the floor admits it.
    ///
    /// The message passes the same secret scan the Ledger uses — the
    /// same function, not a second one, because the scanner nobody
    /// watches is the one that misses something. A hit is replaced in
    /// place: dropping the line would lose the diagnostic entirely, and
    /// the surrounding words are usually what the reader needed.
    pub fn write(&mut self, level: Level, site: Site<'_>, message: &str) {
        if !self.admits(level) {
            return;
        }
        let line = render(level, site, &redact(message));
        (self.sink)(&line);
    }
}

/// Replaces every secret-shaped span with a marker.
#[must_use]
pub fn redact(message: &str) -> String {
    let hits = kernel::scan(message.as_bytes());
    if hits.is_empty() {
        return message.to_owned();
    }
    let bytes = message.as_bytes();
    let mut out = String::with_capacity(message.len());
    let mut at = 0usize;
    for hit in hits {
        // Overlapping or out-of-order spans cannot make this index
        // backwards; a span that starts before what is already copied is
        // skipped rather than rewinding the cursor.
        let Some(end) = hit.start.checked_add(hit.len) else {
            continue;
        };
        if hit.start < at || end > bytes.len() {
            continue;
        }
        if let Some(before) = bytes.get(at..hit.start) {
            out.push_str(&String::from_utf8_lossy(before));
        }
        out.push_str(REDACTED);
        at = end;
    }
    if let Some(rest) = bytes.get(at..) {
        out.push_str(&String::from_utf8_lossy(rest));
    }
    out
}

/// What a redacted span reads as. A reference-shaped marker, so a reader
/// who finds one knows both that something was there and that the way to
/// use it is a reference.
pub const REDACTED: &str = "secret:redacted";

/// One JSON object per line, for the same reason the wire format is:
/// the receiver may be a browser, and a person can still read it.
fn render(level: Level, site: Site<'_>, message: &str) -> String {
    let mut map = serde_json::Map::new();
    map.insert(
        "level".to_owned(),
        serde_json::Value::String(level.as_str().to_owned()),
    );
    map.insert(
        "run".to_owned(),
        serde_json::Value::String(site.run.to_string()),
    );
    map.insert(
        "seq".to_owned(),
        serde_json::Value::Number(site.seq.value().into()),
    );
    map.insert(
        "module".to_owned(),
        serde_json::Value::String(site.module.to_owned()),
    );
    map.insert(
        "message".to_owned(),
        serde_json::Value::String(message.to_owned()),
    );
    serde_json::Value::Object(map).to_string()
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
    use std::sync::{Arc, Mutex};

    fn recorder() -> (Diagnostics, Arc<Mutex<Vec<String>>>) {
        at_floor(Level::DEFAULT)
    }

    fn at_floor(floor: Level) -> (Diagnostics, Arc<Mutex<Vec<String>>>) {
        let held = Arc::new(Mutex::new(Vec::new()));
        let sink = Arc::clone(&held);
        let log = Diagnostics::new(
            floor,
            Box::new(move |line: &str| {
                sink.lock().unwrap().push(line.to_owned());
            }),
        );
        (log, held)
    }

    fn site() -> Site<'static> {
        Site {
            run: RunId::CITY,
            seq: Seq::FIRST,
            module: "runtime::turn",
        }
    }

    #[test]
    fn the_default_floor_carries_the_two_levels_a_person_reads() {
        let (mut log, held) = recorder();
        for level in Level::ALL {
            log.write(level, site(), "something happened");
        }
        let lines = held.lock().unwrap();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].contains("\"level\":\"refuse\""));
        assert!(lines[1].contains("\"level\":\"effect\""));
    }

    #[test]
    fn every_line_can_be_placed_in_the_history() {
        let (mut log, held) = recorder();
        log.write(Level::Effect, site(), "wrote notes.md");
        let lines = held.lock().unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&lines[0]).unwrap();
        for field in ["run", "seq", "module"] {
            assert!(!parsed[field].is_null(), "{field} is required");
        }
        assert_eq!(parsed["seq"], Seq::FIRST.value());
        assert_eq!(parsed["module"], "runtime::turn");
        // No timestamp: the position in the ledger is the anchor, and a
        // clock sampled here would be a second time source.
        assert!(parsed.get("t").is_none());
        assert!(parsed.get("timestamp").is_none());
    }

    #[test]
    fn a_credential_that_reaches_a_line_does_not_leave_in_it() {
        // Assembled at runtime, for the same reason the gate wants: no
        // credential-shaped literal at rest in the repository.
        let token = ["sk-live-", "Zk29fQ4t", "Rr7mVx1L", "pA6c"].concat();
        let (mut log, held) = recorder();
        log.write(
            Level::Effect,
            site(),
            &format!("calling the provider with {token}"),
        );
        let lines = held.lock().unwrap();
        assert!(!lines[0].contains(&token), "line: {}", lines[0]);
        assert!(lines[0].contains(REDACTED));
        // The words around it survive: a redaction that ate the sentence
        // would cost the reader the thing they came for.
        assert!(lines[0].contains("calling the provider with"));
    }

    #[test]
    fn a_message_with_nothing_to_hide_is_unchanged() {
        assert_eq!(redact("wrote notes.md"), "wrote notes.md");
        assert_eq!(redact(""), "");
    }

    #[test]
    fn logging_off_writes_nothing_at_any_level() {
        let mut log = Diagnostics::off();
        assert_eq!(log.floor(), None);
        for level in Level::ALL {
            assert!(!log.admits(level));
            // Reaches the sink or not is the whole question; `off` holds
            // a sink that could not report either way, which is the
            // point — nothing downstream can depend on a log.
            log.write(level, site(), "ignored");
        }
    }

    #[test]
    fn a_floor_admits_everything_at_or_before_it() {
        let (mut log, held) = at_floor(Level::Wire);
        for level in Level::ALL {
            log.write(level, site(), "x");
        }
        assert_eq!(held.lock().unwrap().len(), Level::ALL.len());
        assert!(Level::Refuse < Level::Effect);
        assert!(Level::Effect < Level::Wire);
    }

    #[test]
    fn a_level_is_named_the_same_way_on_the_command_line_and_in_a_line() {
        for level in Level::ALL {
            assert_eq!(Level::parse(level.as_str()), Some(level));
        }
        assert_eq!(Level::parse("verbose"), None);
        assert_eq!(Level::parse("off"), None);
    }
}
