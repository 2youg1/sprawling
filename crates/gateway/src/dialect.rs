// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Pure two-way translation between the canonical conversation
//! (Anthropic-shaped) and the two supported wire
//! dialects. No I/O, no state, no clock: byte-for-byte explainable
//! requests are the whole point of writing the wire format ourselves.
//!
//! **Read the provider's own documentation before changing anything in
//! this module.** Every field here is the provider's shape, not ours; a
//! shape that looks wrong is usually a shape that changed.
//!
//! City dialect — Anthropic Messages:
//! - Request and response: <https://platform.claude.com/docs/en/api/messages>
//! - Thinking blocks, `signature`, preservation across tool use:
//!   <https://platform.claude.com/docs/en/build-with-claude/thinking>
//! - Effort levels: <https://platform.claude.com/docs/en/build-with-claude/effort>
//! - What invalidates a cache breakpoint:
//!   <https://platform.claude.com/docs/en/build-with-claude/prompt-caching>
//! - The 400 that a modified thinking block earns:
//!   <https://platform.claude.com/docs/en/agents-and-tools/tool-use/troubleshooting-tool-use>
//!
//! Translation counterpart — OpenAI Chat Completions:
//! - Request and response:
//!   <https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create/>
//! - `reasoning.effort` and its accepted values:
//!   <https://developers.openai.com/api/docs/guides/reasoning>
//!
//! Loss accounting is explicit: the OpenAI request wire has no explicit
//! cache breakpoints (provider caching is implicit prefix-matching), so
//! `cache` markers drop on that path — documented here, asserted in tests.
//! Float-bearing tool inputs are refused (`E_WIRE_MISMATCH`): tool args
//! become ledger payloads verbatim, and the ledger bans floats.

use kernel::{
    AxCode, AxError, ChatRequest, ChatResponse, ContentBlock, DialectKind, Effort, ModelUsage,
    Payload, Role, StopReason, Tokens,
};
use serde_json::{Map, Value, json};

fn mismatch(path: &str, detail: &str) -> AxError {
    AxError::failure(
        AxCode::WireMismatch,
        "translate wire",
        format!("{path}: {detail}"),
    )
    .with_recovery(
        "the provider answered in a shape this dialect does not know; check that the \
         endpoint's dialect matches the provider and that its base url is the one its \
         documentation prints",
    )
}

/// The provider took the request, answered 200, and put nothing in it.
///
/// Told apart from every other shape mismatch because it is not one: the
/// envelope is this dialect's, every field is where it belongs, and
/// `choices` is null. Observed on a hosted OpenAI-compatible endpoint
/// when `max_tokens` is above what the chosen model allows - the request
/// is neither refused nor answered, it is dropped, and the only trace is
/// a nulled answer with a zeroed usage block. A person reading
/// "expected array" learns nothing they can act on, which is why this
/// case says what to change.
fn empty_answer() -> AxError {
    AxError::failure(
        AxCode::Provider,
        "read the provider's answer",
        "the provider accepted the request and returned no answer at all",
    )
    .with_recovery(
        "lower this model's max output tokens - a ceiling above what the model allows is \
         answered this way rather than refused - then dispatch again",
    )
    .retriable()
}

