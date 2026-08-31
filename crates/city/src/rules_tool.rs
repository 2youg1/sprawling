// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The face a building's rules show a model: read them, and propose the
//! whole of them again.
//!
//! **Why this is not `edit`.** `BUILDING.md` lives in the building's
//! reserved subtree, and no write domain reaches there — which is not an
//! oversight to work around but the rule itself: a run may not quietly
//! widen what it is allowed to do. The way through is a door of its own,
//! `Effect::Govern`, which always asks the person and shows them the
//! proposal.
//!
//! **Whole document, not a patch.** These rules are evaluated as one
//! text — a confidential building may list no egress domains, so two
//! lines can be legal apart and illegal together. A proposal is
//! therefore the complete file, evaluated before a byte is written, and
//! a file that does not evaluate is refused rather than half-applied.

use std::path::{Path, PathBuf};

use kernel::{
    Address, AxCode, AxError, CostTier, Effect, Payload, RenderIntent, Temporal, Tool, ToolCall,
    ToolMeta, ToolName, ToolOutcome,
};
use serde_json::{Map, Value};

use crate::policy::{BUILDING_FILE, building_path, write_rules};

/// The tool. It holds where the city is and which building the calling
/// run belongs to, so a run cannot rewrite another building's rules by
/// naming one.
pub struct RulesTool {
    city_root: PathBuf,
    building: Address,
    meta: ToolMeta,
}

impl RulesTool {
    /// # Errors
    /// Propagates a malformed parameter schema, which is a build-time
    /// defect rather than a runtime one.
    pub fn new(city_root: &Path, building: Address) -> Result<RulesTool, AxError> {
        let mut properties = Map::new();
        for (field, description) in [
            (
                "op",
                "`read` for the rules as they stand, `propose` to replace them",
            ),
            (
                "text",
                "for propose: the whole of the new BUILDING.md. It is evaluated before anything \
                 is written, and a document that does not evaluate is refused",
            ),
        ] {
            let mut spec = Map::new();
            spec.insert("type".to_owned(), Value::String("string".to_owned()));
            spec.insert(
                "description".to_owned(),
                Value::String(description.to_owned()),
            );
            properties.insert(field.to_owned(), Value::Object(spec));
        }
        let mut params = Map::new();
        params.insert("type".to_owned(), Value::String("object".to_owned()));
        params.insert("properties".to_owned(), Value::Object(properties));
        params.insert(
            "required".to_owned(),
            Value::Array(vec![Value::String("op".to_owned())]),
        );
        Ok(RulesTool {
            city_root: city_root.to_path_buf(),
            building,
            meta: ToolMeta {
                name: ToolName::parse("rules")?,
                disclosure: format!(
                    "Read this building's {BUILDING_FILE}, or propose the whole of a new one. \
                     A proposal is evaluated first and goes to the person before it is written."
                ),
                params: Payload::new(params)?,
                effect: Effect::Govern,
                cost_tier: CostTier::Light,
                timeout: None,
                render: RenderIntent::Generic,
                temporal: Temporal::Timeless,
            },
        })
    }
}

/// The two things this tool does. Exhaustive: an unknown verb is refused
/// rather than read as the harmless one, because guessing wrong here
/// means either a silent read where a rewrite was meant or the reverse.
enum Op {
    Read,
    Propose,
}

impl Op {
    fn parse(raw: &str) -> Result<Op, AxError> {
        match raw {
            "read" => Ok(Op::Read),
            "propose" => Ok(Op::Propose),
            other => Err(AxError::failure(
                AxCode::InvalidArgs,
                "read or change a building's rules",
                format!("no such operation: {other}"),
            )
            .with_recovery("`read`, or `propose` with the whole new document in `text`")),
        }
    }
}

fn arg<'a>(args: &'a Map<String, Value>, key: &str) -> Result<&'a str, AxError> {
    args.get(key).and_then(Value::as_str).ok_or_else(|| {
        AxError::failure(
            AxCode::InvalidArgs,
            "read or change a building's rules",
            format!("missing string argument `{key}`"),
        )
        .with_recovery("`read`, or `propose` with the whole new document in `text`")
    })
}

impl Tool for RulesTool {
    fn meta(&self) -> &ToolMeta {
        &self.meta
    }

