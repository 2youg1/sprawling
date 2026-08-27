// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! The three registry books: Artifact, Asset,
//! Resident census. The load-bearing line is the Claim/Artifact split —
//! an unverified result is testimony, and `Artifact::verify` is the only
//! door between the two (player–referee in the type). The registry is a
//! value, not a store: state lives with the caller and is rebuilt from
//! the Ledger (S3 projection).

use std::collections::{BTreeMap, BTreeSet};

use serde::{Deserialize, Serialize};

use crate::error::{AxCode, AxError};
use crate::event::{EventKind, EventRef};
use crate::locator::Locator;

/// Non-empty resident identity; the `role@building.n` grammar tightens
/// with city::resident (P1).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ResidentId(String);

impl ResidentId {
    pub fn new(raw: impl Into<String>) -> Option<ResidentId> {
        let raw = raw.into();
        if raw.is_empty() {
            None
        } else {
            Some(ResidentId(raw))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Testimony: a result nobody has verified yet. Claims never enter the
/// registry; they only carry what to verify.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Claim {
    pub locator: Locator,
    pub by: String,
}

/// A verified product. Private fields: the only constructor demands
/// in-window verification evidence.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Artifact {
    locator: Locator,
    verified_by: EventRef,
}

impl Artifact {
    /// Player–referee in the type: evidence must be a `tool_result` or
    /// `model_returned` ref, else `E_EVIDENCE_MISSING`.
    pub fn verify(claim: Claim, evidence: EventRef) -> Result<Artifact, AxError> {
        match evidence.kind() {
            EventKind::ToolResult | EventKind::ModelReturned => Ok(Artifact {
                locator: claim.locator,
                verified_by: evidence,
            }),
            other => Err(AxError::failure(
                AxCode::EvidenceMissing,
                "verify claim",
                claim.locator.to_string(),
            )
            .with_recovery(format!(
                "evidence must be a tool_result or model_returned ref, got {other:?}; \
                 run the verification and cite its result"
            ))),
        }
    }

    pub fn locator(&self) -> &Locator {
        &self.locator
    }

    pub fn verified_by(&self) -> &EventRef {
        &self.verified_by
    }
}

/// Deliberately exhaustive registration verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RegisterVerdict {
    Registered,
    AlreadyRegistered,
}

/// The three books. Keys are canonical locator spellings (Display is the
/// canonical form by the locator round-trip law).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Registry {
    artifacts: BTreeMap<String, Artifact>,
    assets: BTreeSet<String>,
    residents: BTreeSet<ResidentId>,
}

impl Registry {
    pub fn new() -> Registry {
        Registry::default()
    }

    pub fn register_artifact(&mut self, artifact: Artifact) -> RegisterVerdict {
        let key = artifact.locator.to_string();
        match self.artifacts.entry(key) {
            std::collections::btree_map::Entry::Occupied(_) => RegisterVerdict::AlreadyRegistered,
            std::collections::btree_map::Entry::Vacant(slot) => {
                slot.insert(artifact);
                RegisterVerdict::Registered
            }
        }
    }

    /// Promotion registers standing only; scoring is eval's (P3). An
    /// unregistered locator cannot be an asset — assets are promoted
    /// artifacts, not free-floating paths.
    pub fn promote_asset(&mut self, locator: &Locator) -> Result<RegisterVerdict, AxError> {
        let key = locator.to_string();
        if !self.artifacts.contains_key(&key) {
            return Err(AxError::failure(AxCode::PathNotFound, "promote asset", key)
                .with_nearby(self.artifacts.keys().cloned().collect())
                .with_recovery("register the artifact first, then promote it"));
        }
        if self.assets.insert(key) {
            Ok(RegisterVerdict::Registered)
        } else {
            Ok(RegisterVerdict::AlreadyRegistered)
        }
    }

    pub fn register_resident(&mut self, id: ResidentId) -> RegisterVerdict {
        if self.residents.insert(id) {
            RegisterVerdict::Registered
        } else {
            RegisterVerdict::AlreadyRegistered
        }
    }

    pub fn artifact(&self, locator: &Locator) -> Option<&Artifact> {
        self.artifacts.get(&locator.to_string())
    }

    /// The Discard door's query face (7.2: hitting a registry asset
    /// escalates).
    pub fn is_asset(&self, locator: &Locator) -> bool {
        self.assets.contains(&locator.to_string())
    }

    /// Address-level asset membership for the Discard door: a path is an
    /// asset when any registered asset is a `file:` locator at exactly
    /// this address (content pins differ, the place is the same).
    pub fn is_asset_at(&self, addr: &crate::address::Address) -> bool {
        self.assets.iter().any(|key| {
            matches!(
                Locator::parse(key),
                Ok(Locator::File { address, .. }) if address == *addr
            )
        })
    }

    pub fn is_resident(&self, id: &ResidentId) -> bool {
        self.residents.contains(id)
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
    use crate::event::{EventDraft, EventRecord, Payload, RunId, Seq, TimeMs};
    use crate::ledger::GENESIS_PREV;

    fn evidence(kind: EventKind) -> EventRef {
        let draft = EventDraft {
            run: RunId::CITY,
            t: TimeMs::new(0),
            who: "city".into(),
            addr: None,
            kind,
            data: Payload::empty(),
            ig: false,
        };
        EventRecord::from_draft(draft, Seq::FIRST, GENESIS_PREV).to_ref()
    }

    fn locator() -> Locator {
        Locator::parse(&format!("cas:b3-{}", "cd".repeat(32))).unwrap()
    }

    fn claim() -> Claim {
        Claim {
            locator: locator(),
            by: "worker@sim.1".into(),
        }
    }

    #[test]
    fn verification_accepts_only_in_window_result_kinds() {
        assert!(Artifact::verify(claim(), evidence(EventKind::ToolResult)).is_ok());
        assert!(Artifact::verify(claim(), evidence(EventKind::ModelReturned)).is_ok());
        let err = Artifact::verify(claim(), evidence(EventKind::RunStarted)).unwrap_err();
        assert_eq!(err.code(), &AxCode::EvidenceMissing);
    }

    #[test]
    fn promotion_requires_prior_registration() {
        let mut registry = Registry::new();
        let err = registry.promote_asset(&locator()).unwrap_err();
        assert_eq!(err.code(), &AxCode::PathNotFound);
        let artifact = Artifact::verify(claim(), evidence(EventKind::ToolResult)).unwrap();
        assert_eq!(
            registry.register_artifact(artifact.clone()),
            RegisterVerdict::Registered
        );
        assert_eq!(
            registry.register_artifact(artifact),
            RegisterVerdict::AlreadyRegistered
        );
        assert_eq!(
            registry.promote_asset(&locator()).unwrap(),
            RegisterVerdict::Registered
        );
        assert!(registry.is_asset(&locator()));
    }

    #[test]
    fn the_census_deduplicates() {
        let mut registry = Registry::new();
        let id = ResidentId::new("worker@sim.1").unwrap();
        assert_eq!(
            registry.register_resident(id.clone()),
            RegisterVerdict::Registered
        );
        assert_eq!(
            registry.register_resident(id.clone()),
            RegisterVerdict::AlreadyRegistered
        );
        assert!(registry.is_resident(&id));
    }
}