/// Canonical request onto the wire.
pub fn request_wire(kind: DialectKind, req: &ChatRequest) -> Result<Value, AxError> {
    match kind {
        DialectKind::Anthropic => anthropic_request(req),
        DialectKind::OpenAi => openai_request(req),
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

/// Wire response into the canonical shape.
pub fn response_from_wire(kind: DialectKind, wire: &Value) -> Result<ChatResponse, AxError> {
    match kind {
        DialectKind::Anthropic => anthropic_response_from(wire),
        DialectKind::OpenAi => openai_response_from(wire),
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
        DialectKind::Anthropic => anthropic_response_wire(resp),
        DialectKind::OpenAi => openai_response_wire(resp),
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

// ---------------------------------------------------------------- anthropic

fn role_str(role: Role) -> Result<&'static str, AxError> {
    match role {
        Role::User => Ok("user"),
        Role::Assistant => Ok("assistant"),
        _ => Err(mismatch("message.role", "unknown canonical role")),
    }
}

fn anthropic_block(block: &ContentBlock) -> Result<Value, AxError> {
    match block {
        ContentBlock::Text { text } => Ok(json!({ "type": "text", "text": text })),
        // Verbatim in both directions: the provider verifies `signature`
        // against the reasoning it issued, and a modified thinking block
        // is refused with a 400 rather than tolerated.
        ContentBlock::Thinking {
            thinking,
            signature,
        } => Ok(json!({ "type": "thinking", "thinking": thinking, "signature": signature })),
        ContentBlock::RedactedThinking { data } => {
            Ok(json!({ "type": "redacted_thinking", "data": data }))
        }
        ContentBlock::ToolUse { id, name, input } => Ok(json!({
            "type": "tool_use", "id": id, "name": name.as_str(),
            "input": serde_json::to_value(input)
                .map_err(|err| mismatch("tool_use.input", &err.to_string()))?,
        })),
        ContentBlock::ToolResult {
            tool_use_id,
            content,
            is_error,
        } => Ok(json!({
            "type": "tool_result", "tool_use_id": tool_use_id,
            "content": content, "is_error": is_error,
        })),
        _ => Err(mismatch("content.block", "unknown canonical block kind")),
    }
}

fn anthropic_request(req: &ChatRequest) -> Result<Value, AxError> {
    let mut root = Map::new();
    root.insert("model".to_owned(), Value::String(req.model.clone()));
    root.insert(
        "max_tokens".to_owned(),
        Value::Number(req.max_tokens.into()),
    );
    if !req.system.is_empty() {
        let system: Vec<Value> = req
            .system
            .iter()
            .map(|block| {
                let mut entry = Map::new();
                entry.insert("type".to_owned(), Value::String("text".to_owned()));
                entry.insert("text".to_owned(), Value::String(block.text.clone()));
                if block.cache {
                    entry.insert("cache_control".to_owned(), json!({ "type": "ephemeral" }));
                }
                Value::Object(entry)
            })
            .collect();
        root.insert("system".to_owned(), Value::Array(system));
    }
    let mut messages = Vec::new();
    for message in &req.messages {
        let role = role_str(message.role)?;
        let blocks: Result<Vec<Value>, AxError> =
            message.content.iter().map(anthropic_block).collect();
        messages.push(json!({ "role": role, "content": blocks? }));
    }
    root.insert("messages".to_owned(), Value::Array(messages));
    for (key, value) in anthropic_effort(req.effort)? {
        root.insert(key.to_owned(), value);
    }
    if !req.tools.is_empty() {
        let tools: Result<Vec<Value>, AxError> = req
            .tools
            .iter()
            .map(|tool| {
                Ok(json!({
                    "name": tool.name.as_str(),
                    "description": tool.description,
                    "input_schema": serde_json::to_value(&tool.input_schema)
                        .map_err(|err| mismatch("tools.input_schema", &err.to_string()))?,
                }))
            })
            .collect();
        root.insert("tools".to_owned(), Value::Array(tools?));
    }
    Ok(Value::Object(root))
}

fn require<'v>(wire: &'v Value, path: &str, key: &str) -> Result<&'v Value, AxError> {
    wire.get(key)
        .ok_or_else(|| mismatch(&format!("{path}.{key}"), "missing"))
}

fn as_str<'v>(value: &'v Value, path: &str) -> Result<&'v str, AxError> {
    value
        .as_str()
        .ok_or_else(|| mismatch(path, "expected string"))
}

fn as_u64(value: &Value, path: &str) -> Result<u64, AxError> {
    value
        .as_u64()
        .ok_or_else(|| mismatch(path, "expected unsigned integer"))
}

fn tokens_or_zero(usage: &Value, key: &str, path: &str) -> Result<Tokens, AxError> {
    match usage.get(key) {
        None | Some(Value::Null) => Ok(Tokens::new(0)),
        Some(value) => Ok(Tokens::new(as_u64(value, &format!("{path}.{key}"))?)),
    }
}

fn payload_from(value: &Value, path: &str) -> Result<Payload, AxError> {
    if kernel::value_has_float(value) {
        return Err(mismatch(path, "float payloads are banned city-wide"));
    }
    serde_json::from_value(value.clone()).map_err(|err| mismatch(path, &err.to_string()))
}