    fn invoke(&mut self, call: &ToolCall) -> Result<ToolOutcome, AxError> {
        if call.name != self.meta.name {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "read or change a building's rules",
                format!("call routed to the wrong tool: {}", call.name.as_str()),
            ));
        }
        let args = call.args.as_map();
        let mut out = Map::new();
        out.insert(
            "scope".to_owned(),
            Value::String(self.building.as_str().to_owned()),
        );
        match Op::parse(arg(args, "op")?)? {
            Op::Read => {
                let path = building_path(&self.city_root, &self.building);
                let text = std::fs::read_to_string(&path).unwrap_or_default();
                out.insert("text".to_owned(), Value::String(text));
            }
            Op::Propose => {
                let rules = write_rules(&self.city_root, &self.building, arg(args, "text")?)?;
                out.insert(
                    "confidential".to_owned(),
                    Value::Bool(rules.policy().confidential),
                );
                out.insert("review".to_owned(), Value::Bool(rules.review()));
                out.insert(
                    "takes_effect".to_owned(),
                    Value::String("on the next run in this building".to_owned()),
                );
            }
        }
        Ok(ToolOutcome {
            result: Payload::new(out)?,
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

    fn call(op: &str, text: Option<&str>) -> ToolCall {
        let mut args = Map::new();
        args.insert("op".to_owned(), Value::String(op.to_owned()));
        if let Some(text) = text {
            args.insert("text".to_owned(), Value::String(text.to_owned()));
        }
        ToolCall {
            id: "c1".to_owned(),
            name: ToolName::parse("rules").unwrap(),
            args: Payload::new(args).unwrap(),
        }
    }

    fn tool(root: &Path) -> RulesTool {
        RulesTool::new(root, Address::parse("lab").unwrap()).unwrap()
    }

    #[test]
    fn a_proposal_that_evaluates_becomes_the_rules_the_next_run_is_judged_by() {
        let dir = tempfile::tempdir().unwrap();
        let mut tool = tool(dir.path());
        let outcome = tool
            .invoke(&call(
                "propose",
                Some("# lab\n\nconfidential: false\nreview: true\n\n## Write domain\n\n- lab\n"),
            ))
            .unwrap();
        assert_eq!(outcome.result.as_map()["review"], true);

        let reloaded = crate::policy::load(dir.path(), &Address::parse("lab").unwrap()).unwrap();
        assert!(reloaded.review());
        let read_back = tool.invoke(&call("read", None)).unwrap();
        assert!(
            read_back.result.as_map()["text"]
                .as_str()
                .unwrap()
                .contains("review: true")
        );
    }

    /// The reason the whole document is the unit: two lines legal apart
    /// and illegal together. Nothing is written when they are.
    #[test]
    fn a_proposal_that_does_not_evaluate_leaves_the_old_rules_standing() {
        let dir = tempfile::tempdir().unwrap();
        let mut tool = tool(dir.path());
        tool.invoke(&call("propose", Some("confidential: false\n")))
            .unwrap();

        let err = tool
            .invoke(&call(
                "propose",
                Some("confidential: true\n\n## Egress\n\n- example.com\n"),
            ))
            .unwrap_err();
        assert_eq!(err.code(), &AxCode::ConfigInvalid);

        let standing = crate::policy::load(dir.path(), &Address::parse("lab").unwrap()).unwrap();
        assert!(
            !standing.policy().confidential,
            "a refused proposal changed the building anyway"
        );
    }

    #[test]
    fn a_document_that_does_not_say_whether_it_is_confidential_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        let mut tool = tool(dir.path());
        let err = tool
            .invoke(&call("propose", Some("# lab\n\nreview: false\n")))
            .unwrap_err();
        assert_eq!(err.code(), &AxCode::ConfigInvalid);
        assert!(!building_path(dir.path(), &Address::parse("lab").unwrap()).exists());
    }

    #[test]
    fn an_unknown_verb_is_refused_and_the_tool_still_answers() {
        let dir = tempfile::tempdir().unwrap();
        let mut tool = tool(dir.path());
        assert!(tool.invoke(&call("delete", None)).is_err());
        let mut wrong = call("read", None);
        wrong.name = ToolName::parse("status").unwrap();
        assert!(tool.invoke(&wrong).is_err());
        assert_eq!(tool.meta().name.as_str(), "rules");
    }
}
