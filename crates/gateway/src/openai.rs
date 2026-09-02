// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The OpenAI Chat Completions wire, in both directions, and every loss
//! the translation takes.
//!
//! **Read the provider's own documentation before changing anything
//! here.** Every field is the provider's shape, not ours; a shape that
//! looks wrong is usually a shape that changed.
//!
//! - Request and response:
//!   <https://developers.openai.com/api/reference/resources/chat/subresources/completions/methods/create/>
//! - `reasoning.effort` and its accepted values:
//!   <https://developers.openai.com/api/docs/guides/reasoning>
//!
//! **Loss accounting is explicit, because this dialect is not the
//! canonical shape.** This wire has no explicit cache breakpoints
//! (provider caching is implicit prefix-matching), so `cache` markers
//! drop on this path; and it cannot spell a thinking block, so one is
//! not sent rather than sent as prose. Both are asserted in tests.

use kernel::{
    AxCode, AxError, ChatRequest, ChatResponse, ContentBlock, Effort, ModelUsage, Role, StopReason,
    Tokens,
};
use serde_json::{Map, Value, json};

use crate::mismatch::{
    as_str, mismatch, payload_from, require, stream_cut, tokens_or_zero, unspelled_effort,
};

/// The text one chunk carries, if it carries prose. Absent on the frames
/// that carry a tool call or a finish reason.
pub(crate) fn increment_of(map: &serde_json::Map<String, Value>) -> Option<String> {
    map.get("choices")?
        .as_array()?
        .first()?
        .as_object()?
        .get("delta")?
        .as_object()?
        .get("content")?
        .as_str()
        .map(str::to_owned)
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

/// OpenAI streams one `choices[0].delta` per chunk and the finish reason
/// on the last one. Tool calls arrive by index with their arguments split
/// across chunks, which is why they are joined before being read.
pub(crate) fn settled(frames: &[Value]) -> Result<Value, AxError> {
    let mut said = String::new();
    let mut calls: std::collections::BTreeMap<u64, (String, String, String)> =
        std::collections::BTreeMap::new();
    let mut finish = None;
    let mut usage = None;
    for frame in frames {
        let Some(map) = frame.as_object() else {
            continue;
        };
        if let Some(held) = map.get("usage")
            && !held.is_null()
        {
            usage = Some(held.clone());
        }
        let Some(first) = map
            .get("choices")
            .and_then(Value::as_array)
            .and_then(|choices| choices.first())
            .and_then(Value::as_object)
        else {
            continue;
        };
        if let Some(held) = first.get("finish_reason").and_then(Value::as_str) {
            finish = Some(held.to_owned());
        }
        let Some(delta) = first.get("delta").and_then(Value::as_object) else {
            continue;
        };
        if let Some(text) = delta.get("content").and_then(Value::as_str) {
            said.push_str(text);
        }
        for call in delta
            .get("tool_calls")
            .and_then(Value::as_array)
            .unwrap_or(&Vec::new())
        {
            let Some(one) = call.as_object() else {
                continue;
            };
            let at = one.get("index").and_then(Value::as_u64).unwrap_or_default();
            let held = calls.entry(at).or_default();
            if let Some(id) = one.get("id").and_then(Value::as_str) {
                held.0 = id.to_owned();
            }
            let Some(function) = one.get("function").and_then(Value::as_object) else {
                continue;
            };
            if let Some(name) = function.get("name").and_then(Value::as_str) {
                held.1 = name.to_owned();
            }
            if let Some(part) = function.get("arguments").and_then(Value::as_str) {
                held.2.push_str(part);
            }
        }
    }
    let Some(finish) = finish else {
        return Err(stream_cut(
            "the stream ended without the chunk that says why the model stopped",
        ));
    };
    let mut message = json!({ "role": "assistant", "content": said });
    if !calls.is_empty()
        && let Some(map) = message.as_object_mut()
    {
        let wired: Vec<Value> = calls
            .into_values()
            .map(|(id, name, arguments)| {
                json!({
                    "id": id,
                    "type": "function",
                    "function": { "name": name, "arguments": arguments },
                })
            })
            .collect();
        map.insert("tool_calls".to_owned(), Value::Array(wired));
    }
    Ok(json!({
        "choices": [{ "message": message, "finish_reason": finish }],
        "usage": usage.unwrap_or_else(|| json!({})),
    }))
}

/// This dialect writes every level in one field, `none` included.
fn effort_field(effort: Effort) -> Result<&'static str, AxError> {
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

pub(crate) fn request(req: &ChatRequest) -> Result<Value, AxError> {
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
            json!({ "effort": effort_field(effort)? }),
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

pub(crate) fn response_from(wire: &Value) -> Result<ChatResponse, AxError> {
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

pub(crate) fn response_wire(resp: &ChatResponse) -> Result<Value, AxError> {
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
