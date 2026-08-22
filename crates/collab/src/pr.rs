// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Getting a node's work into the building, with the verification taken
//! out of the implementer's hands.
//!
//! The phases are types: a pull request that has not been verified has
//! no method that merges it, so "verified before merged" is not a rule
//! anybody has to remember. Verification itself is not re-decided here -
//! an [`Artifact`](crate::fanin::Artifact) already carries the fact that
//! somebody other than the producer ran the done check, and this module
//! reuses that judgment rather than making a second one.
//!
//! What this module adds is the match between a request and the work
//! offered for it, and the three records the ledger keeps.

use kernel::{AxCode, AxError, Payload};
use serde_json::{Map, Value};

use crate::fanin::Artifact;
use crate::workshop::NodeId;

/// A request to bring one node's branch into the building.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Pr<S> {
    node: NodeId,
    implementer: String,
    branch: String,
    state: S,
}

/// Opened, not yet answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Open;

/// Answered by verification, and mergeable for that reason.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Verified {
    by: String,
}

/// Merged: the commit the building now stands on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Merged {
    by: String,
    commit: String,
}

impl Pr<Open> {
    /// Opens a request for a node's branch.
    ///
    /// # Errors
    /// Refuses an unnamed implementer: the one thing this flow is built
    /// to know is who must not be the verifier.
    pub fn open(node: NodeId, implementer: String, branch: String) -> Result<Pr<Open>, AxError> {
        if implementer.trim().is_empty() {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "open a pull request",
                node.as_str().to_owned(),
            )
            .with_recovery(
                "name the implementer; this flow exists to keep them out of their own review",
            ));
        }
        Ok(Pr {
            node,
            implementer,
            branch,
            state: Open,
        })
    }

    /// Accepts verification, in the form of an artifact somebody else
    /// produced by running the node's done check.
    ///
    /// # Errors
    /// Refuses an artifact for a different node, and one whose verifier
    /// is the implementer. The second refusal is nearly unreachable -
    /// `Artifact` cannot be built by its own producer - and it is kept
    /// because "nearly" is doing the work of a review nobody performs.
    pub fn verified(self, artifact: &Artifact) -> Result<Pr<Verified>, AxError> {
        if artifact.node() != &self.node {
            return Err(AxError::failure(
                AxCode::EvidenceMissing,
                "verify a pull request",
                format!(
                    "the artifact is for node {}, the request is for {}",
                    artifact.node().as_str(),
                    self.node.as_str()
                ),
            )
            .with_recovery("verify the node this request is for"));
        }
        if artifact.verified_by() == self.implementer {
            return Err(AxError::failure(
                AxCode::EvidenceMissing,
                "verify a pull request",
                format!("{} verified their own work", self.implementer),
            )
            .with_recovery("have a test resident run the done check"));
        }
        Ok(Pr {
            node: self.node,
            implementer: self.implementer,
            branch: self.branch,
            state: Verified {
                by: artifact.verified_by().to_owned(),
            },
        })
    }

    /// Turns the request down. Rejection is available from the open
    /// phase only: what has been verified is answered by merging it or
    /// by opening a new request, not by revoking a verdict.
    ///
    /// # Errors
    /// Propagates the payload's refusal to hold what it was given.
    pub fn rejected_payload(&self, by: &str, why: &str) -> Result<Payload, AxError> {
        let mut map = self.record();
        map.insert("by".to_owned(), Value::String(by.to_owned()));
        map.insert("why".to_owned(), Value::String(why.to_owned()));
        Payload::new(map)
    }

    /// The `pr_opened` record.
    ///
    /// # Errors
    /// Propagates the payload's refusal to hold what it was given.
    pub fn opened_payload(&self) -> Result<Payload, AxError> {
        Payload::new(self.record())
    }
}

impl Pr<Verified> {
    /// The resident that ran the check.
    #[must_use]
    pub fn verified_by(&self) -> &str {
        &self.state.by
    }

    /// Records that the branch landed at `commit`. The merge itself is
    /// `memory`'s: this crate decides, that one moves the files.
    #[must_use]
    pub fn merged(self, commit: String) -> Pr<Merged> {
        Pr {
            node: self.node,
            implementer: self.implementer,
            branch: self.branch,
            state: Merged {
                by: self.state.by,
                commit,
            },
        }
    }
}