fn anthropic_block_from(value: &Value, path: &str) -> Result<ContentBlock, AxError> {
    let kind = as_str(require(value, path, "type")?, &format!("{path}.type"))?;
    match kind {
        "text" => Ok(ContentBlock::Text {
            text: as_str(require(value, path, "text")?, &format!("{path}.text"))?.to_owned(),
        }),
        "thinking" => Ok(ContentBlock::Thinking {
            thinking: as_str(
                require(value, path, "thinking")?,
                &format!("{path}.thinking"),
            )?
            .to_owned(),
            signature: as_str(
                require(value, path, "signature")?,
                &format!("{path}.signature"),
            )?
            .to_owned(),
        }),
        "redacted_thinking" => Ok(ContentBlock::RedactedThinking {
            data: as_str(require(value, path, "data")?, &format!("{path}.data"))?.to_owned(),
        }),
        "tool_use" => {
            let name_raw = as_str(require(value, path, "name")?, &format!("{path}.name"))?;
            let name = kernel::ToolName::parse(name_raw)
                .map_err(|_| mismatch(&format!("{path}.name"), "not a tool name"))?;
            Ok(ContentBlock::ToolUse {
                id: as_str(require(value, path, "id")?, &format!("{path}.id"))?.to_owned(),
                name,
                input: payload_from(require(value, path, "input")?, &format!("{path}.input"))?,
            })
        }
        "tool_result" => Ok(ContentBlock::ToolResult {
            tool_use_id: as_str(
                require(value, path, "tool_use_id")?,
                &format!("{path}.tool_use_id"),
            )?
            .to_owned(),
            content: as_str(require(value, path, "content")?, &format!("{path}.content"))?
                .to_owned(),
            is_error: value
                .get("is_error")
                .and_then(Value::as_bool)
                .unwrap_or(false),
        }),
        other => Err(mismatch(&format!("{path}.type"), other)),
    }
}

fn stop_from(raw: &str, path: &str) -> Result<StopReason, AxError> {
    match raw {
        "end_turn" => Ok(StopReason::EndTurn),
        "tool_use" => Ok(StopReason::ToolUse),
        "max_tokens" => Ok(StopReason::MaxTokens),
        other => Err(mismatch(path, other)),
    }
}

fn anthropic_response_from(wire: &Value) -> Result<ChatResponse, AxError> {
    let content_value = require(wire, "response", "content")?;
    let items = content_value
        .as_array()
        .ok_or_else(|| mismatch("response.content", "expected array"))?;
    let mut content = Vec::new();
    for (i, item) in items.iter().enumerate() {
        content.push(anthropic_block_from(
            item,
            &format!("response.content[{i}]"),
        )?);
    }
    let stop = stop_from(
        as_str(
            require(wire, "response", "stop_reason")?,
            "response.stop_reason",
        )?,
        "response.stop_reason",
    )?;
    let usage_value = require(wire, "response", "usage")?;
    let usage = ModelUsage {
        input_tokens: tokens_or_zero(usage_value, "input_tokens", "response.usage")?,
        output_tokens: tokens_or_zero(usage_value, "output_tokens", "response.usage")?,
        cache_read_tokens: tokens_or_zero(
            usage_value,
            "cache_read_input_tokens",
            "response.usage",
        )?,
        cache_write_tokens: tokens_or_zero(
            usage_value,
            "cache_creation_input_tokens",
            "response.usage",
        )?,
    };
    Ok(ChatResponse {
        content,
        stop,
        usage,
    })
}

fn stop_str(stop: StopReason) -> Result<&'static str, AxError> {
    match stop {
        StopReason::EndTurn => Ok("end_turn"),
        StopReason::ToolUse => Ok("tool_use"),
        StopReason::MaxTokens => Ok("max_tokens"),
        _ => Err(mismatch("stop_reason", "unknown canonical stop reason")),
    }
}

fn anthropic_response_wire(resp: &ChatResponse) -> Result<Value, AxError> {
    let content: Result<Vec<Value>, AxError> = resp.content.iter().map(anthropic_block).collect();
    Ok(json!({
        "content": content?,
        "stop_reason": stop_str(resp.stop)?,
        "usage": {
            "input_tokens": resp.usage.input_tokens.get(),
            "output_tokens": resp.usage.output_tokens.get(),
            "cache_read_input_tokens": resp.usage.cache_read_tokens.get(),
            "cache_creation_input_tokens": resp.usage.cache_write_tokens.get(),
        },
    }))
}

/// The request field that states how hard to think. This dialect spells
/// the five working levels in `effort`, and spells "do not think" in a
/// different field entirely — `effort` has no `none`.
fn anthropic_effort(effort: Option<Effort>) -> Result<Vec<(&'static str, Value)>, AxError> {
    let Some(effort) = effort else {
        return Ok(Vec::new());
    };
    let level = match effort {
        Effort::None => return Ok(vec![("thinking", json!({ "type": "disabled" }))]),
        Effort::Low => "low",
        Effort::Medium => "medium",
        Effort::High => "high",
        Effort::XHigh => "xhigh",
        Effort::Max => "max",
        _ => return Err(unspelled_effort(effort, "anthropic")),
    };
    Ok(vec![("effort", Value::String(level.to_owned()))])
}

