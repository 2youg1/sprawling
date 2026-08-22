// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The status tool: the model's view of its own situation, in the
//! twelve frozen fields, in that order.
//!
//! The order is not cosmetic. A model reading its status reads the top
//! first, so identity and mode come before budget, and budget comes
//! before the long tail of locks and children. Freezing the order means
//! a Resident's habits transfer across versions instead of being
//! relearned each time the field list grows.
//!
//! Nothing here samples. The executor sets the snapshot once per turn
//! and the tool reports it; a tool that read the clock itself would
//! make two calls in one turn disagree about "now".

use kernel::{
    Address, AxCode, AxError, ByteLen, CostTier, Effect, Payload, RenderIntent, RunId, Temporal,
    Tokens, Tool, ToolCall, ToolMeta, ToolName, ToolOutcome, UsdMicros,
};
use serde_json::{Map, Value};

use crate::clock::ClockStamp;
use crate::mode::Mode;

/// How the gateway is currently able to serve. Degraded and LocalOnly
/// are situations the model should plan around, so they are reported
/// rather than hidden behind a retry.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ProviderMode {
    Normal,
    Degraded,
    LocalOnly,
}

impl ProviderMode {
    pub fn as_str(self) -> &'static str {
        match self {
            ProviderMode::Normal => "normal",
            ProviderMode::Degraded => "degraded",
            ProviderMode::LocalOnly => "local_only",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildStatus {
    pub run: RunId,
    pub phase: String,
    pub ctx_used: Tokens,
    pub ctx_lock: Tokens,
}

/// The twelve fields, in the frozen order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StatusSnapshot {
    pub who: String,
    pub addr: Address,
    pub mode: Mode,
    pub ctx_used: Tokens,
    pub ctx_limit: Tokens,
    pub budget_usd: UsdMicros,
    pub budget_tokens: Tokens,
    pub trust: String,
    pub write_domain: String,
    pub locks: Vec<String>,
    pub worktree_path: String,
    pub worktree_disk: ByteLen,
    pub signals_pending: u32,
    pub children: Vec<ChildStatus>,
    pub now: Option<ClockStamp>,
    pub provider_mode: ProviderMode,
}

pub struct StatusTool {
    snapshot: StatusSnapshot,
    meta: ToolMeta,
}

impl StatusTool {
    pub fn new(snapshot: StatusSnapshot) -> Result<StatusTool, AxError> {
        let mut params = Map::new();
        params.insert("type".to_owned(), Value::String("object".to_owned()));
        params.insert("properties".to_owned(), Value::Object(Map::new()));
        Ok(StatusTool {
            snapshot,
            meta: ToolMeta {
                name: ToolName::parse("status")?,
                disclosure:
                    "Report your current situation: mode, context, budget, domain, children."
                        .to_owned(),
                params: Payload::new(params)?,
                effect: Effect::Read,
                cost_tier: CostTier::Free,
                timeout: None,
                render: RenderIntent::Generic,
                temporal: Temporal::Timestamped,
            },
        })
    }

    /// The executor's once-per-turn update. Sampling inside the tool
    /// would let two calls in one turn disagree.
    pub fn set_snapshot(&mut self, snapshot: StatusSnapshot) {
        self.snapshot = snapshot;
    }
}

impl StatusSnapshot {
    /// The frozen order, one field per line.
    ///
    /// The result is text rather than a JSON object because a JSON
    /// object has no order a reader can rely on — `serde_json`'s map
    /// sorts its keys, so "the frozen order" would silently become
    /// alphabetical. Order is a property of what the model reads, so it
    /// is expressed where the model reads it.
    pub fn render(&self) -> String {
        let locks = if self.locks.is_empty() {
            "none".to_owned()
        } else {
            self.locks.join(", ")
        };
        let children = if self.children.is_empty() {
            "none".to_owned()
        } else {
            self.children
                .iter()
                .map(|c| {
                    format!(
                        "{} {} ctx {}/{}",
                        c.run,
                        c.phase,
                        c.ctx_used.get(),
                        c.ctx_lock.get()
                    )
                })
                .collect::<Vec<String>>()
                .join("; ")
        };
        let now = match &self.now {
            Some(stamp) => stamp.render(),
            None => "not stamped".to_owned(),
        };
        [
            format!("who: {}", self.who),
            format!("addr: {}", self.addr),
            format!("mode: {}", self.mode.as_str()),
            format!("ctx: {}/{}", self.ctx_used.get(), self.ctx_limit.get()),
            format!(
                "budget: {} usd_micros, {} tokens",
                self.budget_usd.get(),
                self.budget_tokens.get()
            ),
            format!("trust: {}", self.trust),
            format!("write_domain: {} (locks: {locks})", self.write_domain),
            format!(
                "worktree: {} ({} bytes)",
                self.worktree_path,
                self.worktree_disk.get()
            ),
            format!("signals_pending: {}", self.signals_pending),
            format!("children: {children}"),
            format!("now: {now}"),
            format!("provider_mode: {}", self.provider_mode.as_str()),
        ]
        .join("\n")
    }
}

impl Tool for StatusTool {
    fn meta(&self) -> &ToolMeta {
        &self.meta
    }

