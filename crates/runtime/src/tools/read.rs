// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! The read tool: one argument that is either a path this city lets a
//! model choose, or a name its reading room already admitted.
//!
//! **Two ways in, and the difference is who chose.** A path is chosen by
//! the model, so it is judged: it must parse as an address, and it must
//! not reach a reserved subtree — the accounting, the configuration, the
//! rules and the history stay unreadable to the thing they govern. A
//! catalog name was chosen by the person who wrote the building's
//! reading room, and admission happened when they wrote it; the skill it
//! resolves to may therefore sit in reserved space, because the answer to
//! "may this run see it" was given before the run existed.
//!
//! Without this tool the catalog could name a skill and never hand it
//! over, and every file the prompt asks an agent to consult had to be
//! reached by writing Python inside `exec` — which a city with no
//! sandbox and no shell cannot do at all.

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use kernel::{
    AxCode, AxError, CostTier, Effect, Payload, RenderIntent, Temporal, Tool, ToolCall, ToolMeta,
    ToolName, ToolOutcome,
};
use serde_json::{Map, Value};

use crate::catalog::{Catalog, Expansion};

/// Where the answer to one call comes from.
enum Found {
    File(PathBuf),
    Text(String),
}

/// What a read answers with, and what it refuses.
pub struct ReadTool {
    city_root: PathBuf,
    /// The same catalog the model was shown. Shared rather than copied:
    /// a second list of what this run may open would be a second
    /// authority, and the one that drifts is always the copy.
    catalog: Rc<RefCell<Catalog>>,
    meta: ToolMeta,
}

impl ReadTool {
    /// # Errors
    /// Propagates a malformed parameter schema, which is a build-time
    /// defect rather than a runtime one.
    pub fn new(city_root: &Path, catalog: Rc<RefCell<Catalog>>) -> Result<ReadTool, AxError> {
        let mut params = Map::new();
        params.insert("type".to_owned(), Value::String("object".to_owned()));
        let mut properties = Map::new();
        let mut spec = Map::new();
        spec.insert("type".to_owned(), Value::String("string".to_owned()));
        spec.insert(
            "description".to_owned(),
            Value::String(
                "a file path relative to the city root, or the name of a catalog entry".to_owned(),
            ),
        );
        properties.insert("path".to_owned(), Value::Object(spec));
        params.insert("properties".to_owned(), Value::Object(properties));
        params.insert(
            "required".to_owned(),
            Value::Array(vec![Value::String("path".to_owned())]),
        );
        Ok(ReadTool {
            city_root: city_root.to_path_buf(),
            catalog,
            meta: ToolMeta {
                name: ToolName::parse("read")?,
                disclosure: "Read a file by its path, or a skill by the name the catalog lists it \
                             under."
                    .to_owned(),
                params: Payload::new(params)?,
                effect: Effect::Read,
                cost_tier: CostTier::Light,
                timeout: None,
                render: RenderIntent::Generic,
                temporal: Temporal::Timeless,
            },
        })
    }

    /// Where one argument points, or why it points nowhere.
    ///
    /// Catalog entries are tried first: a building that admits a skill
    /// called `review` has said what that word means here, and a file
    /// that happens to share the name must not be able to shadow it.
    fn resolve(&self, asked: &str) -> Result<Found, AxError> {
        if let Ok(catalog) = self.catalog.try_borrow()
            && let Some(expansion) = catalog.expand(asked)
        {
            return match expansion {
                Expansion::Skill { addr } => {
                    let addr = kernel::Address::parse(&addr).map_err(|err| {
                        AxError::failure(
                            AxCode::ConfigInvalid,
                            "read",
                            format!("{asked} is shelved at {}", err.subject()),
                        )
                        .with_recovery(
                            "this building's reading room names a skill the city cannot address; \
                             a person has to fix the shelf",
                        )
                    })?;
                    Ok(Found::File(self.under_city(&addr)))
                }
                // The catalog's own second level. The prompt carries one
                // line per entry, and this is what that line stood for,
                // so it is handed over rather than refused.
                Expansion::Said { text } => Ok(Found::Text(text)),
            };
        }
        let addr = kernel::Address::parse(asked).map_err(|err| {
            AxError::failure(
                AxCode::InvalidArgs,
                "read",
                format!("{asked}: {}", err.subject()),
            )
            .with_recovery(
                "pass a city-relative path with no `..` and no leading slash, or a name from \
                     the catalog",
            )
        })?;
        if addr.is_reserved() {
            return Err(AxError::failure(
                AxCode::GateDenied,
                "read",
                format!("{asked} is inside a reserved subtree"),
            )
            .with_recovery(
                "a `.sprawling` directory holds what governs a scope, and no run reads its own \
                 governance; ask for a skill by its catalog name instead",
            ));
        }
        Ok(Found::File(self.under_city(&addr)))
    }

    fn under_city(&self, addr: &kernel::Address) -> PathBuf {
        let mut path = self.city_root.clone();
        for segment in addr.as_str().split('/') {
            path.push(segment);
        }
        path
    }
}

impl Tool for ReadTool {
    fn meta(&self) -> &ToolMeta {
        &self.meta
    }

