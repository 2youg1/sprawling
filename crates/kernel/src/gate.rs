// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The five doors and idempotent dedup. This module is
//! the library-wide sole producer of gate refusal codes: every Deny goes
//! through `AxError::refusal` with the three mandatory parts, and every
//! Escalate mints the ApprovalItem that pre-blocks the run. Boundary
//! feedback beats opening sermons — the refusal is the teaching.
//!
//! Five separate functions, not one fat envelope: each door reads its own
//! inputs, the effect layer routes by the `Effect` field (5.2), and kani
//! can walk each door's space without multiplying the others'.

use std::collections::BTreeSet;

use crate::address::Address;
use crate::approval::ApprovalId;
use crate::approval::{ApprovalClass, ApprovalItem, ApprovalSource, ClusterKey};
use crate::budget::{BudgetLadder, BudgetLayer, BudgetUse};
use crate::delegation::{DelegateKind, DelegationVerdict, Depth, admit as admit_delegation};
use crate::discard::{DenyReason, DiscardRequest, DiscardVerdict, EscalateReason, decide};
use crate::error::{AxCode, AxError, GateRefusal};
use crate::event::TimeMs;
use crate::idem::IdemKey;
use crate::locator::Locator;
use crate::registry::Registry;
use crate::secret::SecretSpan;
use crate::taint::TaintSet;
use crate::write_domain::{DomainVerdict, WriteDomain};

/// What an Escalate needs to mint its item; all injected — the gate
/// samples nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GateContext {
    pub actor: String,
    pub now: TimeMs,
    pub item_id: ApprovalId,
}

/// Deliberately exhaustive: every caller decides all three ways.
#[derive(Debug)]
pub enum GateOutcome {
    Allow,
    Escalate { item: ApprovalItem },
    Deny { refusal: Box<AxError> },
}

fn item(
    ctx: &GateContext,
    class: ApprovalClass,
    detail: String,
    action_desc: String,
    artifact: Locator,
    tainted: bool,
) -> ApprovalItem {
    ApprovalItem {
        id: ctx.item_id.clone(),
        source: ApprovalSource::Gate,
        actor: ctx.actor.clone(),
        action_desc,
        artifact,
        cluster_key: ClusterKey { class, detail },
        created: ctx.now,
        tainted,
    }
}

/// The Domain door: writes land inside the write domain (8.3). Taint
/// does not soften or harden a boundary violation, but the refusal names
/// the provenance so the reader sees whose bidding the action did.
pub fn domain(domain: &WriteDomain, target: &Address, taint: &TaintSet) -> GateOutcome {
    match domain.admits(target) {
        DomainVerdict::Within => GateOutcome::Allow,
        DomainVerdict::Outside { prefixes } => {
            let mut violation = format!("target {} is outside the write domain", target.as_str());
            if !taint.is_empty() {
                violation.push_str(&format!(
                    "; the action derives from {} external source(s)",
                    taint.len()
                ));
            }
            let alternative = if prefixes.is_empty() {
                "this actor writes nowhere; read, or hand the change to an actor with a domain"
                    .to_owned()
            } else {
                format!("write under one of: {}", prefixes.join(", "))
            };
            GateOutcome::Deny {
                refusal: Box::new(
                    AxError::refusal(
                        AxCode::OutsideWriteDomain,
                        "write file",
                        target.as_str(),
                        GateRefusal::new(
                            "writes land inside the write domain",
                            violation,
                            alternative,
                        ),
                    )
                    .with_nearby(prefixes),
                ),
            }
        }
    }
}

/// Where an egress points, as classified by the effect layer after
/// address resolution; kernel never resolves names.
///
/// Non-exhaustive as the specification has always said it is: the kinds
/// of destination a city can distinguish grow, and R1.13 added the
/// fourth.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EgressTarget {
    Loopback,
    Private,
    Public {
        host: String,
    },
    /// An external server this building configured. The city names the
    /// connector rather than a host because a connector's own
    /// destination is not visible from here: claiming a host would be
    /// inventing one, and claiming loopback would understate where the
    /// bytes can end up.
    Connector {
        label: crate::tool::ServerLabel,
    },
}

/// Deliberately exhaustive.
#[derive(Debug)]
pub enum EgressOutcome {
    Allow {
        /// True exactly once per run: the pipeline hangs the net notice
        /// on this flag (4.3). Loopback and private targets never set it.
        first_public_egress: bool,
    },
    Deny {
        refusal: Box<AxError>,
    },
}

