// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Progressive disclosure: L2 tools, reading-room
//! SKILL entries and the current mode, one line each in the Resident
//! segment, expansions on demand. Only what this session can actually
//! reach is listed — admission is the caller's evidence (city::policy
//! evaluates reading rooms in P1; until then the assembler supplies the
//! admitted set directly).
//!
//! `tool_defs` is the single source of `ChatRequest.tools`: a tool absent
//! here does not exist for the model.

use std::collections::BTreeMap;

use kernel::{AxCode, AxError, ToolDef, ToolMeta};

use crate::mode::Mode;

/// One disclosed row: "what it is + when to use it" resident-side, the
/// "how to use it" expansion fetched on demand.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CatalogEntry {
    pub name: String,
    pub disclosure: String,
    pub expansion: String,
}

#[derive(Debug, Default)]
pub struct Catalog {
    tools: BTreeMap<String, ToolRow>,
    skills: BTreeMap<String, CatalogEntry>,
    mode: Option<Mode>,
}

#[derive(Debug)]
struct ToolRow {
    disclosure: String,
    def: ToolDef,
}

impl Catalog {
    pub fn new() -> Catalog {
        Catalog::default()
    }

    /// Registers an L2 tool (or an L0 tool for wire schema purposes).
    /// Eight-field completeness is the type's business; what is checked
    /// here is catalog hygiene: non-empty disclosure, no duplicate name.
    pub fn admit_tool(&mut self, meta: &ToolMeta) -> Result<(), AxError> {
        if meta.disclosure.trim().is_empty() {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "admit tool to catalog",
                format!("{}: empty disclosure", meta.name),
            ));
        }
        let key = meta.name.as_str().to_owned();
        if self.tools.contains_key(&key) {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "admit tool to catalog",
                format!("{key}: duplicate name"),
            ));
        }
        self.tools.insert(
            key,
            ToolRow {
                disclosure: meta.disclosure.clone(),
                def: ToolDef {
                    name: meta.name.clone(),
                    description: meta.disclosure.clone(),
                    input_schema: meta.params.clone(),
                },
            },
        );
        Ok(())
    }

    /// Registers one reading-room-admitted SKILL entry.
    pub fn admit_skill(&mut self, entry: CatalogEntry) -> Result<(), AxError> {
        if entry.name.trim().is_empty() || entry.disclosure.trim().is_empty() {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "admit skill to catalog",
                "empty name or disclosure",
            ));
        }
        if self.skills.contains_key(&entry.name) {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "admit skill to catalog",
                format!("{}: duplicate name", entry.name),
            ));
        }
        self.skills.insert(entry.name.clone(), entry);
        Ok(())
    }

    /// The run sits in exactly one mode; the catalog shows only it.
    pub fn set_mode(&mut self, mode: Mode) {
        self.mode = Some(mode);
    }

    /// The Resident-segment text: header line, then one line per entry.
    /// BTreeMap order makes the bytes a pure function of the content.
    pub fn render(&self) -> String {
        let mut out = String::from(
            "Catalog: one line per capability. Expand an entry before first use; \
             a tool not listed here does not exist.\n",
        );
        for (name, row) in &self.tools {
            out.push_str("- tool ");
            out.push_str(name);
            out.push_str(": ");
            out.push_str(&row.disclosure);
            out.push('\n');
        }
        for (name, entry) in &self.skills {
            out.push_str("- skill ");
            out.push_str(name);
            out.push_str(": ");
            out.push_str(&entry.disclosure);
            out.push('\n');
        }
        if let Some(mode) = self.mode {
            let entry = mode.catalog_entry();
            out.push_str("- ");
            out.push_str(&entry.name);
            out.push_str(": ");
            out.push_str(&entry.disclosure);
            out.push('\n');
        }
        out
    }

    /// The only source of `ChatRequest.tools`.
    pub fn tool_defs(&self) -> Vec<ToolDef> {
        self.tools.values().map(|row| row.def.clone()).collect()
    }

    /// Second-level disclosure. Tools expand to their schema via
    /// `tool_defs` (the wire always carries it); skills and the mode
    /// expand to their stored text.
    pub fn expand(&self, name: &str) -> Option<String> {
        if let Some(entry) = self.skills.get(name) {
            return Some(entry.expansion.clone());
        }
        if let Some(mode) = self.mode {
            let entry = mode.catalog_entry();
            if entry.name == name {
                return Some(entry.expansion);
            }
        }
        None
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
    use kernel::{CostTier, Effect, Payload, RenderIntent, Temporal, ToolMeta, ToolName};

    fn meta(name: &str) -> ToolMeta {
        ToolMeta {
            name: ToolName::parse(name).unwrap(),
            disclosure: format!("{name} does one thing; use it when that thing is needed"),
            params: Payload::empty(),
            effect: Effect::Read,
            cost_tier: CostTier::Free,
            timeout: None,
            render: RenderIntent::Generic,
            temporal: Temporal::Timeless,
        }
    }

    #[test]
    fn render_is_deterministic_and_sorted() {
        let mut catalog = Catalog::new();
        catalog.admit_tool(&meta("zeta")).unwrap();
        catalog.admit_tool(&meta("alpha")).unwrap();
        catalog
            .admit_skill(CatalogEntry {
                name: "review".to_owned(),
                disclosure: "review a diff".to_owned(),
                expansion: "run it before merging".to_owned(),
            })
            .unwrap();
        catalog.set_mode(Mode::PlanGoal);
        let text = catalog.render();
        let alpha = text.find("- tool alpha").unwrap();
        let zeta = text.find("- tool zeta").unwrap();
        assert!(alpha < zeta, "BTreeMap order");
        assert!(text.contains("- skill review:"));
        assert!(text.contains("- mode:plan_goal:"));
        assert_eq!(text, catalog.render(), "same content, same bytes");
    }

    #[test]
    fn duplicates_and_empty_disclosures_are_refused() {
        let mut catalog = Catalog::new();
        catalog.admit_tool(&meta("probe")).unwrap();
        assert!(catalog.admit_tool(&meta("probe")).is_err());
        let mut empty = meta("hollow");
        empty.disclosure = "  ".to_owned();
        assert!(catalog.admit_tool(&empty).is_err());
    }

    #[test]
    fn tool_defs_carry_schema_and_expand_serves_skills_and_mode() {
        let mut catalog = Catalog::new();
        catalog.admit_tool(&meta("probe")).unwrap();
        catalog.set_mode(Mode::Experiment);
        let defs = catalog.tool_defs();
        assert_eq!(defs.len(), 1);
        assert_eq!(defs[0].name.as_str(), "probe");
        assert!(catalog.expand("mode:experiment").is_some());
        assert!(catalog.expand("missing").is_none());
    }
}
