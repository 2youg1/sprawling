// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! What a run is told about work it handed down.
//!
//! **The way back is not the next turn.** Only the assembly layer can
//! build a run, and it gets control back when the parent is already
//! frozen, so a child starts after its parent's last turn rather than
//! between two of them. "The parent's next turn" is therefore the next
//! run in the parent's room, and the door that already carries a fact
//! across runs is that room's `Inbox`. A second door would be a second
//! answer to "what is waiting for this resident", so the way back is an
//! ordinary [`Signal`] and `status.signals_pending` counts it without
//! being taught anything new.
//!
//! The city, not the child, is the verifier. `Claim::verified` refuses a
//! producer judging its own work, and `Completion::Done(Evidence)` is
//! something the city observed rather than something the child said -
//! which is exactly what makes an [`Artifact`] constructible here.

use kernel::{Address, AxError, Payload, TimeMs, Version};
use serde_json::{Map, Value};

use crate::fanin::{Artifact, Claim};
use crate::inbox::{Signal, SignalId, SignalKind};
use crate::workshop::NodeId;

/// How one piece of handed-down work came back. Exhaustive: a delegate
/// that finished and one that stopped are different facts, and a parent
/// told only that something came back would have to guess which.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Handback {
    /// The child's own done check passed, and somebody other than the
    /// child said so. The name avoids `Verified`, which this crate
    /// already uses for a pull request's phase.
    Finished(Artifact),
    /// It stopped without evidence, or its claim did not verify.
    /// `because` carries the refusal's own words rather than a summary
    /// of them.
    Stopped { claim: Claim, because: String },
}

impl Handback {
    /// Verifies the child's claim on the city's behalf.
    ///
    /// A refusal is an outcome here rather than a failure: a parent that
    /// is told nothing when its delegate stopped can only find out by
    /// waiting, which is the one thing an agent must never be asked to
    /// do.
    #[must_use]
    pub fn of(claim: Claim, done_check_passed: bool, verifier: &str) -> Handback {
        match claim.clone().verified(done_check_passed, verifier) {
            Ok(artifact) => Handback::Finished(artifact),
            Err(refusal) => Handback::Stopped {
                claim,
                because: refusal.to_string(),
            },
        }
    }

    /// Which room the work was done in. The node id is that room's
    /// address: a delegate's identity is where it worked.
    #[must_use]
    pub fn node(&self) -> &NodeId {
        match self {
            Handback::Finished(artifact) => artifact.node(),
            Handback::Stopped { claim, .. } => claim.node(),
        }
    }

    /// Who did the work.
    #[must_use]
    pub fn by(&self) -> &str {
        match self {
            Handback::Finished(artifact) => artifact.by(),
            Handback::Stopped { claim, .. } => claim.by(),
        }
    }

    /// The signal the asking run's room receives.
    ///
    /// `Thread` rather than `Mention`: this answers something that room
    /// asked for, and the lane it takes is derived from that kind rather
    /// than chosen here.
    ///
    /// # Errors
    /// Propagates the payload's and the signal's own refusals.
    pub fn signal(&self, id: SignalId, to: Address, at: TimeMs) -> Result<Signal, AxError> {
        let mut map = Map::new();
        map.insert(
            "handback".to_owned(),
            Value::String(
                match self {
                    Handback::Finished(_) => "finished",
                    Handback::Stopped { .. } => "stopped",
                }
                .to_owned(),
            ),
        );
        map.insert(
            "room".to_owned(),
            Value::String(self.node().as_str().to_owned()),
        );
        match self {
            Handback::Finished(artifact) => {
                map.insert("at".to_owned(), Value::String(artifact.at().to_string()));
                map.insert(
                    "verified_by".to_owned(),
                    Value::String(artifact.verified_by().to_owned()),
                );
                map.insert(
                    "text".to_owned(),
                    Value::String(format!(
                        "{} finished the work you handed down; its account is pinned at {}",
                        self.by(),
                        artifact.at()
                    )),
                );
            }
            Handback::Stopped { claim, because } => {
                map.insert("at".to_owned(), Value::String(claim.at().to_string()));
                map.insert("because".to_owned(), Value::String(because.clone()));
                map.insert(
                    "text".to_owned(),
                    Value::String(format!(
                        "{} did not finish the work you handed down: {because}",
                        self.by()
                    )),
                );
            }
        }
        Signal::new(
            id,
            SignalKind::Thread,
            self.by().to_owned(),
            to,
            Version::FIRST,
            Payload::new(map)?,
            at,
        )
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
    use kernel::{B3Hash, Locator};

    fn claim(by: &str) -> Claim {
        let digest = B3Hash::digest(b"the account of a delegate");
        Claim::new(
            NodeId::parse("lab/helper").unwrap(),
            Locator::parse(&format!("cas:b3-{digest}")).unwrap(),
            digest,
            by.to_owned(),
        )
    }

    #[test]
    fn a_delegate_that_finished_comes_back_as_a_verified_artifact() {
        let back = Handback::of(claim("lab/helper"), true, "city");
        assert!(matches!(back, Handback::Finished(_)));
        assert_eq!(back.by(), "lab/helper");
        assert_eq!(back.node().as_str(), "lab/helper");

        let signal = back
            .signal(
                SignalId::parse("handback-1").unwrap(),
                Address::parse("lab/room1").unwrap(),
                TimeMs::new(90),
            )
            .unwrap();
        assert_eq!(signal.from(), "lab/helper");
        assert_eq!(signal.room().as_str(), "lab/room1");
        let body = signal.payload().as_map();
        assert_eq!(body["handback"], "finished");
        assert_eq!(body["verified_by"], "city");
        assert!(body["text"].as_str().unwrap().contains("finished"));
    }

    /// The one the parent would otherwise never hear about.
    #[test]
    fn a_delegate_that_stopped_still_reaches_the_run_that_asked() {
        let back = Handback::of(claim("lab/helper"), false, "city");
        let Handback::Stopped { ref because, .. } = back else {
            panic!("a failed done check does not verify");
        };
        assert!(because.contains("done check"));

        let signal = back
            .signal(
                SignalId::parse("handback-2").unwrap(),
                Address::parse("lab/room1").unwrap(),
                TimeMs::new(91),
            )
            .unwrap();
        let body = signal.payload().as_map();
        assert_eq!(body["handback"], "stopped");
        assert!(body["text"].as_str().unwrap().contains("did not finish"));
        assert!(body.contains_key("because"));
    }

    /// The rule `Claim::verified` holds, reached through this door: the
    /// city may verify, the child may not verify itself.
    #[test]
    fn the_child_is_not_allowed_to_be_its_own_verifier() {
        let back = Handback::of(claim("lab/helper"), true, "lab/helper");
        assert!(matches!(back, Handback::Stopped { .. }));
    }
}
