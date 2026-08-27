// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! What a model said, on its way into history.
//!
//! Everything a provider sends back becomes a ledger payload verbatim,
//! and a model that read a key can repeat one. The ledger is permanent
//! and exportable, so it is the sink that matters: a secret there
//! outlives the session, the machine and the person's memory of it.
//!
//! Two decisions carry this module.
//!
//! **The window keeps the bytes; the ledger does not.** The blocks the
//! next request is built from are extracted before this runs, so
//! redacting here cannot break a thinking block's signature or make the
//! conversation stop making sense. History and context are two sinks
//! with two different requirements, and only one of them is forever.
//!
//! **The replacement is a marker, not a vault entry.** A key the model
//! echoed is not a credential the city was asked to keep; storing it
//! would give it a life nobody asked for, and inventing a realm for it
//! would put a made-up name in the vault's namespace. The marker carries
//! the first sixteen hex of the content hash instead, which is enough to
//! see that two occurrences are the same value and never enough to be
//! one.

use kernel::B3Hash;
use serde_json::{Map, Value};

/// The realm every redaction marker sits in. Not a vault realm: nothing
/// is stored under it, and `secret:` references resolve through the
/// vault, so a reader who tries will be told there is nothing there.
const REDACTED_REALM: &str = "redacted";

/// Replaces every secret-shaped span in every string of `payload`.
///
/// Returns the payload and how many spans were replaced, because a count
/// is what a diagnostic line can say without saying what it found.
#[must_use]
pub fn redact(payload: &Map<String, Value>) -> (Map<String, Value>, u32) {
    let mut hits = 0;
    let mut out = Map::new();
    for (key, value) in payload {
        out.insert(key.clone(), walk(value, &mut hits));
    }
    (out, hits)
}

fn walk(value: &Value, hits: &mut u32) -> Value {
    match value {
        Value::String(text) => {
            let (replaced, found) = redact_text(text);
            *hits = hits.saturating_add(found);
            Value::String(replaced)
        }
        Value::Array(items) => Value::Array(items.iter().map(|item| walk(item, hits)).collect()),
        Value::Object(map) => {
            let mut out = Map::new();
            for (key, item) in map {
                out.insert(key.clone(), walk(item, hits));
            }
            Value::Object(out)
        }
        other => other.clone(),
    }
}

/// One string, with every secret shape in it replaced in place.
///
/// The spans arrive sorted and non-overlapping from the detector, so a
/// single left-to-right pass is enough and no offset needs adjusting
/// after a replacement.
#[must_use]
pub fn redact_text(text: &str) -> (String, u32) {
    let bytes = text.as_bytes();
    let spans = kernel::scan(bytes);
    if spans.is_empty() {
        return (text.to_owned(), 0);
    }
    let mut out = String::with_capacity(text.len());
    let mut at = 0usize;
    let mut hits: u32 = 0;
    for span in spans {
        let end = span.start.saturating_add(span.len);
        if span.start < at || end > bytes.len() {
            continue;
        }
        let Some(before) = bytes
            .get(at..span.start)
            .and_then(|b| std::str::from_utf8(b).ok())
        else {
            continue;
        };
        let Some(found) = bytes.get(span.start..end) else {
            continue;
        };
        out.push_str(before);
        out.push_str(&marker(found));
        at = end;
        hits = hits.saturating_add(1);
    }
    // A span that ended inside a character would leave the tail
    // unreadable; the honest answer is to keep nothing of it rather than
    // emit half a character.
    if let Some(rest) = bytes.get(at..).and_then(|b| std::str::from_utf8(b).ok()) {
        out.push_str(rest);
    }
    (out, hits)
}

fn marker(found: &[u8]) -> String {
    let digest = B3Hash::digest(found).to_string();
    let short = digest.get(..16).unwrap_or(&digest);
    format!("secret:{REDACTED_REALM}/{short}")
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

    /// Assembled at runtime: a literal key-shaped sample in the source
    /// would be a finding about this file.
    fn key_shaped() -> String {
        ["sk", "9fQ2xZ", "7Lm4Rt", "0Bv8Kd", "3Wp6"].join("")
    }

    #[test]
    fn a_key_shaped_run_of_bytes_does_not_reach_the_payload() {
        let key = key_shaped();
        let mut payload = Map::new();
        payload.insert(
            "message".to_owned(),
            Value::String(format!("the token is {key} and it works")),
        );
        let (redacted, hits) = redact(&payload);
        assert_eq!(hits, 1);
        let text = redacted
            .get("message")
            .and_then(Value::as_str)
            .unwrap()
            .to_owned();
        assert!(!text.contains(&key), "the value itself is gone");
        assert!(text.contains("the token is "), "the sentence survives");
        assert!(text.contains("secret:redacted/"));
    }

    #[test]
    fn the_same_value_twice_marks_the_same_way_and_two_values_do_not() {
        let key = key_shaped();
        let other = ["sk", "2Tg7Yh", "4Nn1Qs", "8Cz5Ud", "6Ke0"].join("");
        let (first, _) = redact_text(&key);
        let (again, _) = redact_text(&key);
        let (different, _) = redact_text(&other);
        assert_eq!(first, again, "one value, one marker");
        assert_ne!(first, different, "two values are distinguishable");
    }

    #[test]
    fn nested_strings_are_reached_and_ordinary_prose_is_untouched() {
        let key = key_shaped();
        let mut inner = Map::new();
        inner.insert("text".to_owned(), Value::String(key.clone()));
        let mut payload = Map::new();
        payload.insert(
            "content".to_owned(),
            Value::Array(vec![Value::Object(inner)]),
        );
        payload.insert(
            "prose".to_owned(),
            Value::String("nothing here looks like a key at all".to_owned()),
        );
        payload.insert("count".to_owned(), Value::Number(3.into()));
        let (redacted, hits) = redact(&payload);
        assert_eq!(hits, 1);
        assert_eq!(
            redacted.get("prose").and_then(Value::as_str),
            Some("nothing here looks like a key at all"),
            "prose is not a shape, and a scanner that trimmed it would be unusable"
        );
        assert_eq!(redacted.get("count").and_then(Value::as_u64), Some(3));
        let text = serde_json::to_string(&redacted).unwrap();
        assert!(!text.contains(&key));
    }

    #[test]
    fn a_payload_with_nothing_to_hide_comes_back_unchanged() {
        let mut payload = Map::new();
        payload.insert(
            "message".to_owned(),
            Value::String("measured in metres".to_owned()),
        );
        let (redacted, hits) = redact(&payload);
        assert_eq!(hits, 0);
        assert_eq!(redacted, payload);
    }
}
