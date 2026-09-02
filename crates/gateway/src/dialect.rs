// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Which dialect answers a question, and nothing about how it answers.
//!
//! Five entrances, one `match` each, and a closed set of two: a dialect
//! this build cannot translate is refused rather than approximated with
//! the nearer of the two it knows. What each dialect does with a request
//! is `gateway::anthropic`'s and `gateway::openai`'s; what they share is
//! `gateway::mismatch`'s.
//!
//! The canonical conversation is Anthropic-shaped, so the losses are all
//! on the other path and each is documented where it is taken.
//!
//! No I/O, no state, no clock: byte-for-byte explainable requests are
//! the whole point of writing the wire format ourselves.

use kernel::{AxCode, AxError, ChatRequest, ChatResponse, DialectKind};
use serde_json::Value;

use crate::{anthropic, openai};

/// Canonical request onto the wire.
pub fn request_wire(kind: DialectKind, req: &ChatRequest) -> Result<Value, AxError> {
    match kind {
        DialectKind::Anthropic => anthropic::request(req),
        DialectKind::OpenAi => openai::request(req),
        // Fail closed: a dialect this build cannot translate is not
        // approximated with the nearer of the two it knows.
        _ => Err(AxError::failure(
            AxCode::EndpointDialectUnsupported,
            "translate wire",
            format!("{kind:?} is not a dialect this build speaks"),
        )
        .with_recovery("attach the endpoint as anthropic or openai")),
    }
}

/// The text one server-sent event carries, if it carries any.
///
/// **Only text.** Thinking blocks and tool arguments also arrive in
/// increments and neither may be shown while it is partial: half a tool
/// argument is not a shorter tool argument, and a thinking block is
/// relayed for the provider's signature rather than published. So this
/// reads exactly the field each dialect uses for assistant prose and
/// answers `None` for everything else, including every frame this build
/// does not recognise.
///
/// No `Result`: an increment nobody can read is an increment not shown,
/// and a display detail must not be able to fail a call that is
/// otherwise going fine.
#[must_use]
pub fn increment_of(kind: DialectKind, frame: &Value) -> Option<String> {
    let map = frame.as_object()?;
    match kind {
        DialectKind::Anthropic => anthropic::increment_of(map),
        DialectKind::OpenAi => openai::increment_of(map),
        _ => None,
    }
    .filter(|text| !text.is_empty())
}

/// The settled answer a stream ends with, in the shape a non-streaming
/// call would have returned.
///
/// **One parser for the answer.** Reassembling here rather than reading
/// the stream into a `ChatResponse` directly is what stops a second
/// authority forming: `response_from_wire` remains the only code that
/// decides what a provider said, so a streamed call and a blocking call
/// cannot come to different conclusions about the same reply.
///
/// # Errors
/// `Provider` when the stream ended without the frames that carry the
/// answer. That is the same failure a truncated body is, and it is
/// deliberately not recoverable by keeping the increments: a partial
/// reply presented as a whole one is the one outcome this must not have.
pub fn settled_from_stream(kind: DialectKind, frames: &[Value]) -> Result<Value, AxError> {
    match kind {
        DialectKind::Anthropic => anthropic::settled(frames),
        DialectKind::OpenAi => openai::settled(frames),
        _ => Err(AxError::failure(
            AxCode::EndpointDialectUnsupported,
            "read a streamed answer",
            format!("{kind:?} is not a dialect this build speaks"),
        )
        .with_recovery("attach the endpoint as anthropic or openai")),
    }
}

/// Wire response into the canonical shape.
pub fn response_from_wire(kind: DialectKind, wire: &Value) -> Result<ChatResponse, AxError> {
    match kind {
        DialectKind::Anthropic => anthropic::response_from(wire),
        DialectKind::OpenAi => openai::response_from(wire),
        // Fail closed: a dialect this build cannot translate is not
        // approximated with the nearer of the two it knows.
        _ => Err(AxError::failure(
            AxCode::EndpointDialectUnsupported,
            "translate wire",
            format!("{kind:?} is not a dialect this build speaks"),
        )
        .with_recovery("attach the endpoint as anthropic or openai")),
    }
}