    fn invoke(&mut self, call: &ToolCall) -> Result<ToolOutcome, AxError> {
        if call.name != self.meta.name {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "read",
                format!("call routed to the wrong tool: {}", call.name.as_str()),
            ));
        }
        let asked = call
            .args
            .as_map()
            .get("path")
            .and_then(Value::as_str)
            .ok_or_else(|| {
                AxError::failure(
                    AxCode::InvalidArgs,
                    "read",
                    "missing string argument `path`",
                )
                .with_recovery("pass one string: a city-relative path, or a catalog name")
            })?;
        let text = match self.resolve(asked)? {
            Found::Text(text) => text,
            Found::File(path) => std::fs::read_to_string(&path).map_err(|err| {
                let code = match err.kind() {
                    std::io::ErrorKind::NotFound => AxCode::InvalidArgs,
                    _ => AxCode::StorageFatal,
                };
                AxError::failure(code, "read", format!("{asked}: {err}")).with_recovery(
                    "check the name against what the catalog lists, or list the \
                                    directory with `exec` first",
                )
            })?,
        };
        let mut out = Map::new();
        out.insert("path".to_owned(), Value::String(asked.to_owned()));
        // The count the model needs to decide whether it has the whole
        // thing: a result the pipeline shortened says so in its own
        // envelope, and this is what it was shortened from.
        let bytes = u64::try_from(text.len()).map_err(|_| {
            AxError::failure(
                AxCode::StorageFatal,
                "read",
                format!("{asked}: length overflow"),
            )
        })?;
        out.insert("bytes".to_owned(), Value::Number(bytes.into()));
        out.insert("text".to_owned(), Value::String(text));
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
    use crate::catalog::CatalogEntry;

    fn tool(root: &Path) -> (ReadTool, Rc<RefCell<Catalog>>) {
        let catalog = Rc::new(RefCell::new(Catalog::new()));
        let tool = ReadTool::new(root, Rc::clone(&catalog)).unwrap();
        (tool, catalog)
    }

    fn call(path: &str) -> ToolCall {
        let mut args = Map::new();
        args.insert("path".to_owned(), Value::String(path.to_owned()));
        ToolCall {
            id: "call-1".to_owned(),
            name: ToolName::parse("read").unwrap(),
            args: Payload::new(args).unwrap(),
        }
    }

    #[test]
    fn a_file_in_the_city_comes_back_with_its_own_length() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("lab")).unwrap();
        std::fs::write(dir.path().join("lab").join("Memo.md"), "one decision\n").unwrap();
        let (mut tool, _catalog) = tool(dir.path());

        let outcome = tool.invoke(&call("lab/Memo.md")).unwrap();
        let map = outcome.result.as_map();
        assert_eq!(map["text"], "one decision\n");
        assert_eq!(map["bytes"], 13);
    }

    /// The rule that keeps a run from reading its own governance is the
    /// same predicate the write side uses, so there is one answer to
    /// "what is reserved" rather than two.
    #[test]
    fn a_model_chosen_path_cannot_reach_a_reserved_subtree() {
        let dir = tempfile::tempdir().unwrap();
        let (mut tool, _catalog) = tool(dir.path());
        for asked in [
            ".sprawling/ledger/0001.jsonl",
            "lab/.sprawling/BUILDING.md",
            ".sprawling/CONFIG.toml",
        ] {
            let err = tool.invoke(&call(asked)).unwrap_err();
            assert_eq!(err.code(), &AxCode::GateDenied, "{asked} was allowed");
            assert!(
                !err.recovery().is_empty(),
                "{asked} refused with no way out"
            );
        }
    }

    /// Admission happened when a person wrote the reading room, so the
    /// skill it names opens even though it lives where no model-chosen
    /// path may go.
    #[test]
    fn the_reading_room_hands_over_what_a_path_could_not_reach() {
        let dir = tempfile::tempdir().unwrap();
        let shelf = dir.path().join(".sprawling").join("library");
        std::fs::create_dir_all(&shelf).unwrap();
        std::fs::write(shelf.join("review.md"), "check the diff first\n").unwrap();
        let (mut tool, catalog) = tool(dir.path());
        catalog
            .borrow_mut()
            .admit_skill(CatalogEntry {
                name: "review".to_owned(),
                disclosure: "how this building reviews".to_owned(),
                expansion: ".sprawling/library/review.md".to_owned(),
            })
            .unwrap();

        assert!(
            tool.invoke(&call(".sprawling/library/review.md")).is_err(),
            "the path is still closed"
        );
        let outcome = tool.invoke(&call("review")).unwrap();
        assert_eq!(outcome.result.as_map()["text"], "check the diff first\n");
    }

    /// The catalog's own second level: the prompt carries one line per
    /// entry, and this is the tool that fetches what the line stood for.
    #[test]
    fn an_entry_the_catalog_holds_is_handed_over_not_refused() {
        let dir = tempfile::tempdir().unwrap();
        let (mut tool, catalog) = tool(dir.path());
        catalog.borrow_mut().set_mode(crate::mode::Mode::Experiment);

        let mode = tool.invoke(&call("mode:experiment")).unwrap();
        let said = mode.result.as_map()["text"].as_str().unwrap_or_default();
        assert!(said.contains("Memo.md"), "the mode's discipline: {said}");

        let dev = tool.invoke(&call("dev")).unwrap();
        let said = dev.result.as_map()["text"].as_str().unwrap_or_default();
        assert!(said.contains("-SPEC.md"), "the developer entry: {said}");
        assert!(said.contains("wait for the person to grant it"));
    }

    #[test]
    fn a_missing_file_is_the_callers_mistake_not_the_disks() {
        let dir = tempfile::tempdir().unwrap();
        let (mut tool, _catalog) = tool(dir.path());
        let err = tool.invoke(&call("lab/nowhere.md")).unwrap_err();
        assert_eq!(err.code(), &AxCode::InvalidArgs);
    }

    #[cfg(feature = "conformance")]
    #[test]
    fn the_tool_refuses_another_tools_call_and_still_answers() {
        let dir = tempfile::tempdir().unwrap();
        let (mut tool, _catalog) = tool(dir.path());
        kernel::tool_conformance::assert_tool_conformance(&mut tool);
    }
}
