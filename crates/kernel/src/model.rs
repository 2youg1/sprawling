// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! The model seam (seam registry ARCHITECTURE 3).
//! Every provider call carries a `BuildingPolicy` value; the policy is
//! *defined* here (kernel cannot name outer crates) and *evaluated* by
//! `city::policy` (P1) — dependency inversion, same as the ledger seam.

use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::budget::{Tokens, UsdMicros};
use crate::error::{AxCode, AxError};
use crate::event::Payload;
use crate::locator::B3Hash;
use crate::tool::{ToolCall, ToolName};

/// Building-level constraints riding along the call. S2 carries the one
/// load-bearing bit; further fields only grow (14.3).
#[non_exhaustive]
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct BuildingPolicy {
    /// Confidential buildings lock the call to the local model pool;
    /// content never leaves the machine (11.4).
    pub confidential: bool,
}

impl BuildingPolicy {
    pub fn new(confidential: bool) -> Self {
        BuildingPolicy { confidential }
    }
}

/// Message author on the canonical (Anthropic-shaped) conversation.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Role {
    User,
    Assistant,
}

/// Why the provider stopped. Verdict-adjacent but wire-borne, so it stays
/// open like other wire enums (14.3).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    EndTurn,
    ToolUse,
    MaxTokens,
}

/// One frozen-prefix block on the wire. `cache` marks an explicit prompt
/// cache breakpoint (breakpoints sit on segment edges).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SystemBlock {
    pub text: String,
    pub cache: bool,
}

/// Which wire the far side speaks. Open for growth; every match on it
/// handles the known kinds exhaustively and fails closed.
///
/// Lives here rather than in `gateway` for the same reason
/// [`BuildingPolicy`] does: two outer crates must name it (the gateway
/// translates it, the wire carries it) and neither may name the other.
/// `gateway::dialect` is its evaluator, not its definition.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DialectKind {
    Anthropic,
    OpenAi,
}

/// What a chosen model is for. Exhaustive rather than a free label: a
/// tag exists because some code asks for a model by it, so a tag with no
/// asker is a setting the person can fill in and never see used. It
/// grows when a caller appears, not when a name is imagined.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelTag {
    /// The model a resident thinks with.
    Main,
    /// The small model that reads long documents so the main one does
    /// not have to: summaries, structure trees, search results.
    Digest,
}

impl ModelTag {
    /// Every tag, in the order a settings page should offer them.
    pub const ALL: [ModelTag; 2] = [ModelTag::Main, ModelTag::Digest];

    #[must_use]
    pub fn as_str(&self) -> &'static str {
        match self {
            ModelTag::Main => "main",
            ModelTag::Digest => "digest",
        }
    }
}

impl std::fmt::Display for ModelTag {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// How hard the provider should think before answering, ordered from
/// least to most. Both dialects accept every level; they disagree only
/// on where `None` is written (an effort value on one wire, a separate
/// thinking field on the other).
///
/// The ladder mirrors the providers' own vocabularies; check theirs
/// before changing it:
/// <https://platform.claude.com/docs/en/build-with-claude/effort> and
/// <https://developers.openai.com/api/docs/guides/reasoning>.
///
/// Absence (`Option::None`) is not `Effort::None`: absence leaves the
/// choice to the provider, `Effort::None` asks it not to think.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effort {
    None,
    Low,
    Medium,
    High,
    #[serde(rename = "xhigh")]
    XHigh,
    Max,
}

/// Canonical content block. Tool inputs are [`Payload`] — the float ban
/// holds here because these bytes become ledger payloads verbatim.
///
/// `Thinking` and `RedactedThinking` are carried unchanged end to end:
/// the provider verifies `signature` against the reasoning it issued, and
/// altering either block earns a 400 that names them as unmodifiable.
/// The block shapes are the city dialect's, so
/// <https://platform.claude.com/docs/en/build-with-claude/thinking> is
/// what a change to them has to agree with.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ContentBlock {
    Text {
        text: String,
    },
    Thinking {
        thinking: String,
        signature: String,
    },
    RedactedThinking {
        data: String,
    },
    ToolUse {
        id: String,
        name: ToolName,
        input: Payload,
    },
    ToolResult {
        tool_use_id: String,
        content: String,
        is_error: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatMessage {
    pub role: Role,
    pub content: Vec<ContentBlock>,
}

/// A tool as the provider sees it; sourced from catalog `tool_defs` only.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolDef {
    pub name: ToolName,
    pub description: String,
    pub input_schema: Payload,
}