/// The Egress door: secret shapes never leave (C13), and the first
/// public egress of a run is named. The subject counts spans and offsets
/// — the matched bytes never appear (the error message is itself an
/// egress surface).
pub fn egress(
    spans: &[SecretSpan],
    target: &EgressTarget,
    prior_public_egress: bool,
) -> EgressOutcome {
    if !spans.is_empty() {
        let offsets: Vec<String> = spans
            .iter()
            .map(|s| format!("{}+{}", s.start, s.len))
            .collect();
        return EgressOutcome::Deny {
            refusal: Box::new(AxError::refusal(
                AxCode::SecretEgress,
                "send bytes out",
                format!(
                    "{} secret-shaped span(s) at {}",
                    spans.len(),
                    offsets.join(", ")
                ),
                GateRefusal::new(
                    "credentials leave only as secret: references (C13)",
                    format!("the payload carries {} secret-shaped span(s)", spans.len()),
                    "replace each span with its secret:<realm>/<name> reference; \
                     if a real credential already left, rotation is the only remedy",
                ),
            )),
        };
    }
    // A connector counts as leaving, for the same reason a public host
    // does: the run has reached a service outside itself, and the one
    // notice a run gets about that must not depend on whether the city
    // could name the far end.
    let leaves_the_machine = matches!(
        target,
        EgressTarget::Public { .. } | EgressTarget::Connector { .. }
    );
    let first_public_egress = leaves_the_machine && !prior_public_egress;
    EgressOutcome::Allow {
        first_public_egress,
    }
}

/// Which hosts a run may reach. A suffix list, because a building names
/// domains rather than addresses, and an empty list is the honest way to
/// say "nothing public" — a confidential building holds exactly this.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct EgressAllowlist {
    suffixes: Vec<String>,
}

impl EgressAllowlist {
    /// Builds the list. Entries are lowercased and stripped of a leading
    /// dot, so `.example.com` and `example.com` are the same rule rather
    /// than two rules with different behaviour.
    #[must_use]
    pub fn new(entries: Vec<String>) -> EgressAllowlist {
        let mut suffixes: Vec<String> = entries
            .into_iter()
            .map(|entry| entry.trim().trim_start_matches('.').to_ascii_lowercase())
            .filter(|entry| !entry.is_empty())
            .collect();
        suffixes.sort();
        suffixes.dedup();
        EgressAllowlist { suffixes }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.suffixes.is_empty()
    }

    pub fn entries(&self) -> impl Iterator<Item = &str> {
        self.suffixes.iter().map(String::as_str)
    }

    /// True when `host` is the domain itself or one of its subdomains.
    /// Matching is on label boundaries: `notexample.com` does not match
    /// `example.com`, which a plain suffix test would let through.
    #[must_use]
    pub fn admits(&self, host: &str) -> bool {
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        self.suffixes.iter().any(|allowed| {
            host == *allowed
                || host
                    .strip_suffix(allowed.as_str())
                    .is_some_and(|head| head.ends_with('.'))
        })
    }
}

/// The destination half of the egress question: may this run reach this
/// host at all? The payload half stays with [`egress`] — "may these bytes
/// leave" and "may this host be reached" are two questions, and a caller
/// that asks only one of them should not look as if it asked both.
///
/// Loopback and private targets are not on the list: a local model or a
/// machine on the same network is not egress, and putting them behind a
/// domain list would mean a confidential building could not reach its own
/// inference server.
#[must_use]
pub fn egress_target(list: &EgressAllowlist, target: &EgressTarget) -> EgressOutcome {
    // A connector is admitted by the building's own server table, frozen
    // at run start; this list answers a different question, about hosts,
    // and a connector label is not a host.
    if let EgressTarget::Connector { .. } = target {
        return EgressOutcome::Allow {
            first_public_egress: true,
        };
    }
    let EgressTarget::Public { host } = target else {
        return EgressOutcome::Allow {
            first_public_egress: false,
        };
    };
    if list.admits(host) {
        return EgressOutcome::Allow {
            first_public_egress: true,
        };
    }
    let known: Vec<&str> = list.entries().collect();
    let alternative = if known.is_empty() {
        "this building reaches nothing public; add a domain to its egress list, \
         or do the work in a building that has one"
            .to_owned()
    } else {
        format!("reachable domains here: {}", known.join(", "))
    };
    EgressOutcome::Deny {
        refusal: Box::new(AxError::refusal(
            AxCode::GateDenied,
            "reach a host outside this building's egress list",
            host.clone(),
            GateRefusal::new(
                "a building reaches only the domains it names",
                format!("{host} is not one of them"),
                alternative,
            ),
        )),
    }
}