/// A level added to the canonical ladder that this module has not been
/// taught to write. Fail closed rather than send a neighbouring level:
/// "I asked for max" must never silently become "I got high".
fn unspelled_effort(effort: Effort, dialect: &str) -> AxError {
    AxError::failure(
        AxCode::ConfigInvalid,
        format!("put an effort level on the {dialect} wire"),
        format!("{effort:?} has no spelling in this dialect"),
    )
    .with_recovery("teach this dialect the level, or pick one it already writes")
}

// ------------------------------------------------------------------ openai

/// This dialect writes every level in one field, `none` included.
fn openai_effort(effort: Effort) -> Result<&'static str, AxError> {
    match effort {
        Effort::None => Ok("none"),
        Effort::Low => Ok("low"),
        Effort::Medium => Ok("medium"),
        Effort::High => Ok("high"),
        Effort::XHigh => Ok("xhigh"),
        Effort::Max => Ok("max"),
        _ => Err(unspelled_effort(effort, "openai")),
    }
}

fn joined_text(content: &[ContentBlock]) -> String {
    let mut out = String::new();
    for block in content {
        if let ContentBlock::Text { text } = block {
            if !out.is_empty() {
                out.push_str("\n\n");
            }
            out.push_str(text);
        }
    }
    out
}

fn openai_request(req: &ChatRequest) -> Result<Value, AxError> {
    let mut messages = Vec::new();
    if !req.system.is_empty() {
        // Explicit breakpoints have no OpenAI wire slot; the marker drops
        // here by design (provider caching is implicit prefix matching).
        let system: Vec<&str> = req.system.iter().map(|b| b.text.as_str()).collect();
        messages.push(json!({ "role": "system", "content": system.join("\n\n") }));
    }
    for message in &req.messages {
        match message.role {
            Role::User => {
                for block in &message.content {
                    if let ContentBlock::ToolResult {
                        tool_use_id,
                        content,
                        is_error: _,
                    } = block
                    {
                        messages.push(json!({
                            "role": "tool", "tool_call_id": tool_use_id, "content": content,
                        }));
                    }
                }
                let text = joined_text(&message.content);
                if !text.is_empty() {
                    messages.push(json!({ "role": "user", "content": text }));
                }
            }
            Role::Assistant => {
                let text = joined_text(&message.content);
                let mut entry = Map::new();
                entry.insert("role".to_owned(), Value::String("assistant".to_owned()));
                entry.insert(
                    "content".to_owned(),
                    if text.is_empty() {
                        Value::Null
                    } else {
                        Value::String(text)
                    },
                );
                let mut tool_calls = Vec::new();
                for block in &message.content {
                    if let ContentBlock::ToolUse { id, name, input } = block {
                        let arguments = serde_json::to_string(input)
                            .map_err(|err| mismatch("tool_use.input", &err.to_string()))?;
                        tool_calls.push(json!({
                            "id": id, "type": "function",
                            "function": { "name": name.as_str(), "arguments": arguments },
                        }));
                    }
                }
                if !tool_calls.is_empty() {
                    entry.insert("tool_calls".to_owned(), Value::Array(tool_calls));
                }
                messages.push(Value::Object(entry));
            }
            _ => return Err(mismatch("message.role", "unknown canonical role")),
        }
    }
    let mut root = Map::new();
    root.insert("model".to_owned(), Value::String(req.model.clone()));
    root.insert(
        "max_tokens".to_owned(),
        Value::Number(req.max_tokens.into()),
    );
    root.insert("messages".to_owned(), Value::Array(messages));
    if let Some(effort) = req.effort {
        root.insert(
            "reasoning".to_owned(),
            json!({ "effort": openai_effort(effort)? }),
        );
    }
    if !req.tools.is_empty() {
        let tools: Result<Vec<Value>, AxError> = req
            .tools
            .iter()
            .map(|tool| {
                Ok(json!({
                    "type": "function",
                    "function": {
                        "name": tool.name.as_str(),
                        "description": tool.description,
                        "parameters": serde_json::to_value(&tool.input_schema)
                            .map_err(|err| mismatch("tools.parameters", &err.to_string()))?,
                    },
                }))
            })
            .collect();
        root.insert("tools".to_owned(), Value::Array(tools?));
    }
    Ok(Value::Object(root))
}