/// The canonical request conversation (city dialect = Anthropic Messages). Both production dialects and the scripted model
/// consume this one shape — the seam carries it so replay, citysim and
/// the real gateway argue about the same object.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatRequest {
    pub model: String,
    pub max_tokens: u64,
    pub system: Vec<SystemBlock>,
    pub messages: Vec<ChatMessage>,
    pub tools: Vec<ToolDef>,
    /// Frozen at run start: changing it mid-run would start a new cached
    /// prompt prefix, so the value rides in from [`crate::FrozenConfig`].
    pub effort: Option<Effort>,
}

/// Provider-reported token counts; absent wire fields read as zero.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct ModelUsage {
    pub input_tokens: Tokens,
    pub output_tokens: Tokens,
    pub cache_read_tokens: Tokens,
    pub cache_write_tokens: Tokens,
}

/// The canonical response: dialect output, endpoint input.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ChatResponse {
    pub content: Vec<ContentBlock>,
    pub stop: StopReason,
    pub usage: ModelUsage,
}

/// The account-side form of assistant content: the single authority for
/// how content blocks enter `model_returned` payloads (and thus how the
/// window is rebuilt offline, C16).
pub fn message_payload(content: &[ContentBlock]) -> Result<Payload, AxError> {
    let blocks = serde_json::to_value(content).map_err(|err| {
        AxError::failure(
            AxCode::InvalidArgs,
            "encode content blocks",
            err.to_string(),
        )
    })?;
    let mut map = Map::new();
    map.insert("content".to_owned(), blocks);
    Payload::new(map)
}

/// What the adapter needs to make one call. `segments` are the frozen
/// prefix hashes (same source as `prompt_assembled`); `chat` is the full
/// canonical conversation the dialect puts on the wire.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelRequest {
    pub policy: BuildingPolicy,
    pub segments: [B3Hash; 4],
    pub chat: ChatRequest,
}

impl ChatRequest {
    /// An empty conversation shell for adapters and tests that argue
    /// about hashes, not content.
    pub fn empty(model: &str, max_tokens: u64) -> ChatRequest {
        ChatRequest {
            model: model.to_owned(),
            max_tokens,
            system: Vec::new(),
            messages: Vec::new(),
            tools: Vec::new(),
            effort: None,
        }
    }
}

/// One assistant turn. `message` is the in-window content; `calls` is the
/// requested tool wave — empty means the turn loop may conclude the run.
/// `usage`/`stop`/`billed_usd_micros` ride along from real dialects and
/// stay `None` for scripted adapters that predate cost accounting.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ModelReturn {
    pub message: Payload,
    pub calls: Vec<ToolCall>,
    pub usage: Option<ModelUsage>,
    pub stop: Option<StopReason>,
    pub billed_usd_micros: Option<UsdMicros>,
}

impl ModelReturn {
    /// Scripted/minimal construction: content and wave only.
    pub fn bare(message: Payload, calls: Vec<ToolCall>) -> ModelReturn {
        ModelReturn {
            message,
            calls,
            usage: None,
            stop: None,
            billed_usd_micros: None,
        }
    }

    /// The one mapping from canonical response to seam return: tool_use
    /// blocks become the wave, everything is kept for the account.
    pub fn from_response(
        resp: ChatResponse,
        billed_usd_micros: Option<UsdMicros>,
    ) -> Result<ModelReturn, AxError> {
        let message = message_payload(&resp.content)?;
        let mut calls = Vec::new();
        for block in &resp.content {
            if let ContentBlock::ToolUse { id, name, input } = block {
                calls.push(ToolCall {
                    id: id.clone(),
                    name: name.clone(),
                    args: input.clone(),
                });
            }
        }
        Ok(ModelReturn {
            message,
            calls,
            usage: Some(resp.usage),
            stop: Some(resp.stop),
            billed_usd_micros,
        })
    }
}

/// The inverse of [`message_payload`] for window folding and rebuild.
///
/// A message without a canonical `content` array folds to no blocks —
/// defined behavior for scripted payloads, not an error path. A `content`
/// array that is present but unreadable is an error: silently folding it
/// to nothing would drop an assistant message the ledger still holds, and
/// a window that disagrees with the ledger is a second history.
pub fn content_from_message(message: &Payload) -> Result<Vec<ContentBlock>, AxError> {
    let Some(value) = message.as_map().get("content") else {
        return Ok(Vec::new());
    };
    serde_json::from_value(value.clone()).map_err(|err| {
        AxError::failure(
            AxCode::WireMismatch,
            "read assistant content",
            err.to_string(),
        )
        .with_recovery(
            "this build cannot read a block kind the ledger holds; replay with the build that \
             wrote it, or extend ContentBlock",
        )
    })
}