/// The Spend door: exhaustion is an approval, not an error (6.3). The
/// run parks at the boundary without burning tokens; approval resumes it
/// in place.
pub fn spend(
    ladder: &BudgetLadder,
    cost: &BudgetUse,
    taint: &TaintSet,
    ctx: &GateContext,
    artifact: &Locator,
) -> GateOutcome {
    match crate::budget::admit_spend(ladder, cost) {
        crate::budget::SpendVerdict::Admit => GateOutcome::Allow,
        crate::budget::SpendVerdict::Exhausted { layer } => {
            let layer_name = match layer {
                BudgetLayer::City => "city",
                BudgetLayer::Building => "building",
                BudgetLayer::Run => "run",
            };
            GateOutcome::Escalate {
                item: item(
                    ctx,
                    ApprovalClass::BudgetLimit,
                    layer_name.to_owned(),
                    format!("continue past the {layer_name} budget"),
                    artifact.clone(),
                    !taint.is_empty(),
                ),
            }
        }
    }
}

/// A human ruling on a commitment, when one exists.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CommitmentDecision {
    Approved,
    Denied,
}

/// The Commitment door: outward promises pass a human, always (9.1's
/// first must-pass class; no Policy variant can waive it). No ruling
/// pre-blocks; a denial is final for this action.
pub fn commitment(
    decision: Option<&CommitmentDecision>,
    taint: &TaintSet,
    ctx: &GateContext,
    action_desc: &str,
    artifact: &Locator,
) -> GateOutcome {
    match decision {
        None => GateOutcome::Escalate {
            item: item(
                ctx,
                ApprovalClass::Commitment,
                action_desc.to_owned(),
                action_desc.to_owned(),
                artifact.clone(),
                !taint.is_empty(),
            ),
        },
        Some(CommitmentDecision::Approved) => GateOutcome::Allow,
        Some(CommitmentDecision::Denied) => GateOutcome::Deny {
            refusal: Box::new(
                AxError::failure(AxCode::ApprovalDenied, "commit outward", action_desc)
                    .with_recovery(
                        "the human refused this commitment; change the plan or ask \
                         with a different, clearly scoped item",
                    ),
            ),
        },
    }
}

/// The Discard door: wholly delegated to `discard::decide` (single
/// authority); this wrapper only shapes the refusal and the item.
pub fn discard(
    req: &DiscardRequest,
    registry: &Registry,
    ctx: &GateContext,
    action_desc: &str,
    artifact: &Locator,
) -> GateOutcome {
    match decide(req, registry) {
        DiscardVerdict::Allow => GateOutcome::Allow,
        DiscardVerdict::Escalate { reason } => {
            let (detail, tainted) = match reason {
                EscalateReason::Tainted => ("tainted", true),
                EscalateReason::FilesOverMax => ("files_over_max", false),
                EscalateReason::BytesOverMax => ("bytes_over_max", false),
                EscalateReason::RegistryAsset => ("registry_asset", false),
            };
            GateOutcome::Escalate {
                item: item(
                    ctx,
                    ApprovalClass::DiscardEscalate,
                    detail.to_owned(),
                    action_desc.to_owned(),
                    artifact.clone(),
                    tainted,
                ),
            }
        }
        DiscardVerdict::Deny { reason } => {
            let DenyReason::NoRestoration = reason;
            GateOutcome::Deny {
                refusal: Box::new(AxError::refusal(
                    AxCode::DiscardIrreversible,
                    "discard files",
                    action_desc,
                    GateRefusal::new(
                        "every discard carries a resolvable restoration (C14)",
                        "this request names no restoration plan",
                        "split the batch under the thresholds, or inter the originals \
                         in CAS (Interred) and retry with that locator",
                    ),
                )),
            }
        }
    }
}

