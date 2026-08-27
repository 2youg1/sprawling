// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! Scripted model: the second adapter of the model seam. Deterministic by
//! construction — the script is the entire behavior; an exhausted script
//! answers with an empty wave, which the executor reads as conclusion.
//!
//! S3.14 puts the translation face into the loop. A scripted turn is
//! written as provider wire JSON and parsed by `gateway::dialect`, the
//! same function the real endpoint uses, so the simulator exercises the
//! translation rather than stepping around it. What stays out is the
//! HTTP face: a network call is not deterministic, and its evidence
//! lives in gateway's own loopback tests.

use std::collections::VecDeque;

use gateway::response_from_wire;
use kernel::{AxError, DialectKind, Model, ModelRequest, ModelReturn, Payload};
use serde_json::Value;

pub struct ScriptModel {
    script: VecDeque<ModelReturn>,
}

impl ScriptModel {
    pub fn new(script: Vec<ModelReturn>) -> Self {
        ScriptModel {
            script: script.into(),
        }
    }

    /// The wire-form script: each entry is what a provider would have
    /// sent, parsed through the production translation.
    pub fn from_wire(kind: DialectKind, script: Vec<Value>) -> Result<Self, AxError> {
        let mut returns = Vec::new();
        for wire in &script {
            let response = response_from_wire(kind, wire)?;
            returns.push(ModelReturn::from_response(response, None)?);
        }
        Ok(ScriptModel {
            script: returns.into(),
        })
    }

    /// An empty-script model: concludes on the first call.
    pub fn silent() -> Self {
        ScriptModel {
            script: VecDeque::new(),
        }
    }
}

impl Model for ScriptModel {
    fn call(&mut self, _req: &ModelRequest) -> Result<ModelReturn, AxError> {
        Ok(self
            .script
            .pop_front()
            .unwrap_or_else(|| ModelReturn::bare(Payload::empty(), Vec::new())))
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
    use kernel::{B3Hash, BuildingPolicy};

    fn req() -> ModelRequest {
        ModelRequest {
            policy: BuildingPolicy::default(),
            segments: [B3Hash::digest(b"x"); 4],
            chat: kernel::ChatRequest::empty("script", 64),
        }
    }

    #[test]
    fn pops_in_script_order_then_concludes() {
        let mut model = ScriptModel::new(vec![ModelReturn {
            usage: None,
            stop: None,
            billed_usd_micros: None,
            message: Payload::empty(),
            calls: vec![kernel::ToolCall {
                id: "call-1".to_owned(),
                name: kernel::ToolName::parse("probe").unwrap(),
                args: Payload::empty(),
            }],
        }]);
        assert_eq!(model.call(&req()).unwrap().calls.len(), 1);
        assert!(model.call(&req()).unwrap().calls.is_empty());
        assert!(model.call(&req()).unwrap().calls.is_empty());
    }

    #[test]
    fn a_wire_script_reaches_the_seam_through_the_real_translation() {
        let mut model = ScriptModel::from_wire(
            DialectKind::Anthropic,
            vec![serde_json::json!({
                "content": [
                    { "type": "text", "text": "editing now" },
                    { "type": "tool_use", "id": "call-1", "name": "edit",
                      "input": { "path": "work/a.txt" } },
                ],
                "stop_reason": "tool_use",
                "usage": { "input_tokens": 12, "output_tokens": 5 },
            })],
        )
        .unwrap();
        let returned = model.call(&req()).unwrap();
        assert_eq!(returned.calls.len(), 1);
        assert_eq!(returned.calls[0].name.as_str(), "edit");
        assert_eq!(returned.calls[0].id, "call-1");
        // usage survived the translation: the account can bill it.
        assert!(returned.usage.is_some());
    }

    #[test]
    fn passes_the_model_conformance_suite() {
        let mut model = ScriptModel::silent();
        kernel::model_conformance::assert_model_conformance(&mut model, &req());
    }
}
