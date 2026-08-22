// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Scripted tools: the second adapter of the tool seam. Every failure
//! mode is injectable — an outcome script may hold typed errors, and an
//! exhausted script is itself a failure (E_TOOL_UNAVAILABLE).

use std::collections::{BTreeMap, VecDeque};

use kernel::{AxCode, AxError, Tool, ToolCall, ToolMeta, ToolOutcome};

pub struct ScriptTool {
    meta: ToolMeta,
    outcomes: VecDeque<Result<ToolOutcome, AxError>>,
}

impl ScriptTool {
    pub fn new(meta: ToolMeta, outcomes: Vec<Result<ToolOutcome, AxError>>) -> Self {
        ScriptTool {
            meta,
            outcomes: outcomes.into(),
        }
    }
}

impl Tool for ScriptTool {
    fn meta(&self) -> &ToolMeta {
        &self.meta
    }

    fn invoke(&mut self, call: &ToolCall) -> Result<ToolOutcome, AxError> {
        if call.name != self.meta.name {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "invoke scripted tool",
                call.name.to_string(),
            )
            .with_nearby(vec![self.meta.name.to_string()])
            .with_recovery("call this tool by its registered name"));
        }
        self.outcomes.pop_front().unwrap_or_else(|| {
            Err(AxError::failure(
                AxCode::ToolUnavailable,
                "invoke scripted tool",
                call.name.to_string(),
            )
            .with_recovery("the outcome script is exhausted; extend the scenario"))
        })
    }
}

/// Name-routed set of scripted tools; the executor's dispatch face.
pub struct ScriptToolSet {
    tools: BTreeMap<String, ScriptTool>,
}

impl ScriptToolSet {
    pub fn new(tools: Vec<ScriptTool>) -> Self {
        ScriptToolSet {
            tools: tools
                .into_iter()
                .map(|tool| (tool.meta.name.to_string(), tool))
                .collect(),
        }
    }

    pub fn empty() -> Self {
        ScriptToolSet {
            tools: BTreeMap::new(),
        }
    }

    pub fn meta_of(&self, name: &str) -> Option<&ToolMeta> {
        self.tools.get(name).map(|tool| &tool.meta)
    }

    pub fn invoke(&mut self, call: &ToolCall) -> Result<ToolOutcome, AxError> {
        let names: Vec<String> = self.tools.keys().cloned().collect();
        match self.tools.get_mut(call.name.as_str()) {
            Some(tool) => tool.invoke(call),
            None => Err(AxError::failure(
                AxCode::ToolUnknown,
                "invoke scripted tool",
                call.name.to_string(),
            )
            .with_nearby(names)
            .with_recovery("a tool not in the list does not exist; use a registered one")),
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
    use kernel::{CostTier, Effect, Payload, RenderIntent, Temporal, ToolName};

    fn meta(name: &str) -> ToolMeta {
        ToolMeta {
            name: ToolName::parse(name).unwrap(),
            disclosure: "scripted probe; call it when the scenario says so".into(),
            params: Payload::empty(),
            effect: Effect::Read,
            cost_tier: CostTier::Free,
            timeout: None,
            render: RenderIntent::Generic,
            temporal: Temporal::Timeless,
        }
    }

    fn call(name: &str) -> ToolCall {
        ToolCall {
            id: "call-t".to_owned(),
            name: ToolName::parse(name).unwrap(),
            args: Payload::empty(),
        }
    }

    #[test]
    fn routes_by_name_and_fails_closed_on_unknown() {
        let mut set = ScriptToolSet::new(vec![ScriptTool::new(
            meta("probe"),
            vec![Ok(ToolOutcome {
                result: Payload::empty(),
            })],
        )]);
        assert!(set.invoke(&call("probe")).is_ok());
        let err = set.invoke(&call("ghost")).unwrap_err();
        assert_eq!(err.code(), &AxCode::ToolUnknown);
        assert_eq!(err.nearby(), ["probe"]);
    }

    #[test]
    fn exhausted_script_is_a_typed_failure() {
        let mut set = ScriptToolSet::new(vec![ScriptTool::new(meta("probe"), vec![])]);
        let err = set.invoke(&call("probe")).unwrap_err();
        assert_eq!(err.code(), &AxCode::ToolUnavailable);
    }

    #[test]
    fn passes_the_tool_conformance_suite() {
        let mut tool = ScriptTool::new(
            meta("probe"),
            vec![Ok(ToolOutcome {
                result: Payload::empty(),
            })],
        );
        kernel::tool_conformance::assert_tool_conformance(&mut tool);
    }
}