/// Canonical response onto the wire. Production uses this for replay
/// fixtures and citysim scripts; tests use it for round-trip proof.
pub fn response_wire(kind: DialectKind, resp: &ChatResponse) -> Result<Value, AxError> {
    match kind {
        DialectKind::Anthropic => anthropic::response_wire(resp),
        DialectKind::OpenAi => openai::response_wire(resp),
        // Fail closed: a dialect this build cannot translate is not
        // approximated with the nearer of the two it knows.
        _ => Err(AxError::failure(
            AxCode::EndpointDialectUnsupported,
            "translate wire",
            format!("{kind:?} is not a dialect this build speaks"),
        )
        .with_recovery("attach the endpoint as anthropic or openai")),
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
    /// A provider that answers 200 with a nulled `choices` has not spoken
    /// a shape we do not know - it has dropped the request. Found on a
    /// real hosted endpoint, where `max_tokens` above the model's own
    /// ceiling produced exactly this and the refusal that reached the
    /// person said "expected array" with an empty recovery.
    #[test]
    fn a_dropped_request_is_told_apart_from_a_shape_we_cannot_read() {
        let dropped = serde_json::json!({
            "id": "", "object": "", "created": 0, "model": "m",
            "choices": null,
            "usage": {"prompt_tokens": 0, "completion_tokens": 0, "total_tokens": 0}
        });
        let refused = super::response_from_wire(kernel::DialectKind::OpenAi, &dropped)
            .expect_err("a nulled answer is not an answer");
        assert_eq!(refused.code(), &kernel::AxCode::Provider);
        assert!(
            refused.recovery().contains("max output tokens"),
            "a refusal has to name what to change: {}",
            refused.recovery()
        );
    }

    /// Every shape mismatch carries a way out. An empty `recovery` is
    /// the contract `AxError` states being broken in the one place a
    /// person meets it.
    #[test]
    fn no_translation_refusal_leaves_a_person_with_nothing_to_try() {
        let alien = serde_json::json!({ "choices": "not an array", "usage": {} });
        let refused = super::response_from_wire(kernel::DialectKind::OpenAi, &alien)
            .expect_err("a string is not a choices array");
        assert!(!refused.recovery().is_empty());
    }

    use super::*;
    use kernel::{ChatMessage, ContentBlock, ModelUsage, Role, StopReason, SystemBlock, ToolDef};
    use kernel::{Effort, Payload, Tokens, ToolName};
    use proptest::prelude::*;
    use serde_json::{Map, json};

    fn sample_request() -> ChatRequest {
        let mut schema = Map::new();
        schema.insert("type".to_owned(), Value::String("object".to_owned()));
        let mut args = Map::new();
        args.insert("path".to_owned(), Value::String("notes.md".to_owned()));
        ChatRequest {
            model: "sonnet".to_owned(),
            max_tokens: 4096,
            system: vec![
                SystemBlock {
                    text: "city".to_owned(),
                    cache: true,
                },
                SystemBlock {
                    text: "building".to_owned(),
                    cache: true,
                },
            ],
            messages: vec![
                ChatMessage {
                    role: Role::User,
                    content: vec![ContentBlock::Text {
                        text: "Task: probe".to_owned(),
                    }],
                },
                ChatMessage {
                    role: Role::Assistant,
                    content: vec![
                        ContentBlock::Text {
                            text: "reading".to_owned(),
                        },
                        ContentBlock::ToolUse {
                            id: "tu_1".to_owned(),
                            name: ToolName::parse("exec").unwrap(),
                            input: Payload::new(args).unwrap(),
                        },
                    ],
                },
                ChatMessage {
                    role: Role::User,
                    content: vec![ContentBlock::ToolResult {
                        tool_use_id: "tu_1".to_owned(),
                        content: "ok".to_owned(),
                        is_error: false,
                    }],
                },
            ],
            tools: vec![ToolDef {
                name: ToolName::parse("exec").unwrap(),
                description: "run things".to_owned(),
                input_schema: Payload::new(schema).unwrap(),
            }],
            effort: None,
        }
    }

    #[test]
    fn anthropic_request_wire_is_pinned() {
        let wire = request_wire(DialectKind::Anthropic, &sample_request()).unwrap();
        insta::assert_snapshot!(serde_json::to_string_pretty(&wire).unwrap());
    }

    #[test]
    fn openai_request_wire_is_pinned() {
        let wire = request_wire(DialectKind::OpenAi, &sample_request()).unwrap();
        insta::assert_snapshot!(serde_json::to_string_pretty(&wire).unwrap());
    }

    #[test]
    fn anthropic_breakpoints_land_on_marked_blocks_only() {
        let wire = request_wire(DialectKind::Anthropic, &sample_request()).unwrap();
        let system = wire["system"].as_array().unwrap();
        assert!(
            system
                .iter()
                .all(|b| b["cache_control"]["type"] == "ephemeral")
        );
        let mut unmarked = sample_request();
        unmarked.system[1].cache = false;
        let wire = request_wire(DialectKind::Anthropic, &unmarked).unwrap();
        assert!(wire["system"][1].get("cache_control").is_none());
    }

    #[test]
    fn tool_shapes_survive_both_request_dialects() {
        let req = sample_request();
        let anthropic = request_wire(DialectKind::Anthropic, &req).unwrap();
        assert_eq!(anthropic["tools"][0]["name"], "exec");
        assert_eq!(anthropic["tools"][0]["input_schema"]["type"], "object");
        let openai = request_wire(DialectKind::OpenAi, &req).unwrap();
        assert_eq!(openai["tools"][0]["function"]["name"], "exec");
        assert_eq!(
            openai["tools"][0]["function"]["parameters"]["type"],
            "object"
        );
        // The assistant tool_use crosses as a tool_call with the same id.
        let calls = openai["messages"]
            .as_array()
            .unwrap()
            .iter()
            .find_map(|m| m.get("tool_calls"))
            .unwrap();
        assert_eq!(calls[0]["id"], "tu_1");
    }

    #[test]
    fn anthropic_returns_thinking_blocks_exactly_as_it_issued_them() {
        // Official rule: "Include the complete unmodified block back to
        // the API"; altering it earns a 400 saying the thinking blocks
        // "cannot be modified". So both directions must be byte-exact.
        let wire = json!({
            "content": [
                { "type": "thinking", "thinking": "two parts", "signature": "WaUjzkyp" },
                { "type": "redacted_thinking", "data": "EroBCkYIARgC" },
                { "type": "text", "text": "answer" },
            ],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 1, "output_tokens": 2 },
        });
        let canonical = response_from_wire(DialectKind::Anthropic, &wire).unwrap();
        assert_eq!(
            canonical.content[0],
            ContentBlock::Thinking {
                thinking: "two parts".to_owned(),
                signature: "WaUjzkyp".to_owned(),
            }
        );
        assert_eq!(
            canonical.content[1],
            ContentBlock::RedactedThinking {
                data: "EroBCkYIARgC".to_owned(),
            }
        );
        assert_eq!(
            response_wire(DialectKind::Anthropic, &canonical).unwrap()["content"],
            wire["content"],
            "a thinking block that made a round trip is no longer the one Claude signed"
        );

        // And on the way back out, inside a request's message history.
        let mut req = sample_request();
        req.messages[1].content.insert(
            0,
            ContentBlock::Thinking {
                thinking: "two parts".to_owned(),
                signature: "WaUjzkyp".to_owned(),
            },
        );
        let out = request_wire(DialectKind::Anthropic, &req).unwrap();
        assert_eq!(out["messages"][1]["content"][0], wire["content"][0]);
    }

    #[test]
    fn openai_drops_thinking_because_it_cannot_spell_it() {
        // Chat Completions has no counterpart and demands no round trip.
        // The canonical record keeps the block; only the wire loses it,
        // and the dialect is a pure function, so replay can still derive
        // the bytes that were actually sent.
        let mut req = sample_request();
        req.messages[1].content.insert(
            0,
            ContentBlock::Thinking {
                thinking: "two parts".to_owned(),
                signature: "WaUjzkyp".to_owned(),
            },
        );
        let out = request_wire(DialectKind::OpenAi, &req).unwrap();
        assert!(
            !out.to_string().contains("WaUjzkyp"),
            "a signature the other provider cannot verify does not belong on its wire"
        );
        assert!(out.to_string().contains("reading"));
    }

    #[test]
    fn effort_rides_the_wire_each_dialect_spells_it_its_own_way() {
        let mut req = sample_request();
        assert!(
            request_wire(DialectKind::Anthropic, &req)
                .unwrap()
                .get("effort")
                .is_none(),
            "an unstated effort writes no field: the provider's default is its own business"
        );

        req.effort = Some(Effort::High);
        assert_eq!(
            request_wire(DialectKind::Anthropic, &req).unwrap()["effort"],
            "high"
        );
        assert_eq!(
            request_wire(DialectKind::OpenAi, &req).unwrap()["reasoning"]["effort"],
            "high"
        );

        // The one place the two dialects part: not thinking is an effort
        // value on one wire and a different field on the other.
        req.effort = Some(Effort::None);
        let anthropic = request_wire(DialectKind::Anthropic, &req).unwrap();
        assert_eq!(anthropic["thinking"]["type"], "disabled");
        assert!(anthropic.get("effort").is_none());
        assert_eq!(
            request_wire(DialectKind::OpenAi, &req).unwrap()["reasoning"]["effort"],
            "none"
        );
    }

    #[test]
    fn every_level_of_the_ladder_reaches_both_wires() {
        let mut req = sample_request();
        for (level, spelling) in [
            (Effort::Low, "low"),
            (Effort::Medium, "medium"),
            (Effort::High, "high"),
            (Effort::XHigh, "xhigh"),
            (Effort::Max, "max"),
        ] {
            req.effort = Some(level);
            assert_eq!(
                request_wire(DialectKind::Anthropic, &req).unwrap()["effort"],
                spelling
            );
            assert_eq!(
                request_wire(DialectKind::OpenAi, &req).unwrap()["reasoning"]["effort"],
                spelling
            );
        }
    }

    #[test]
    fn float_tool_input_is_refused_at_the_wire_face() {
        let wire = json!({
            "content": [ { "type": "tool_use", "id": "x", "name": "exec",
                           "input": { "temperature": 0.5 } } ],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 1, "output_tokens": 1 },
        });
        let err = response_from_wire(DialectKind::Anthropic, &wire).unwrap_err();
        assert_eq!(*err.code(), AxCode::WireMismatch);
    }

    #[test]
    fn unknown_stop_reason_is_a_mismatch_not_a_guess() {
        let wire = json!({
            "content": [], "stop_reason": "pause_turn",
            "usage": { "input_tokens": 0, "output_tokens": 0 },
        });
        let err = response_from_wire(DialectKind::Anthropic, &wire).unwrap_err();
        assert_eq!(*err.code(), AxCode::WireMismatch);
        assert!(err.subject().contains("pause_turn"));
    }

    #[test]
    fn missing_usage_fields_read_as_zero() {
        let wire = json!({
            "content": [ { "type": "text", "text": "hi" } ],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 7, "output_tokens": 3 },
        });
        let resp = response_from_wire(DialectKind::Anthropic, &wire).unwrap();
        assert_eq!(resp.usage.cache_read_tokens, Tokens::new(0));
        assert_eq!(resp.usage.input_tokens, Tokens::new(7));
    }

    fn text_block() -> impl Strategy<Value = ContentBlock> {
        "[a-z ]{1,20}".prop_map(|text| ContentBlock::Text { text })
    }

    fn tool_use_block() -> impl Strategy<Value = ContentBlock> {
        ("[a-z]{1,8}", "[a-z_]{1,10}", "[a-z]{0,12}").prop_map(|(id, name, arg)| {
            let mut args = Map::new();
            args.insert("q".to_owned(), Value::String(arg));
            ContentBlock::ToolUse {
                id,
                name: ToolName::parse(&name).unwrap(),
                input: Payload::new(args).unwrap(),
            }
        })
    }

    fn usage_strategy(cache_write: bool) -> impl Strategy<Value = ModelUsage> {
        (0u64..9999, 0u64..9999, 0u64..9999, 0u64..9999).prop_map(move |(i, o, r, w)| ModelUsage {
            input_tokens: Tokens::new(i),
            output_tokens: Tokens::new(o),
            cache_read_tokens: Tokens::new(r),
            cache_write_tokens: Tokens::new(if cache_write { w } else { 0 }),
        })
    }

    fn stop_strategy() -> impl Strategy<Value = StopReason> {
        prop_oneof![
            Just(StopReason::EndTurn),
            Just(StopReason::ToolUse),
            Just(StopReason::MaxTokens),
        ]
    }

    proptest! {
        /// Anthropic response round-trip is lossless: wire -> canonical -> wire.
        #[test]
        fn anthropic_response_roundtrip(
            blocks in proptest::collection::vec(prop_oneof![text_block(), tool_use_block()], 0..4),
            stop in stop_strategy(),
            usage in usage_strategy(true),
        ) {
            let resp = ChatResponse { content: blocks, stop, usage };
            let wire = response_wire(DialectKind::Anthropic, &resp).unwrap();
            let back = response_from_wire(DialectKind::Anthropic, &wire).unwrap();
            prop_assert_eq!(back, resp);
        }

        /// OpenAI response round-trip within its wire's expressiveness:
        /// at most one text block, no distinct cache-write slot.
        #[test]
        fn openai_response_roundtrip(
            text in proptest::option::of(text_block()),
            tools in proptest::collection::vec(tool_use_block(), 0..3),
            stop in stop_strategy(),
            usage in usage_strategy(false),
        ) {
            let mut content = Vec::new();
            if let Some(t) = text { content.push(t); }
            content.extend(tools);
            let resp = ChatResponse { content, stop, usage };
            let wire = response_wire(DialectKind::OpenAi, &resp).unwrap();
            let back = response_from_wire(DialectKind::OpenAi, &wire).unwrap();
            prop_assert_eq!(back, resp);
        }

        /// Usage integers survive both dialect wires verbatim.
        #[test]
        fn usage_is_preserved_verbatim(usage in usage_strategy(true)) {
            let resp = ChatResponse { content: vec![], stop: StopReason::EndTurn, usage };
            let wire = response_wire(DialectKind::Anthropic, &resp).unwrap();
            prop_assert_eq!(wire["usage"]["input_tokens"].as_u64().unwrap(), usage.input_tokens.get());
            prop_assert_eq!(wire["usage"]["cache_creation_input_tokens"].as_u64().unwrap(), usage.cache_write_tokens.get());
            let back = response_from_wire(DialectKind::Anthropic, &wire).unwrap();
            prop_assert_eq!(back.usage, usage);
        }
    }
}