impl Pr<Merged> {
    #[must_use]
    pub fn commit(&self) -> &str {
        &self.state.commit
    }

    /// The `pr_merged` record.
    ///
    /// # Errors
    /// Propagates the payload's refusal to hold what it was given.
    pub fn merged_payload(&self) -> Result<Payload, AxError> {
        let mut map = self.record();
        map.insert(
            "verified_by".to_owned(),
            Value::String(self.state.by.clone()),
        );
        map.insert(
            "commit".to_owned(),
            Value::String(self.state.commit.clone()),
        );
        Payload::new(map)
    }
}

impl<S> Pr<S> {
    #[must_use]
    pub fn node(&self) -> &NodeId {
        &self.node
    }

    #[must_use]
    pub fn implementer(&self) -> &str {
        &self.implementer
    }

    /// The branch the node worked on, which is also its worktree's name.
    #[must_use]
    pub fn branch(&self) -> &str {
        &self.branch
    }

    fn record(&self) -> Map<String, Value> {
        let mut map = Map::new();
        map.insert(
            "node".to_owned(),
            Value::String(self.node.as_str().to_owned()),
        );
        map.insert(
            "implementer".to_owned(),
            Value::String(self.implementer.clone()),
        );
        map.insert("branch".to_owned(), Value::String(self.branch.clone()));
        map
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
    use crate::fanin::Claim;
    use kernel::{B3Hash, Locator};

    fn artifact(node: &str, by: &str, verifier: &str) -> Artifact {
        let digest = B3Hash::digest(b"work");
        Claim::new(
            NodeId::parse(node).unwrap(),
            Locator::parse(&format!("cas:b3-{digest}")).unwrap(),
            digest,
            by.to_owned(),
        )
        .verified(true, verifier)
        .unwrap()
    }

    fn request() -> Pr<Open> {
        Pr::open(
            NodeId::parse("node-1").unwrap(),
            "lab/room1".to_owned(),
            "node-1".to_owned(),
        )
        .unwrap()
    }

    #[test]
    fn a_request_nobody_verified_has_no_way_to_merge() {
        // The proof is the trybuild counterexample in tests/ui: `Pr<Open>`
        // has no `merged`. Here we hold the other half - the phase a
        // merge is reachable from is the one verification produces.
        let verified = request().verified(&artifact("node-1", "lab/room1", "lab/tests"));
        assert!(verified.is_ok());
        assert_eq!(verified.unwrap().verified_by(), "lab/tests");
    }

    #[test]
    fn the_implementer_cannot_be_the_verifier_at_either_gate() {
        // The first gate is in `fanin`: an artifact cannot be built by
        // its own producer. The second is here, in case a caller ever
        // hands over an artifact from somewhere else.
        let err = request()
            .verified(&artifact("node-1", "lab/tests", "lab/room1"))
            .unwrap_err();
        assert_eq!(err.code(), &AxCode::EvidenceMissing);
        assert!(err.subject().contains("lab/room1"));
    }

    #[test]
    fn work_verified_for_another_node_does_not_merge_this_one() {
        let err = request()
            .verified(&artifact("node-2", "lab/room1", "lab/tests"))
            .unwrap_err();
        assert!(err.subject().contains("node-2"));
        assert!(err.recovery().contains("the node this request is for"));
    }

    #[test]
    fn the_three_records_say_who_did_what() {
        let opened = request().opened_payload().unwrap();
        assert_eq!(
            opened.as_map().get("implementer").and_then(Value::as_str),
            Some("lab/room1")
        );

        let rejected = request()
            .rejected_payload("lab/tests", "the done check fails on Windows")
            .unwrap();
        assert!(
            rejected
                .as_map()
                .get("why")
                .and_then(Value::as_str)
                .is_some_and(|why| why.contains("Windows"))
        );

        let merged = request()
            .verified(&artifact("node-1", "lab/room1", "lab/tests"))
            .unwrap()
            .merged("abc123".to_owned());
        assert_eq!(merged.commit(), "abc123");
        let record = merged.merged_payload().unwrap();
        assert_eq!(
            record.as_map().get("verified_by").and_then(Value::as_str),
            Some("lab/tests")
        );
        assert_eq!(
            record.as_map().get("commit").and_then(Value::as_str),
            Some("abc123")
        );
    }
}