    fn invoke(&mut self, call: &ToolCall) -> Result<ToolOutcome, AxError> {
        if call.name != self.meta.name {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "read status",
                format!("call routed to the wrong tool: {}", call.name.as_str()),
            ));
        }
        let mut result = Map::new();
        result.insert("text".to_owned(), Value::String(self.snapshot.render()));
        Ok(ToolOutcome {
            result: Payload::new(result)?,
        })
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

    fn snapshot() -> StatusSnapshot {
        StatusSnapshot {
            who: "alice".to_owned(),
            addr: Address::parse("work").unwrap(),
            mode: Mode::Up,
            ctx_used: Tokens::new(1200),
            ctx_limit: Tokens::new(8000),
            budget_usd: UsdMicros::new(250_000),
            budget_tokens: Tokens::new(40_000),
            trust: "trusted".to_owned(),
            write_domain: "work".to_owned(),
            locks: vec!["work/a.txt".to_owned()],
            worktree_path: "/city/work".to_owned(),
            worktree_disk: ByteLen::new(4096),
            signals_pending: 2,
            children: vec![],
            now: None,
            provider_mode: ProviderMode::Normal,
        }
    }

    fn call() -> ToolCall {
        ToolCall {
            id: "s1".to_owned(),
            name: ToolName::parse("status").unwrap(),
            args: Payload::new(Map::new()).unwrap(),
        }
    }

    #[test]
    fn the_twelve_fields_report_in_the_frozen_order() {
        let mut tool = StatusTool::new(snapshot()).unwrap();
        let outcome = tool.invoke(&call()).unwrap();
        let value = serde_json::to_value(&outcome.result).unwrap();
        let text = value["text"].as_str().unwrap();
        let order = [
            "who",
            "addr",
            "mode",
            "ctx",
            "budget",
            "trust",
            "write_domain",
            "worktree",
            "signals_pending",
            "children",
            "now",
            "provider_mode",
        ];
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 12, "the frozen list is twelve fields: {text}");
        for (line, field) in lines.iter().zip(order) {
            assert!(
                line.starts_with(&format!("{field}:")),
                "expected {field}, got {line}"
            );
        }
    }

    #[test]
    fn the_tool_reports_what_it_was_given_and_never_samples() {
        let mut tool = StatusTool::new(snapshot()).unwrap();
        let first = tool.invoke(&call()).unwrap();
        let second = tool.invoke(&call()).unwrap();
        assert_eq!(
            first.result, second.result,
            "two calls, one turn, one answer"
        );

        let mut next = snapshot();
        next.provider_mode = ProviderMode::Degraded;
        next.signals_pending = 0;
        tool.set_snapshot(next);
        let after = serde_json::to_value(&tool.invoke(&call()).unwrap().result).unwrap();
        let text = after["text"].as_str().unwrap();
        assert!(text.contains("provider_mode: degraded"), "{text}");
        assert!(text.contains("signals_pending: 0"), "{text}");
    }

    #[test]
    fn a_call_for_another_tool_is_refused() {
        let mut tool = StatusTool::new(snapshot()).unwrap();
        let mut wrong = call();
        wrong.name = ToolName::parse("edit").unwrap();
        assert_eq!(
            *match tool.invoke(&wrong) {
                Err(err) => err,
                Ok(_) => panic!("identity is fail-closed"),
            }
            .code(),
            AxCode::InvalidArgs
        );
    }
}