/// The Delegation door: a second agent starts because a person allowed
/// it, not because a prompt asked politely.
///
/// Always escalates. Whether this person has already said yes is the
/// caller's own record of what was granted - the same shape the write
/// door uses - and keeping the two apart is what lets one answer cover a
/// cluster instead of one call.
///
/// The cluster detail is the *asking* address rather than the room being
/// opened: the person is being asked whether this resident may hand work
/// to anybody, which is the question, and asking again per room would
/// train them to click through it.
pub fn delegation(
    ctx: &GateContext,
    asking: &Address,
    room: &Address,
    artifact: &Locator,
    taint: &TaintSet,
) -> GateOutcome {
    GateOutcome::Escalate {
        item: item(
            ctx,
            ApprovalClass::Delegation,
            asking.as_str().to_owned(),
            format!(
                "{} wants to hand work to another agent, in {}",
                asking.as_str(),
                room.as_str()
            ),
            artifact.clone(),
            !taint.is_empty(),
        ),
    }
}

/// The spawn admission: delegates do not delegate (10.1). The refusal
/// teaches the alternative instead of hiding the tool.
pub fn spawn(parent: Depth, kind: &DelegateKind) -> GateOutcome {
    match admit_delegation(parent, kind) {
        DelegationVerdict::Allow => GateOutcome::Allow,
        DelegationVerdict::Deny => GateOutcome::Deny {
            refusal: Box::new(AxError::refusal(
                AxCode::DelegationDepth,
                "spawn delegate",
                format!("{kind:?}"),
                GateRefusal::new(
                    "delegates do not delegate: one level deep",
                    "a delegated position requested a spawn",
                    "return this subtask to the resident who spawned you; \
                     that resident can delegate it",
                ),
            )),
        },
    }
}

/// Deliberately exhaustive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DedupVerdict {
    Fresh,
    Duplicate,
}

/// Idempotent dedup: judged before any unreplayable side effect —
/// decrypting, billing, outward delivery all come after (8.2). The seen
/// set is the caller's state; kernel only judges membership.
pub fn dedup(seen: &BTreeSet<IdemKey>, key: &IdemKey) -> DedupVerdict {
    if seen.contains(key) {
        DedupVerdict::Duplicate
    } else {
        DedupVerdict::Fresh
    }
}

#[cfg(kani)]
mod verification {
    //! V5: the composed doors fail closed — the mapping from module
    //! verdicts to gate outcomes never invents an Allow.

    use super::*;

    #[kani::proof]
    fn a_reserved_target_never_allows() {
        let domain_set = WriteDomain::new(vec![Address::parse("b").unwrap()]).unwrap();
        let target = Address::parse(".sprawling/ledger").unwrap();
        assert!(!matches!(
            domain(&domain_set, &target, &TaintSet::empty()),
            GateOutcome::Allow
        ));
    }

    #[kani::proof]
    fn secret_spans_never_allow() {
        let spans = [SecretSpan {
            start: kani::any(),
            len: kani::any(),
            provider: None,
        }];
        let target = EgressTarget::Loopback;
        assert!(matches!(
            egress(&spans, &target, kani::any()),
            EgressOutcome::Deny { .. }
        ));
    }

