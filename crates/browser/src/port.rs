// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! The browser seam.
//!
//! The port carries frames, not intentions. Everything above it —
//! which command a snapshot needs, what an action turns into, when a
//! development loop should look again — is decided by pure code in this
//! crate; everything below it is one socket somebody else owns.
//!
//! That line is where the testability comes from. A WebDriver session
//! is a long-lived bidirectional connection, and a crate that owned one
//! would need an async runtime to be tested at all. Here the adapters
//! are: a real transport in the binary, and a recording in the test
//! suite, and the two are held to the same assertions.

use kernel::{AxCode, AxError};
use serde_json::Value;

/// One request on the wire, already shaped as the remote end expects.
///
/// The id is minted by the caller rather than the transport, because a
/// recorded session has to replay to the same bytes and a transport
/// that numbered its own frames would renumber them on the way back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Frame {
    id: u64,
    method: String,
    params: Value,
}

impl Frame {
    /// # Errors
    /// Refuses an empty method and params that are not an object: both
    /// are refused by the remote end anyway, and refusing here names the
    /// caller instead of the wire.
    pub fn new(id: u64, method: &str, params: Value) -> Result<Frame, AxError> {
        if method.is_empty() {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "build a browser frame",
                "empty method",
            )
            .with_recovery("name a module command, for example `browsingContext.navigate`"));
        }
        if !params.is_object() {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "build a browser frame",
                format!("{method}: params must be an object"),
            )
            .with_recovery("pass an object, using `{}` when the command takes nothing"));
        }
        Ok(Frame {
            id,
            method: method.to_owned(),
            params,
        })
    }

    #[must_use]
    pub fn id(&self) -> u64 {
        self.id
    }

    #[must_use]
    pub fn method(&self) -> &str {
        &self.method
    }

    #[must_use]
    pub fn params(&self) -> &Value {
        &self.params
    }

    /// The bytes that go out. Field order is fixed here rather than left
    /// to a map's iteration order, so a recorded session compares byte
    /// for byte on every platform.
    #[must_use]
    pub fn to_wire(&self) -> String {
        let mut out = String::new();
        out.push_str("{\"id\":");
        out.push_str(&self.id.to_string());
        out.push_str(",\"method\":");
        out.push_str(&Value::String(self.method.clone()).to_string());
        out.push_str(",\"params\":");
        out.push_str(&self.params.to_string());
        out.push('}');
        out
    }
}

/// What came back for one frame. Errors from the remote end are values,
/// not transport failures: "this page has no such element" is an answer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Reply {
    Success {
        id: u64,
        result: Value,
    },
    Error {
        id: u64,
        code: String,
        message: String,
    },
}

impl Reply {
    /// Reads one line from the remote end.
    ///
    /// # Errors
    /// Refuses anything that is not a JSON object carrying `id` and a
    /// recognised `type`. A reply this version cannot read is a version
    /// mismatch, and guessing at it would put invented text in front of
    /// a model.
    pub fn parse(line: &str) -> Result<Reply, AxError> {
        let value: Value = serde_json::from_str(line).map_err(|err| {
            AxError::failure(
                AxCode::WireMismatch,
                "read a browser reply",
                err.to_string(),
            )
            .with_recovery("check the driver's protocol version")
        })?;
        let object = value.as_object().ok_or_else(|| {
            AxError::failure(
                AxCode::WireMismatch,
                "read a browser reply",
                "not an object",
            )
            .with_recovery("check the driver's protocol version")
        })?;
        let id = object.get("id").and_then(Value::as_u64).ok_or_else(|| {
            AxError::failure(AxCode::WireMismatch, "read a browser reply", "no id")
                .with_recovery("events carry no id; route them before parsing replies")
        })?;
        match object.get("type").and_then(Value::as_str) {
            Some("success") => Ok(Reply::Success {
                id,
                result: object.get("result").cloned().unwrap_or(Value::Null),
            }),
            Some("error") => Ok(Reply::Error {
                id,
                code: object
                    .get("error")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown error")
                    .to_owned(),
                message: object
                    .get("message")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
            }),
            other => Err(AxError::failure(
                AxCode::WireMismatch,
                "read a browser reply",
                other.unwrap_or("no type").to_owned(),
            )
            .with_recovery("this version reads `success` and `error`")),
        }
    }

