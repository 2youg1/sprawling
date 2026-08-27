// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! The session: which frames a conversation with a browser is made of,
//! and the recording that plays one back.
//!
//! `Session` mints ids and builds frames; it never holds a socket. That
//! is what lets a whole browser conversation be asserted without a
//! browser, and it is why the recording below is a real second adapter
//! rather than a test double — the binary's transport and this one
//! differ in where the bytes come from and in nothing else.

use std::collections::BTreeMap;

use kernel::{AxCode, AxError};
use serde_json::{Value, json};

use crate::port::{BrowserPort, Frame, Reply};

/// A browsing context — a tab, a frame, a popup. Opaque on purpose: the
/// string is the remote end's, and inventing one has no meaning.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ContextId(String);

impl ContextId {
    /// # Errors
    /// Refuses an empty id, which is the shape a missing field takes
    /// after a partial parse.
    pub fn parse(raw: &str) -> Result<ContextId, AxError> {
        if raw.is_empty() {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "read a browsing context",
                "empty id",
            )
            .with_recovery("take the id from `browsingContext.getTree`"));
        }
        Ok(ContextId(raw.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Which capabilities a session asks for. Kept minimal deliberately:
/// every capability is a thing the remote end may then do, and a session
/// that asked for less is a session that can do less.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct SessionRequest {
    /// Whether the session subscribes to network events. Off by default:
    /// a subscription that nobody reads is bytes crossing a boundary for
    /// no reason.
    pub network: bool,
}

/// Builds the frames of one conversation, in order, with ids nobody else
/// mints.
#[derive(Debug, Default)]
pub struct Session {
    next: u64,
}

impl Session {
    #[must_use]
    pub fn new() -> Session {
        Session { next: 0 }
    }

    fn mint(&mut self) -> u64 {
        self.next = self.next.saturating_add(1);
        self.next
    }

    /// `session.new`: the first frame of every conversation.
    ///
    /// # Errors
    /// Propagates the frame's own refusal, which the literals below
    /// cannot provoke.
    pub fn begin(&mut self, request: SessionRequest) -> Result<Frame, AxError> {
        let mut events = Vec::new();
        if request.network {
            events.push(Value::String("network".to_owned()));
        }
        Frame::new(
            self.mint(),
            "session.new",
            json!({ "capabilities": {}, "events": Value::Array(events) }),
        )
    }

    /// `browsingContext.getTree`: which tabs exist.
    ///
    /// # Errors
    /// Propagates the frame's own refusal.
    pub fn tree(&mut self) -> Result<Frame, AxError> {
        Frame::new(self.mint(), "browsingContext.getTree", json!({}))
    }

    /// `browsingContext.navigate`, waiting for the document to be
    /// complete. Anything earlier hands a model a page that is still
    /// arriving, and the difference is invisible in the text.
    ///
    /// # Errors
    /// Refuses an empty url, and propagates the frame's own refusal.
    pub fn navigate(&mut self, context: &ContextId, url: &str) -> Result<Frame, AxError> {
        if url.is_empty() {
            return Err(
                AxError::failure(AxCode::InvalidArgs, "navigate", "empty url")
                    .with_recovery("pass an absolute url"),
            );
        }
        Frame::new(
            self.mint(),
            "browsingContext.navigate",
            json!({ "context": context.as_str(), "url": url, "wait": "complete" }),
        )
    }

    /// `script.evaluate` in the page's own realm, awaiting promises.
    ///
    /// # Errors
    /// Propagates the frame's own refusal.
    pub fn evaluate(&mut self, context: &ContextId, expression: &str) -> Result<Frame, AxError> {
        Frame::new(
            self.mint(),
            "script.evaluate",
            json!({
                "expression": expression,
                "target": { "context": context.as_str() },
                "awaitPromise": true,
            }),
        )
    }

    /// `session.end`.
    ///
    /// # Errors
    /// Propagates the frame's own refusal.
    pub fn end(&mut self) -> Result<Frame, AxError> {
        Frame::new(self.mint(), "session.end", json!({}))
    }

    /// Reads `browsingContext.getTree`'s answer.
    ///
    /// # Errors
    /// Refuses a result whose shape this version does not read.
    pub fn read_tree(result: &Value) -> Result<Vec<ContextId>, AxError> {
        let contexts = result
            .get("contexts")
            .and_then(Value::as_array)
            .ok_or_else(|| {
                AxError::failure(
                    AxCode::WireMismatch,
                    "read the context tree",
                    "no `contexts` array",
                )
                .with_recovery("check the driver's protocol version")
            })?;
        let mut out = Vec::new();
        for entry in contexts {
            let raw = entry
                .get("context")
                .and_then(Value::as_str)
                .ok_or_else(|| {
                    AxError::failure(
                        AxCode::WireMismatch,
                        "read the context tree",
                        "a context without an id",
                    )
                    .with_recovery("check the driver's protocol version")
                })?;
            out.push(ContextId::parse(raw)?);
        }
        Ok(out)
    }
}

/// The second adapter: a conversation somebody already had.
///
/// It answers by method and by the frame's own bytes rather than by
/// arrival order, so a caller that legitimately reorders two independent
/// commands still replays, while a caller that changed what it asks
/// fails loudly instead of receiving somebody else's answer.
#[derive(Debug, Default)]
pub struct Recording {
    answers: BTreeMap<String, Value>,
    /// Frames that arrived with no recorded answer, for the assertion
    /// that a replay covered the conversation it claims to.
    missed: Vec<String>,
}

impl Recording {
    #[must_use]
    pub fn new() -> Recording {
        Recording::default()
    }

    /// Records what this frame's method-and-params were answered with.
    /// The id is deliberately excluded from the key: a replay mints its
    /// own ids and they are not part of what was asked.
    pub fn answer(&mut self, frame: &Frame, result: Value) {
        self.answers.insert(key_of(frame), result);
    }

    /// Frames this recording could not answer, in arrival order.
    #[must_use]
    pub fn missed(&self) -> &[String] {
        &self.missed
    }
}

fn key_of(frame: &Frame) -> String {
    format!("{} {}", frame.method(), frame.params())
}

impl BrowserPort for Recording {
    fn send(&mut self, frame: &Frame) -> Result<Reply, AxError> {
        match self.answers.get(&key_of(frame)) {
            Some(result) => Ok(Reply::Success {
                id: frame.id(),
                result: result.clone(),
            }),
            None => {
                self.missed.push(key_of(frame));
                Err(AxError::failure(
                    AxCode::BrowserUnavailable,
                    "replay a browser command",
                    key_of(frame),
                )
                .with_recovery(
                    "record this command against a real browser before replaying it offline",
                ))
            }
        }
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
    use crate::port::assert_port_conformance;

    #[test]
    fn a_conversation_numbers_its_own_frames_and_never_reuses_one() {
        let mut session = Session::new();
        let begin = session.begin(SessionRequest::default()).unwrap();
        let tree = session.tree().unwrap();
        let end = session.end().unwrap();
        assert_eq!((begin.id(), tree.id(), end.id()), (1, 2, 3));
        assert_eq!(begin.method(), "session.new");
    }

    #[test]
    fn a_session_that_wants_nothing_subscribes_to_nothing() {
        let mut session = Session::new();
        let quiet = session.begin(SessionRequest::default()).unwrap();
        assert_eq!(quiet.params().get("events"), Some(&json!([])));
        let loud = session.begin(SessionRequest { network: true }).unwrap();
        assert_eq!(loud.params().get("events"), Some(&json!(["network"])));
    }

    #[test]
    fn navigation_waits_for_the_document_rather_than_the_request() {
        let mut session = Session::new();
        let context = ContextId::parse("c1").unwrap();
        let frame = session.navigate(&context, "https://example.test/").unwrap();
        assert_eq!(
            frame.params().get("wait").and_then(Value::as_str),
            Some("complete"),
            "a page still arriving reads the same as one that finished"
        );
        assert!(session.navigate(&context, "").is_err());
    }

    #[test]
    fn the_recording_answers_what_was_asked_and_says_what_it_cannot() {
        let mut session = Session::new();
        let tree = session.tree().unwrap();
        let mut recording = Recording::new();
        recording.answer(&tree, json!({ "contexts": [{ "context": "c1" }] }));

        let mut replay = Session::new();
        let asked = replay.tree().unwrap();
        let result = recording.send(&asked).unwrap().into_result().unwrap();
        assert_eq!(
            Session::read_tree(&result).unwrap(),
            vec![ContextId::parse("c1").unwrap()]
        );

        let unknown = replay
            .navigate(&ContextId::parse("c1").unwrap(), "https://elsewhere.test/")
            .unwrap();
        let err = recording.send(&unknown).unwrap_err();
        assert_eq!(err.code(), &AxCode::BrowserUnavailable);
        assert_eq!(recording.missed().len(), 1);
    }

    #[test]
    fn the_recording_satisfies_the_seams_assertions() {
        let mut session = Session::new();
        let tree = session.tree().unwrap();
        let mut recording = Recording::new();
        recording.answer(&tree, json!({ "contexts": [] }));
        assert_port_conformance(&mut recording, &tree);
    }

    #[test]
    fn a_tree_this_version_cannot_read_is_refused_rather_than_returned_empty() {
        assert!(Session::read_tree(&json!({})).is_err());
        assert!(Session::read_tree(&json!({ "contexts": [{}] })).is_err());
        assert_eq!(
            Session::read_tree(&json!({ "contexts": [] })).unwrap(),
            Vec::new()
        );
    }
}
