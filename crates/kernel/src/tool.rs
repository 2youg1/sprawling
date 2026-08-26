// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The tool seam (seam registry ARCHITECTURE 3).
//! `ToolMeta` is the eight-field registration: a tool without a complete
//! meta is invisible to the model. `effect` decides which gate a call
//! passes; `temporal` decides whether its envelope carries a clock stamp.

use serde::{Deserialize, Serialize};

use crate::address::Address;
use crate::error::{AxCode, AxError};
use crate::event::Payload;

/// Tool identity as it appears in catalog and events: non-empty ASCII
/// lowercase, digits, underscore.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct ToolName(String);

impl ToolName {
    pub fn parse(raw: &str) -> Result<Self, AxError> {
        let well_formed = !raw.is_empty()
            && raw
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_');
        if well_formed {
            Ok(ToolName(raw.to_owned()))
        } else {
            Err(
                AxError::failure(AxCode::InvalidArgs, "parse tool name", raw)
                    .with_recovery("use ascii lowercase, digits and underscore, non-empty"),
            )
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ToolName {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

/// How one external server is named inside this city.
///
/// The label is the first segment of every tool that server offers
/// (`{label}_{tool}`), so two servers that both offer `search` stay two
/// tools rather than becoming one that sometimes does the wrong thing.
/// Its grammar is [`ToolName`]'s minus the underscore: allowing one
/// would let `apps_foo_bar` be read as two different splits, and this
/// name routes a call.
///
/// It lives here rather than beside the protocol that uses it because
/// the rule it enforces is a rule about tool names, and a rule written
/// in two crates is a rule with two authorities.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct ServerLabel(String);

impl ServerLabel {
    /// Sole constructor.
    ///
    /// # Errors
    /// Refuses anything a tool name cannot contain, and the underscore
    /// that would make the resulting tool name ambiguous.
    pub fn parse(raw: &str) -> Result<ServerLabel, AxError> {
        let well_formed = !raw.is_empty()
            && raw
                .chars()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit());
        if well_formed {
            Ok(ServerLabel(raw.to_owned()))
        } else {
            Err(
                AxError::failure(AxCode::InvalidArgs, "name an mcp server", raw.to_owned())
                    .with_recovery("use ascii lowercase and digits, non-empty"),
            )
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for ServerLabel {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for ServerLabel {
    type Error = AxError;

    fn try_from(raw: String) -> Result<ServerLabel, AxError> {
        ServerLabel::parse(&raw)
    }
}

impl From<ServerLabel> for String {
    fn from(label: ServerLabel) -> String {
        label.0
    }
}

impl Serialize for ToolName {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for ToolName {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        ToolName::parse(&raw).map_err(serde::de::Error::custom)
    }
}

/// Declaring a timeout is a promise of cooperative cancellation; absence
/// means no deadline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TimeoutMs(pub u64);

/// What kind of boundary a call crosses — this field routes the call to
/// its gate; it is machine input, not documentation.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    Read,
    Write {
        domain: Address,
    },
    /// Leaves this machine for a destination the call names.
    Egress,
    /// Leaves this process for an external server the building
    /// configured. The destination is fixed by the registration rather
    /// than named per call, because every call to one connector tool
    /// goes to the same place — and a model filling in a `host`
    /// argument would be inventing a fact the city already knows.
    Connector {
        label: ServerLabel,
    },
    /// Starts a second agent on part of this work. Its own class rather
    /// than a `Read`: what a spawn costs and what it can reach is not
    /// bounded by anything the calling run's other gates check, and the
    /// only thing that bounds it is the person.
    Spawn,
    /// Changes what governs a scope - the rules its own runs are judged
    /// by. Deliberately not a `Write`: the reserved subtree is outside
    /// every write domain, so the write door would refuse it, and the
    /// reason it refuses is that this decision is the person's.
    Govern,
    Spend,
}

/// Whether "now" is load-bearing for this tool's results (4.3).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Temporal {
    Timeless,
    Timestamped,
}

/// Cost bucket for budget and routing; consumers arrive in S3.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CostTier {
    Free,
    Light,
    Heavy,
}

/// Presentation intent; per-call `locations` are a pure function of args
/// (tool side, S3). Meta-level declarations use an empty list.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderIntent {
    Generic,
    Terminal,
    Diff { locations: Vec<Address> },
}

/// The eight-field registration, none optional in spirit: `timeout: None`
/// is itself a declaration (no deadline), not an omission.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolMeta {
    pub name: ToolName,
    pub disclosure: String,
    pub params: Payload,
    pub effect: Effect,
    pub cost_tier: CostTier,
    pub timeout: Option<TimeoutMs>,
    pub render: RenderIntent,
    pub temporal: Temporal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolCall {
    /// Wire identity: `tool_use` and `tool_result` pair by this id on
    /// both dialects. Scripted adapters synthesize deterministic ids.
    pub id: String,
    pub name: ToolName,
    pub args: Payload,
}

impl ToolCall {
    /// The bytes that say what this call does, for `IdemKey::derive` to
    /// take as its `action_canonical`.
    ///
    /// The name and the arguments together are the action. Two calls of
    /// one tool with different arguments are two actions, and a key that
    /// read only the name made them one — which the deduplication then
    /// reported to the model as a call it had already made. `id` stays
    /// out: two calls differing only by wire id are the same action.
    ///
    /// # Errors
    /// Propagates arguments that do not serialise. `Payload` keys are
    /// strings and its values carry no floats, so this arm is out of
    /// reach today; reporting it is still what keeps the key honest if
    /// that ever stops being true, because the empty string this used to
    /// substitute would collide two different calls into one key.
    pub fn action(&self) -> Result<Vec<u8>, AxError> {
        let mut action = self.name.as_str().as_bytes().to_vec();
        let args = serde_json::to_string(&self.args).map_err(|err| {
            AxError::failure(
                AxCode::InvalidArgs,
                "read a tool call's action",
                err.to_string(),
            )
        })?;
        action.extend_from_slice(args.as_bytes());
        Ok(action)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ToolOutcome {
    pub result: Payload,
}

/// The tool port. Adapters: runtime L0 three (S3), browser, protocol;
/// second adapter: citysim scripted tools (S2.03).
pub trait Tool {
    fn meta(&self) -> &ToolMeta;

    /// Fail-closed identity: a call whose name differs from `meta().name`
    /// must return `E_INVALID_ARGS`, never route silently.
    fn invoke(&mut self, call: &ToolCall) -> Result<ToolOutcome, AxError>;
}

/// The exec three-arm shape (L0 frozen surface, 5.1). Lives on the tool
/// face because `discard::forecast` consumes it ahead of the S3 exec tool.
/// Exactly three arms — deliberately exhaustive.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecArm {
    Program { path: String, args: Vec<String> },
    Python { code: String },
    Shell { text: String },
}

#[cfg(feature = "conformance")]
pub mod conformance {
    //! One assertion suite for every tool implementation (V3).

    use super::{Tool, ToolCall, ToolName};
    use crate::error::AxCode;
    use crate::event::Payload;

    /// Asserts the meta is complete and the identity is fail-closed:
    /// a wrong-name call is refused with `E_INVALID_ARGS`, and the tool
    /// still answers after refusing (no poisoned state).
    #[allow(
        clippy::panic,
        clippy::expect_used,
        reason = "conformance suites assert by panicking; they are dev-only by feature"
    )]
    pub fn assert_tool_conformance<T: Tool>(tool: &mut T) {
        let meta = tool.meta().clone();
        assert!(
            !meta.name.as_str().is_empty(),
            "tool name must be non-empty"
        );
        assert!(
            !meta.disclosure.is_empty(),
            "disclosure must answer what-and-when in one breath"
        );
        let wrong = ToolName::parse("no_such_tool_name").expect("literal is well-formed");
        assert_ne!(
            wrong, meta.name,
            "conformance probe name collides with the tool under test"
        );
        let refused = tool.invoke(&ToolCall {
            id: "conf-mismatch".to_owned(),
            name: wrong,
            args: Payload::empty(),
        });
        match refused {
            Err(err) => assert_eq!(
                err.code(),
                &AxCode::InvalidArgs,
                "wrong-name call must be E_INVALID_ARGS"
            ),
            Ok(_) => panic!("a tool must refuse a call bearing another tool's name"),
        }
        let meta_after = tool.meta();
        assert_eq!(
            meta_after.name, meta.name,
            "a refusal must not poison the tool"
        );
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
    fn tool_name_grammar_is_fail_closed() {
        assert!(ToolName::parse("exec").is_ok());
        assert!(ToolName::parse("l2_search_v2").is_ok());
        for bad in ["", "Exec", "with space", "dash-ed", "cjk工具", "UPPER"] {
            assert!(ToolName::parse(bad).is_err(), "{bad:?} must be rejected");
        }
    }

    #[test]
    fn a_label_that_could_not_start_a_tool_name_is_refused() {
        assert_eq!(ServerLabel::parse("apps").unwrap().as_str(), "apps");
        assert!(ServerLabel::parse("mail2").is_ok());
        for bad in ["", "Apps", "app-s", "应用", "with space"] {
            assert!(ServerLabel::parse(bad).is_err(), "{bad:?} must be rejected");
        }
        assert!(
            ServerLabel::parse("app_s").is_err(),
            "an underscore would make `app_s_send` two different splits of one name"
        );
    }

    #[test]
    fn a_label_round_trips_through_its_serialised_form() {
        let label = ServerLabel::parse("apps").unwrap();
        let text = serde_json::to_string(&label).unwrap();
        assert_eq!(text, "\"apps\"");
        assert_eq!(serde_json::from_str::<ServerLabel>(&text).unwrap(), label);
        assert!(
            serde_json::from_str::<ServerLabel>("\"Apps\"").is_err(),
            "the grammar holds on the way in, or a file could mint one that parse refuses"
        );
    }

    #[test]
    fn effect_serde_names_are_snake_case() {
        let write = Effect::Write {
            domain: Address::parse("b/room").unwrap(),
        };
        let json = serde_json::to_string(&write).unwrap();
        assert_eq!(json, "{\"write\":{\"domain\":\"b/room\"}}");
        assert_eq!(
            serde_json::to_string(&Temporal::Timestamped).unwrap(),
            "\"timestamped\""
        );
    }

    #[test]
    fn exec_arm_shapes_serialize_distinctly() {
        let program = ExecArm::Program {
            path: "git".into(),
            args: vec!["status".into()],
        };
        let json = serde_json::to_string(&program).unwrap();
        assert!(json.contains("\"program\""));
        let python = ExecArm::Python {
            code: "print(1)".into(),
        };
        assert!(
            serde_json::to_string(&python)
                .unwrap()
                .contains("\"python\"")
        );
    }

    #[test]
    fn meta_holds_all_eight_fields() {
        let meta = ToolMeta {
            name: ToolName::parse("status").unwrap(),
            disclosure: "reports run state; call it when you need now/usage/signals".into(),
            params: Payload::empty(),
            effect: Effect::Read,
            cost_tier: CostTier::Free,
            timeout: Some(TimeoutMs(1000)),
            render: RenderIntent::Generic,
            temporal: Temporal::Timestamped,
        };
        let json = serde_json::to_value(&meta).unwrap();
        let keys: std::collections::BTreeSet<String> =
            json.as_object().unwrap().keys().cloned().collect();
        let expected: std::collections::BTreeSet<String> = [
            "name",
            "disclosure",
            "params",
            "effect",
            "cost_tier",
            "timeout",
            "render",
            "temporal",
        ]
        .into_iter()
        .map(String::from)
        .collect();
        assert_eq!(keys, expected, "all eight fields must be present on wire");
    }
}
