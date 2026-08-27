// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! The status tool: the model's view of its own situation, in the
//! thirteen frozen fields, in that order.
//!
//! The order is not cosmetic. A model reading its status reads the top
//! first, so identity and mode come before budget, and budget comes
//! before the long tail of locks and children. Freezing the order means
//! a Resident's habits transfer across versions instead of being
//! relearned each time the field list grows - which is why the
//! thirteenth field went on the end rather than beside the twelfth it
//! belongs with.
//!
//! Nothing here samples. The executor sets the snapshot once per turn
//! and the tool reports it; a tool that read the clock itself would
//! make two calls in one turn disagree about "now".

use kernel::{
    Address, AxCode, AxError, ByteLen, CostTier, DelegateKind, Effect, Payload, RenderIntent,
    Temporal, Tokens, Tool, ToolCall, ToolMeta, ToolName, ToolOutcome, UsdMicros,
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

/// One piece of work this run handed down.
///
/// Two fields, because two facts exist. A child starts after the run
/// that asked for it has frozen, so while that run is still reading its
/// own status the child has no id, no phase and no context reading - and
/// a field that can only ever hold zero says "zero" where the truth is
/// "not yet".
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChildStatus {
    pub room: Address,
    pub kind: DelegateKind,
}

/// The thirteen fields, in the frozen order.
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
    pub now: Option<ClockStamp>,
    pub provider_mode: ProviderMode,
    /// How many residents this run can reach, itself excluded. A count
    /// rather than a list: the list grows with the building's population
    /// and this text does not, so the names live behind the `neighbours`
    /// tool and what stands here is whether asking is worth a call.
    ///
    /// People, not places. An empty room takes messages nobody reads, so
    /// counting one would make `neighbours: 1` mean the opposite of what
    /// it says.
    pub neighbours: u32,
}

pub struct StatusTool {
    snapshot: StatusSnapshot,
    children: Box<dyn Fn() -> Vec<ChildStatus>>,
    meta: ToolMeta,
}

impl StatusTool {
    /// A run that hands nothing down.
    ///
    /// # Errors
    /// Propagates a malformed parameter schema.
    pub fn new(snapshot: StatusSnapshot) -> Result<StatusTool, AxError> {
        StatusTool::watching(snapshot, Box::new(Vec::new))
    }

    /// A run whose delegate desk is asked every time the model calls
    /// `status`.
    ///
    /// A closure rather than a field, because delegation happens after
    /// this tool is built: a snapshot taken before the run started is
    /// empty for the whole run. A closure rather than a seam, because
    /// the desk lives in `collab` and this crate may not depend on it -
    /// the assembly layer owns both ends and hands one to the other.
    ///
    /// # Errors
    /// Propagates a malformed parameter schema.
    pub fn watching(
        snapshot: StatusSnapshot,
        children: Box<dyn Fn() -> Vec<ChildStatus>>,
    ) -> Result<StatusTool, AxError> {
        let mut params = Map::new();
        params.insert("type".to_owned(), Value::String("object".to_owned()));
        params.insert("properties".to_owned(), Value::Object(Map::new()));
        Ok(StatusTool {
            snapshot,
            children,
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
    pub fn render(&self, children: &[ChildStatus]) -> String {
        let locks = if self.locks.is_empty() {
            "none".to_owned()
        } else {
            self.locks.join(", ")
        };
        let children = render_children(children);
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
            format!("neighbours: {}", self.neighbours),
        ]
        .join("\n")
    }
}

/// Where each piece of handed-down work went, or the word for none.
/// One line, because `status` is read as lines and a list that wrapped
/// would break the field order the whole tool exists to keep.
fn render_children(children: &[ChildStatus]) -> String {
    if children.is_empty() {
        return "none".to_owned();
    }
    children
        .iter()
        .map(|child| format!("{} ({})", child.room, child.kind.as_str()))
        .collect::<Vec<String>>()
        .join("; ")
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
        result.insert(
            "text".to_owned(),
            Value::String(self.snapshot.render(&(self.children)())),
        );
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
            now: None,
            provider_mode: ProviderMode::Normal,
            neighbours: 3,
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
    fn the_thirteen_fields_report_in_the_frozen_order() {
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
            "neighbours",
        ];
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(
            lines.len(),
            13,
            "the frozen list is thirteen fields: {text}"
        );
        for (line, field) in lines.iter().zip(order) {
            assert!(
                line.starts_with(&format!("{field}:")),
                "expected {field}, got {line}"
            );
        }
    }

    /// `children` was a hardcoded empty list, so a run that had just
    /// handed work down and then asked about its own situation was told
    /// it had handed nothing down.
    #[test]
    fn the_children_line_says_where_the_work_went_and_which_kind_of_delegate() {
        let handed = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
        let seen = std::rc::Rc::clone(&handed);
        let mut tool =
            StatusTool::watching(snapshot(), Box::new(move || seen.borrow().clone())).unwrap();

        let before = serde_json::to_value(&tool.invoke(&call()).unwrap().result).unwrap();
        assert!(before["text"].as_str().unwrap().contains("children: none"));

        handed.borrow_mut().push(ChildStatus {
            room: Address::parse("work/helper").unwrap(),
            kind: DelegateKind::Ephemeral,
        });
        let after = serde_json::to_value(&tool.invoke(&call()).unwrap().result).unwrap();
        assert!(
            after["text"]
                .as_str()
                .unwrap()
                .contains("children: work/helper (ephemeral)"),
            "the desk is asked at call time, not frozen with the tool: {after}"
        );
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