    #[must_use]
    pub fn id(&self) -> u64 {
        match self {
            Reply::Success { id, .. } | Reply::Error { id, .. } => *id,
        }
    }

    /// The result, or the remote end's refusal turned into ours.
    ///
    /// # Errors
    /// Carries the driver's own code and message through, because the
    /// caller can act on "no such node" and cannot act on "browser said
    /// no".
    pub fn into_result(self) -> Result<Value, AxError> {
        match self {
            Reply::Success { result, .. } => Ok(result),
            Reply::Error { code, message, .. } => Err(AxError::failure(
                AxCode::BrowserUnavailable,
                "run a browser command",
                format!("{code}: {message}"),
            )
            .with_recovery("take a fresh snapshot; the page may have moved under the reference")),
        }
    }
}

/// The seam. One method, because a browser session is a request and a
/// reply however elaborate the thing on the other side is.
///
/// Adapters: the WebDriver transport in the binary, and the recording in
/// [`crate::session`]. Both are held to
/// [`assert_port_conformance`](crate::port::assert_port_conformance).
pub trait BrowserPort {
    /// # Errors
    /// Transport failures only. A refusal from the remote end arrives as
    /// [`Reply::Error`], which is an answer rather than a fault.
    fn send(&mut self, frame: &Frame) -> Result<Reply, AxError>;
}

/// One assertion suite for every implementation of the seam.
///
/// # Panics
/// On any implementation that renumbers frames, invents a reply for a
/// frame nobody sent, or answers after it has been closed.
#[allow(
    clippy::panic,
    clippy::expect_used,
    reason = "conformance suites assert by panicking; dev-only by contract"
)]
pub fn assert_port_conformance<P: BrowserPort>(port: &mut P, known: &Frame) {
    let reply = port.send(known).expect("a known frame is answered");
    assert_eq!(
        reply.id(),
        known.id(),
        "a reply belongs to the frame that asked for it; renumbering loses that"
    );
    let again = port
        .send(known)
        .expect("the same frame is answerable twice");
    assert_eq!(
        again.id(),
        known.id(),
        "answering once must not poison the session"
    );
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

    #[test]
    fn a_frames_bytes_do_not_depend_on_map_order() {
        let frame = Frame::new(
            7,
            "browsingContext.navigate",
            serde_json::json!({ "url": "https://example.test/", "context": "c1" }),
        )
        .unwrap();
        assert_eq!(
            frame.to_wire(),
            "{\"id\":7,\"method\":\"browsingContext.navigate\",\"params\":{\"context\":\"c1\",\"url\":\"https://example.test/\"}}"
        );
        assert_eq!(frame.to_wire(), frame.to_wire());
    }

    #[test]
    fn a_frame_without_a_method_or_an_object_is_refused_at_construction() {
        assert!(Frame::new(1, "", serde_json::json!({})).is_err());
        let err = Frame::new(1, "script.evaluate", serde_json::json!([])).unwrap_err();
        assert_eq!(err.code(), &AxCode::InvalidArgs);
        assert!(err.recovery().contains("object"));
    }

    #[test]
    fn a_remote_refusal_is_an_answer_and_keeps_its_own_words() {
        let reply = Reply::parse(
            "{\"type\":\"error\",\"id\":3,\"error\":\"no such node\",\"message\":\"stale ref\"}",
        )
        .unwrap();
        assert_eq!(reply.id(), 3);
        let err = reply.into_result().unwrap_err();
        assert!(err.subject().contains("no such node"));
        assert!(err.subject().contains("stale ref"));
        assert!(err.recovery().contains("fresh snapshot"));
    }

    #[test]
    fn a_reply_shape_this_version_cannot_read_is_refused_rather_than_guessed() {
        for line in [
            "not json",
            "[]",
            "{\"type\":\"success\"}",
            "{\"id\":1,\"type\":\"event\"}",
        ] {
            let err = Reply::parse(line).unwrap_err();
            assert_eq!(err.code(), &AxCode::WireMismatch, "{line}");
        }
    }

    #[test]
    fn a_success_carries_its_result_through_unchanged() {
        let reply =
            Reply::parse("{\"type\":\"success\",\"id\":1,\"result\":{\"contexts\":[]}}").unwrap();
        assert_eq!(
            reply.into_result().unwrap(),
            serde_json::json!({ "contexts": [] })
        );
    }
}