fn openai_response_from(wire: &Value) -> Result<ChatResponse, AxError> {
    let offered = require(wire, "response", "choices")?;
    // A null here is a provider that dropped the request rather than a
    // provider speaking a shape we do not know.
    if offered.is_null() {
        return Err(empty_answer());
    }
    let choices = offered
        .as_array()
        .ok_or_else(|| mismatch("response.choices", "expected array"))?;
    let first = choices
        .first()
        .ok_or_else(|| mismatch("response.choices", "empty"))?;
    let message = require(first, "response.choices[0]", "message")?;
    let mut content = Vec::new();
    if let Some(text) = message.get("content").and_then(Value::as_str)
        && !text.is_empty()
    {
        content.push(ContentBlock::Text {
            text: text.to_owned(),
        });
    }
    if let Some(calls) = message.get("tool_calls").and_then(Value::as_array) {
        for (i, call) in calls.iter().enumerate() {
            let path = format!("response.choices[0].message.tool_calls[{i}]");
            let function = require(call, &path, "function")?;
            let name_raw = as_str(require(function, &path, "name")?, &format!("{path}.name"))?;
            let name = kernel::ToolName::parse(name_raw)
                .map_err(|_| mismatch(&format!("{path}.name"), "not a tool name"))?;
            let arguments = as_str(
                require(function, &path, "arguments")?,
                &format!("{path}.arguments"),
            )?;
            let parsed: Value = serde_json::from_str(arguments)
                .map_err(|err| mismatch(&format!("{path}.arguments"), &err.to_string()))?;
            content.push(ContentBlock::ToolUse {
                id: as_str(require(call, &path, "id")?, &format!("{path}.id"))?.to_owned(),
                name,
                input: payload_from(&parsed, &format!("{path}.arguments"))?,
            });
        }
    }
    let stop = match as_str(
        require(first, "response.choices[0]", "finish_reason")?,
        "response.choices[0].finish_reason",
    )? {
        "stop" => StopReason::EndTurn,
        "tool_calls" => StopReason::ToolUse,
        "length" => StopReason::MaxTokens,
        other => return Err(mismatch("response.choices[0].finish_reason", other)),
    };
    let usage_value = require(wire, "response", "usage")?;
    let cache_read = usage_value
        .get("prompt_tokens_details")
        .map(|details| {
            tokens_or_zero(
                details,
                "cached_tokens",
                "response.usage.prompt_tokens_details",
            )
        })
        .transpose()?
        .unwrap_or(Tokens::new(0));
    let usage = ModelUsage {
        input_tokens: tokens_or_zero(usage_value, "prompt_tokens", "response.usage")?,
        output_tokens: tokens_or_zero(usage_value, "completion_tokens", "response.usage")?,
        cache_read_tokens: cache_read,
        // No OpenAI wire slot: cache writes are not reported distinctly.
        cache_write_tokens: Tokens::new(0),
    };
    Ok(ChatResponse {
        content,
        stop,
        usage,
    })
}

fn openai_response_wire(resp: &ChatResponse) -> Result<Value, AxError> {
    let text = joined_text(&resp.content);
    let mut message = Map::new();
    message.insert("role".to_owned(), Value::String("assistant".to_owned()));
    message.insert(
        "content".to_owned(),
        if text.is_empty() {
            Value::Null
        } else {
            Value::String(text)
        },
    );
    let mut tool_calls = Vec::new();
    for block in &resp.content {
        if let ContentBlock::ToolUse { id, name, input } = block {
            let arguments = serde_json::to_string(input)
                .map_err(|err| mismatch("tool_use.input", &err.to_string()))?;
            tool_calls.push(json!({
                "id": id, "type": "function",
                "function": { "name": name.as_str(), "arguments": arguments },
            }));
        }
    }
    if !tool_calls.is_empty() {
        message.insert("tool_calls".to_owned(), Value::Array(tool_calls));
    }
    let finish = match resp.stop {
        StopReason::EndTurn => "stop",
        StopReason::ToolUse => "tool_calls",
        StopReason::MaxTokens => "length",
        _ => return Err(mismatch("stop_reason", "unknown canonical stop reason")),
    };
    Ok(json!({
        "choices": [ { "message": Value::Object(message), "finish_reason": finish } ],
        "usage": {
            "prompt_tokens": resp.usage.input_tokens.get(),
            "completion_tokens": resp.usage.output_tokens.get(),
            "prompt_tokens_details": { "cached_tokens": resp.usage.cache_read_tokens.get() },
        },
    }))
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
    use kernel::{ChatMessage, SystemBlock, ToolDef, ToolName};
    use proptest::prelude::*;

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
