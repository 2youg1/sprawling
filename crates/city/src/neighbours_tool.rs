// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The face the neighbourhood shows a model (city-SPEC.md section
//! 8-15b): this building's addresses, or the city's buildings by name.
//!
//! The first line of every answer says that an address this does not
//! list has no reader. That sentence is the defect this tool was built
//! for, stated where a model reads first: `signal` will queue a message
//! for any address inside the building, whether or not anybody stands
//! there to take it.

use kernel::{
    AxCode, AxError, CostTier, Effect, Payload, RenderIntent, Temporal, Tool, ToolCall, ToolMeta,
    ToolName, ToolOutcome,
};
use serde_json::{Map, Value};

use crate::neighbourhood::{Neighbourhood, Occupancy};

/// The tool. It holds the neighbourhood the run was dispatched into, so
/// a run cannot read another building's residents by naming one.
pub struct NeighboursTool {
    seen: Neighbourhood,
    meta: ToolMeta,
}

impl NeighboursTool {
    /// # Errors
    /// Propagates a malformed tool name or parameter schema, neither of
    /// which can happen with the literals below.
    pub fn new(seen: Neighbourhood) -> Result<NeighboursTool, AxError> {
        let mut scope = Map::new();
        scope.insert("type".to_owned(), Value::String("string".to_owned()));
        scope.insert(
            "description".to_owned(),
            Value::String(
                "`building` for the addresses you can reach and who stands at them, \
                 `city` for the other buildings by name. Defaults to `building`."
                    .to_owned(),
            ),
        );
        let mut properties = Map::new();
        properties.insert("scope".to_owned(), Value::Object(scope));
        let mut params = Map::new();
        params.insert("type".to_owned(), Value::String("object".to_owned()));
        params.insert("properties".to_owned(), Value::Object(properties));
        Ok(NeighboursTool {
            seen,
            meta: ToolMeta {
                name: ToolName::parse("neighbours")?,
                disclosure: "Who else this building has, and what each of them is for; call it \
                             before signalling or delegating to an address you have not been \
                             given."
                    .to_owned(),
                params: Payload::new(params)?,
                effect: Effect::Read,
                cost_tier: CostTier::Free,
                timeout: None,
                render: RenderIntent::Generic,
                temporal: Temporal::Timeless,
            },
        })
    }
}

/// How far the question reaches. The two words are the configuration
/// ladder's own rungs (`Layer::City`, `Layer::Building`) rather than a
/// second vocabulary for near and far.
enum Scope {
    Building,
    City,
}

impl Scope {
    fn parse(raw: Option<&str>) -> Result<Scope, AxError> {
        match raw {
            None | Some("building") => Ok(Scope::Building),
            Some("city") => Ok(Scope::City),
            Some(other) => Err(AxError::failure(
                AxCode::InvalidArgs,
                "list the neighbours of this run",
                format!("no such scope: {other}"),
            )
            .with_recovery(
                "`building` for who is within reach, `city` for the buildings there are",
            )),
        }
    }
}

/// This building, one address per line.
///
/// Empty rooms are listed with the residents rather than after them:
/// address order is the order a reader can predict, and a second
/// ordering by occupancy would make a place's position depend on
/// whether somebody happened to be living in it.
fn render_building(seen: &Neighbourhood) -> String {
    let here = seen.here();
    if here.is_empty() {
        return format!(
            "Nobody else has an address in {}. You are the only place in this building; \
             ask the person for somebody to work with, or delegate to open a room.\n",
            seen.building().as_str()
        );
    }
    let mut out = format!(
        "Neighbours in {}. Signal or delegate to one of these addresses; an address this list \
         does not have takes messages nobody reads.\n",
        seen.building().as_str()
    );
    for neighbour in here {
        out.push_str("- ");
        out.push_str(neighbour.addr.as_str());
        match &neighbour.occupancy {
            Occupancy::Resident { bring } if bring.is_empty() => {
                out.push_str(": stands here, and says nothing about itself");
            }
            Occupancy::Resident { bring } => {
                out.push_str(": ");
                out.push_str(bring);
            }
            Occupancy::Empty => out.push_str(": an open room, nobody in it"),
        }
        out.push('\n');
    }
    out
}

/// The city, by building name only.
fn render_city(seen: &Neighbourhood) -> String {
    let mut out = String::from(
        "Buildings in this city. Only the one you are in lists who is inside it; work that \
         crosses to another building goes through the person.\n",
    );
    for building in seen.buildings() {
        out.push_str("- ");
        out.push_str(building.as_str());
        if building == seen.building() {
            out.push_str(" (you are here)");
        }
        out.push('\n');
    }
    out
}