    #[kani::proof]
    fn a_delegated_spawn_never_allows() {
        let kind = if kani::any() {
            DelegateKind::Resident
        } else {
            DelegateKind::Ephemeral
        };
        assert!(!matches!(
            spawn(Depth::Delegated, &kind),
            GateOutcome::Allow
        ));
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
    #[test]
    fn an_allowlist_matches_on_label_boundaries_not_on_text() {
        let list = EgressAllowlist::new(vec![".Example.com".to_owned(), "docs.rs".to_owned()]);
        assert!(list.admits("example.com"));
        assert!(list.admits("api.example.com"));
        assert!(list.admits("EXAMPLE.COM"), "hosts are case-insensitive");
        assert!(
            !list.admits("notexample.com"),
            "a suffix test would allow this"
        );
        assert!(!list.admits("example.com.evil.test"));
    }

    #[test]
    fn a_host_outside_the_list_is_refused_in_three_parts_that_name_the_way_out() {
        let list = EgressAllowlist::new(vec!["example.com".to_owned()]);
        let target = EgressTarget::Public {
            host: "pastebin.test".to_owned(),
        };
        let EgressOutcome::Deny { refusal } = egress_target(&list, &target) else {
            panic!("an unlisted host is refused");
        };
        assert!(refusal.to_string().contains("pastebin.test"));
        let gate = refusal.gate().unwrap();
        assert!(gate.alternative().contains("example.com"));
    }

    #[test]
    fn an_empty_list_reaches_nothing_public_and_says_what_to_do_instead() {
        let list = EgressAllowlist::default();
        let EgressOutcome::Deny { refusal } = egress_target(
            &list,
            &EgressTarget::Public {
                host: "example.com".to_owned(),
            },
        ) else {
            panic!("a building with no list reaches nothing public");
        };
        let gate = refusal.gate().unwrap();
        assert!(gate.alternative().contains("building that has one"));
    }

    #[test]
    fn local_and_private_targets_are_not_egress_at_all() {
        let list = EgressAllowlist::default();
        assert!(matches!(
            egress_target(&list, &EgressTarget::Loopback),
            EgressOutcome::Allow { .. }
        ));
        assert!(
            matches!(
                egress_target(&list, &EgressTarget::Private),
                EgressOutcome::Allow { .. }
            ),
            "a confidential building must still reach its own inference server"
        );
    }

    use super::*;
    use crate::budget::{BudgetCap, BudgetLevel, Tokens, UsdMicros};
    use crate::taint::TaintSource;

    fn ctx() -> GateContext {
        GateContext {
            actor: "worker@sim.1".into(),
            now: TimeMs::new(42),
            item_id: ApprovalId::new("item-7").unwrap(),
        }
    }

    fn artifact() -> Locator {
        Locator::parse(&format!("cas:b3-{}", "aa".repeat(32))).unwrap()
    }

    fn assert_three_parts(outcome: &GateOutcome, code: AxCode) {
        match outcome {
            GateOutcome::Deny { refusal } => {
                assert_eq!(refusal.code(), &code);
                let gate = refusal.gate().expect("gate refusals carry three parts");
                assert!(!gate.rule().is_empty());
                assert!(!gate.violation().is_empty());
                assert!(!gate.alternative().is_empty());
            }
            _ => panic!("expected a refusal"),
        }
    }

    #[test]
    fn the_domain_door_refuses_with_prefixes_as_nearby() {
        let wd = WriteDomain::new(vec![Address::parse("b1/room").unwrap()]).unwrap();
        let inside = Address::parse("b1/room/notes.md").unwrap();
        assert!(matches!(
            domain(&wd, &inside, &TaintSet::empty()),
            GateOutcome::Allow
        ));
        let outside = Address::parse("b2/other.md").unwrap();
        let outcome = domain(&wd, &outside, &TaintSet::empty());
        assert_three_parts(&outcome, AxCode::OutsideWriteDomain);
        let GateOutcome::Deny { refusal } = outcome else {
            panic!("refusal shape asserted above")
        };
        assert_eq!(refusal.nearby(), ["b1/room"]);
        // Tainted violations name their provenance in the violation part.
        let tainted = TaintSet::of(TaintSource::new("web:evil").unwrap());
        let outcome = domain(&wd, &outside, &tainted);
        let GateOutcome::Deny { refusal } = outcome else {
            panic!("refusal shape asserted above")
        };
        assert!(
            refusal
                .gate()
                .unwrap()
                .violation()
                .contains("external source")
        );
    }

    #[test]
    fn the_egress_door_denies_spans_and_flags_the_first_public_hop() {
        let span = SecretSpan {
            start: 10,
            len: 40,
            provider: Some("anthropic"),
        };
        let outcome = egress(
            std::slice::from_ref(&span),
            &EgressTarget::Public {
                host: "api.example.com".into(),
            },
            false,
        );
        match outcome {
            EgressOutcome::Deny { refusal } => {
                assert_eq!(refusal.code(), &AxCode::SecretEgress);
                assert!(refusal.subject().contains("10+40"));
                assert!(!refusal.subject().contains("sk-ant"), "never echo bytes");
                assert!(refusal.gate().unwrap().alternative().contains("rotation"));
            }
            EgressOutcome::Allow { .. } => panic!("spans must deny"),
        }
        match egress(
            &[],
            &EgressTarget::Public {
                host: "x.dev".into(),
            },
            false,
        ) {
            EgressOutcome::Allow {
                first_public_egress,
            } => assert!(first_public_egress),
            EgressOutcome::Deny { .. } => panic!("clean egress allows"),
        }
        match egress(
            &[],
            &EgressTarget::Public {
                host: "x.dev".into(),
            },
            true,
        ) {
            EgressOutcome::Allow {
                first_public_egress,
            } => assert!(!first_public_egress),
            EgressOutcome::Deny { .. } => panic!(),
        }
        match egress(&[], &EgressTarget::Loopback, false) {
            EgressOutcome::Allow {
                first_public_egress,
            } => assert!(!first_public_egress, "localhost is not the internet"),
            EgressOutcome::Deny { .. } => panic!(),
        }
    }

    #[test]
    fn the_spend_door_escalates_exhaustion_as_a_budget_item() {
        let level = |cap: u64, used: u64| BudgetLevel {
            cap: BudgetCap {
                usd: UsdMicros::new(cap),
                tokens: Tokens::new(1_000_000),
            },
            used: BudgetUse {
                usd: UsdMicros::new(used),
                tokens: Tokens::new(0),
            },
        };
        let ladder = BudgetLadder {
            city: level(1000, 0),
            building: level(1000, 0),
            run: level(10, 10),
        };
        let cost = BudgetUse {
            usd: UsdMicros::new(1),
            tokens: Tokens::new(1),
        };
        match spend(&ladder, &cost, &TaintSet::empty(), &ctx(), &artifact()) {
            GateOutcome::Escalate { item } => {
                assert_eq!(item.cluster_key.class, ApprovalClass::BudgetLimit);
                assert_eq!(item.cluster_key.detail, "run");
                assert_eq!(item.source, ApprovalSource::Gate);
                assert!(!item.tainted);
            }
            _ => panic!("exhaustion escalates"),
        }
        let roomy = BudgetLadder {
            city: level(1000, 0),
            building: level(1000, 0),
            run: level(1000, 0),
        };
        assert!(matches!(
            spend(&roomy, &cost, &TaintSet::empty(), &ctx(), &artifact()),
            GateOutcome::Allow
        ));
    }

    #[test]
    fn the_commitment_door_pre_blocks_and_respects_denial() {
        let outcome = commitment(
            None,
            &TaintSet::empty(),
            &ctx(),
            "send release mail",
            &artifact(),
        );
        match outcome {
            GateOutcome::Escalate { item } => {
                assert_eq!(item.cluster_key.class, ApprovalClass::Commitment);
            }
            _ => panic!("no ruling pre-blocks"),
        }
        assert!(matches!(
            commitment(
                Some(&CommitmentDecision::Approved),
                &TaintSet::empty(),
                &ctx(),
                "send release mail",
                &artifact()
            ),
            GateOutcome::Allow
        ));
        match commitment(
            Some(&CommitmentDecision::Denied),
            &TaintSet::empty(),
            &ctx(),
            "send release mail",
            &artifact(),
        ) {
            GateOutcome::Deny { refusal } => {
                assert_eq!(refusal.code(), &AxCode::ApprovalDenied);
                assert!(refusal.gate().is_none(), "not a gate-carrier code");
            }
            _ => panic!("denied is final"),
        }
    }

    #[test]
    fn the_discard_door_maps_verdicts_and_teaches_the_alternative() {
        use crate::budget::ByteLen;
        let registry = Registry::new();
        let unplanned = DiscardRequest::Unplanned {
            paths: vec![Address::parse("b/x.md").unwrap()],
            taint: TaintSet::empty(),
            total_bytes: ByteLen::new(10),
        };
        let outcome = discard(&unplanned, &registry, &ctx(), "delete b/x.md", &artifact());
        assert_three_parts(&outcome, AxCode::DiscardIrreversible);
        let GateOutcome::Deny { refusal } = outcome else {
            panic!("refusal shape asserted above")
        };
        assert!(refusal.gate().unwrap().alternative().contains("Interred"));
    }

    #[test]
    fn the_spawn_door_teaches_the_way_back_up() {
        assert!(matches!(
            spawn(Depth::Root, &DelegateKind::Ephemeral),
            GateOutcome::Allow
        ));
        let outcome = spawn(Depth::Delegated, &DelegateKind::Ephemeral);
        assert_three_parts(&outcome, AxCode::DelegationDepth);
    }

    #[test]
    fn dedup_judges_membership_only() {
        let run = crate::event::RunId::CITY;
        let key = IdemKey::derive(&run, crate::event::Seq::FIRST, b"send mail");
        let mut seen = BTreeSet::new();
        assert_eq!(dedup(&seen, &key), DedupVerdict::Fresh);
        seen.insert(key);
        assert_eq!(dedup(&seen, &key), DedupVerdict::Duplicate);
        let other = IdemKey::derive(&run, crate::event::Seq::FIRST, b"send other mail");
        assert_eq!(dedup(&seen, &other), DedupVerdict::Fresh);
    }
}