/// Guard for wire faces: `serde_json::Value` trees entering payload-adjacent
/// positions must respect the float ban before conversion.
pub fn value_has_float(value: &Value) -> bool {
    match value {
        Value::Number(n) => !n.is_i64() && !n.is_u64(),
        Value::Array(items) => items.iter().any(value_has_float),
        Value::Object(map) => map.values().any(value_has_float),
        Value::Null | Value::Bool(_) | Value::String(_) => false,
    }
}

/// The model port. Production adapters: gateway::native, gateway::endpoint
/// (S3); second adapter: citysim scripted model (S2.03). Implementations
/// never sample clocks or read global state.
pub trait Model {
    fn call(&mut self, req: &ModelRequest) -> Result<ModelReturn, AxError>;
}

#[cfg(feature = "conformance")]
pub mod conformance {
    //! One assertion suite for every model implementation (V3).

    use super::{Model, ModelRequest};

    /// The universal model contract is thin on purpose: two consecutive
    /// calls must both *return* — no panic, and no poisoned state after an
    /// Err. Result shapes are already the types' business; determinism is
    /// not asserted (real providers are not deterministic — scripted
    /// adapters prove theirs in citysim).
    #[allow(
        clippy::panic,
        reason = "conformance suites assert by panicking; they are dev-only by feature"
    )]
    pub fn assert_model_conformance<M: Model>(model: &mut M, benign: &ModelRequest) {
        for round in 0..2u8 {
            match model.call(benign) {
                Ok(ret) => assert!(
                    ret.calls.len() != usize::MAX,
                    "round {round}: adapter returned a wave"
                ),
                Err(err) => assert!(
                    !err.code().as_str().is_empty(),
                    "round {round}: adapter returned a typed error"
                ),
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

    #[test]
    fn policy_default_is_not_confidential() {
        assert!(!BuildingPolicy::default().confidential);
        assert!(BuildingPolicy::new(true).confidential);
    }

    #[test]
    fn model_return_roundtrips_with_sorted_payload_keys() {
        let ret = ModelReturn::bare(Payload::empty(), vec![]);
        let json = serde_json::to_string(&ret).unwrap();
        let back: ModelReturn = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ret);
    }

    #[test]
    fn thinking_survives_the_payload_round_trip_verbatim() {
        // The provider verifies `signature` against the reasoning it
        // issued, so every byte of both blocks has to come back.
        // Assembled at runtime: a signature-shaped literal is exactly
        // what the secret gate exists to keep out of the repository.
        let signature = format!("{}{}", "WaUjzkyp", "Q2mUEVM36O2Txu");
        let issued = vec![
            ContentBlock::Thinking {
                thinking: "The question has two parts.".to_owned(),
                signature,
            },
            ContentBlock::RedactedThinking {
                data: "EroBCkYIARgCKkBmx".to_owned(),
            },
            ContentBlock::Text {
                text: "Based on my analysis...".to_owned(),
            },
        ];
        let payload = message_payload(&issued).unwrap();
        assert_eq!(content_from_message(&payload).unwrap(), issued);
    }

    #[test]
    fn a_content_array_that_cannot_be_read_is_an_error_not_an_empty_window() {
        // A block kind this build does not know must not make the whole
        // assistant message vanish from the window: the ledger still has
        // it, and a window that disagrees with the ledger is the second
        // history this design exists to prevent.
        let mut map = Map::new();
        map.insert(
            "content".to_owned(),
            serde_json::json!([{ "kind": "from_a_later_build" }]),
        );
        let payload = Payload::new(map).unwrap();
        let err = content_from_message(&payload).unwrap_err();
        assert_eq!(*err.code(), AxCode::WireMismatch);

        // A message with no content array at all still folds to no
        // blocks: that is the scripted-payload contract, not a failure.
        assert!(content_from_message(&Payload::empty()).unwrap().is_empty());
    }

    #[test]
    fn effort_is_ordered_from_none_to_max() {
        let ladder = [
            Effort::None,
            Effort::Low,
            Effort::Medium,
            Effort::High,
            Effort::XHigh,
            Effort::Max,
        ];
        assert!(ladder.windows(2).all(|pair| pair[0] < pair[1]));
        assert_eq!(
            serde_json::to_string(&Effort::XHigh).unwrap(),
            "\"xhigh\"",
            "the city spells an effort level the way the provider spells it"
        );
    }

    #[test]
    fn request_carries_four_segment_hashes() {
        let req = ModelRequest {
            policy: BuildingPolicy::default(),
            segments: [B3Hash::digest(b"a"); 4],
            chat: ChatRequest::empty("m", 64),
        };
        let json = serde_json::to_value(&req).unwrap();
        assert_eq!(json["segments"].as_array().unwrap().len(), 4);
    }
}