impl Tool for NeighboursTool {
    fn meta(&self) -> &ToolMeta {
        &self.meta
    }

    fn invoke(&mut self, call: &ToolCall) -> Result<ToolOutcome, AxError> {
        if call.name != self.meta.name {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "list the neighbours of this run",
                format!("call routed to the wrong tool: {}", call.name.as_str()),
            ));
        }
        let scope = Scope::parse(call.args.as_map().get("scope").and_then(Value::as_str))?;
        let mut out = Map::new();
        out.insert(
            "text".to_owned(),
            Value::String(match scope {
                Scope::Building => render_building(&self.seen),
                Scope::City => render_city(&self.seen),
            }),
        );
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
    use kernel::Address;
    use std::path::Path;

    fn city_with_two_residents() -> tempfile::TempDir {
        let dir = tempfile::tempdir().unwrap();
        let root: &Path = dir.path();
        for (at, text) in [
            (
                "lab/mason",
                "# URBANITE.md\n\n## Bring them\n\nAnything that has to survive a firing.\n",
            ),
            ("lab/scribe", "# URBANITE.md\n\nReads twice, writes once.\n"),
        ] {
            std::fs::create_dir_all(root.join(at)).unwrap();
            std::fs::write(root.join(at).join(crate::resident::URBANITE_FILE), text).unwrap();
        }
        std::fs::create_dir_all(root.join("lab").join("store")).unwrap();
        std::fs::create_dir_all(root.join("market")).unwrap();
        dir
    }

    fn tool(root: &Path, me: &str) -> NeighboursTool {
        let seen = Neighbourhood::scan(
            root,
            &Address::parse("lab").unwrap(),
            &Address::parse(me).unwrap(),
            &|addr| u32::from(addr.as_str() == "lab/scribe"),
        )
        .unwrap();
        NeighboursTool::new(seen).unwrap()
    }

    fn call(scope: Option<&str>) -> ToolCall {
        let mut args = Map::new();
        if let Some(value) = scope {
            args.insert("scope".to_owned(), Value::String(value.to_owned()));
        }
        ToolCall {
            id: "n1".to_owned(),
            name: ToolName::parse("neighbours").unwrap(),
            args: Payload::new(args).unwrap(),
        }
    }

    fn text_of(outcome: &ToolOutcome) -> String {
        outcome.result.as_map()["text"].as_str().unwrap().to_owned()
    }

    #[test]
    fn the_default_scope_is_the_building_and_it_names_every_address_once() {
        let dir = city_with_two_residents();
        let mut tool = tool(dir.path(), "lab/mason");
        let answer = text_of(&tool.invoke(&call(None)).unwrap());
        assert!(answer.contains("- lab/scribe: Reads twice, writes once."));
        assert!(answer.contains("- lab/store: an open room, nobody in it"));
        assert!(
            !answer.contains("lab/mason"),
            "a run does not need to be told where it is standing"
        );
        assert!(
            !answer.contains("market"),
            "another building is out of reach and so out of this answer"
        );
        assert!(
            answer.contains("nobody reads"),
            "the sentence this tool exists for comes before the list"
        );
        assert_eq!(answer, text_of(&tool.invoke(&call(None)).unwrap()));
    }

    #[test]
    fn the_city_scope_gives_names_and_no_residents() {
        let dir = city_with_two_residents();
        let mut tool = tool(dir.path(), "lab/mason");
        let answer = text_of(&tool.invoke(&call(Some("city"))).unwrap());
        assert!(answer.contains("- lab (you are here)"));
        assert!(answer.contains("- market"));
        assert!(
            !answer.contains("scribe"),
            "a resident of another building is not named at this distance"
        );
    }

    #[test]
    fn a_scope_nobody_defined_is_refused_with_the_two_that_exist() {
        let dir = city_with_two_residents();
        let mut tool = tool(dir.path(), "lab/mason");
        let refusal = tool.invoke(&call(Some("planet"))).unwrap_err();
        assert_eq!(*refusal.code(), AxCode::InvalidArgs);
        assert!(refusal.recovery().contains("building"));
        assert!(refusal.recovery().contains("city"));
    }

    #[test]
    fn a_building_with_one_address_says_so_instead_of_printing_an_empty_list() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("lab")).unwrap();
        let mut tool = tool(dir.path(), "lab");
        let answer = text_of(&tool.invoke(&call(None)).unwrap());
        assert!(answer.contains("only place in this building"));
        assert!(!answer.contains("- "), "there is no list to print");
    }
}
