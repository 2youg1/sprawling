// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Delegation: two delegate kinds, one level deep.
//! The guard is two-layered on purpose: statically, a `Delegate` value
//! has no delegate method — minting a grandchild cannot be spelled;
//! dynamically, `admit` re-checks depth at every spawn so a hand-rolled
//! path cannot sneak past the type. One level is an invariant, not a
//! constant — a constant would imply it is tunable.

use serde::{Deserialize, Serialize};

/// Resident delegates join society and answer for output; Ephemerals are
/// tools — one clarification channel up, nothing else.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DelegateKind {
    Resident,
    Ephemeral,
}

impl DelegateKind {
    /// The one spelling. Both the tool that parses these words and the
    /// status line that prints them read it here, so a rename cannot
    /// half-happen.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            DelegateKind::Resident => "resident",
            DelegateKind::Ephemeral => "ephemeral",
        }
    }
}

/// The depth-zero position: the only type with a `delegate` method
/// (15.3-10). Who holds a `Delegator` is assembly's discipline; what a
/// `Delegate` value can mint is this module's — nothing.
#[derive(Debug)]
pub struct Delegator(());

impl Delegator {
    /// Minting point: assembly and citysim only.
    pub fn root() -> Delegator {
        Delegator(())
    }

    pub fn delegate(&self, kind: DelegateKind) -> Delegate {
        Delegate { kind }
    }
}

/// A delegated position. No delegate method — the compile error is the
/// design (trybuild counterexample, S2.11).
#[derive(Debug)]
pub struct Delegate {
    kind: DelegateKind,
}

impl Delegate {
    pub fn kind(&self) -> &DelegateKind {
        &self.kind
    }
}

/// Where a spawner stands.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Depth {
    Root,
    Delegated,
}

/// Deliberately exhaustive verdict; the refusal (E_DELEGATION_DEPTH,
/// three parts) is shaped by gate::spawn — the sole gate-code producer.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DelegationVerdict {
    Allow,
    Deny,
}

/// The dynamic half of the two-layer guard: a delegated position spawns
/// nothing, whatever kind it asks for.
pub fn admit(parent: Depth, kind: &DelegateKind) -> DelegationVerdict {
    match parent {
        Depth::Root => {
            // Exhaustive on purpose: a new kind must decide its depth rule.
            match kind {
                DelegateKind::Resident | DelegateKind::Ephemeral => DelegationVerdict::Allow,
            }
        }
        Depth::Delegated => DelegationVerdict::Deny,
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
    fn root_spawns_both_kinds_delegated_spawns_none() {
        assert_eq!(
            admit(Depth::Root, &DelegateKind::Resident),
            DelegationVerdict::Allow
        );
        assert_eq!(
            admit(Depth::Root, &DelegateKind::Ephemeral),
            DelegationVerdict::Allow
        );
        assert_eq!(
            admit(Depth::Delegated, &DelegateKind::Resident),
            DelegationVerdict::Deny
        );
        assert_eq!(
            admit(Depth::Delegated, &DelegateKind::Ephemeral),
            DelegationVerdict::Deny
        );
    }

    #[test]
    fn the_static_path_mints_one_level_only() {
        let root = Delegator::root();
        let child = root.delegate(DelegateKind::Ephemeral);
        assert_eq!(child.kind(), &DelegateKind::Ephemeral);
        // `child.delegate(...)` does not compile — pinned by the S2.11
        // trybuild case; this test documents the runtime-visible half.
    }
}
