// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Approval and Autonomy (9.2). The three
//! must-pass-a-human classes are unrepresentable in `PolicyClass` — a
//! policy that would waive them cannot be spelled. Tainted items refuse
//! both policy waivers and delegate answers (C15). Autonomy changes who
//! answers the inbox, never what the gates decide.

use serde::{Deserialize, Serialize};

use crate::consts_policy::POLICY_IDLE_DAYS;
use crate::event::TimeMs;
use crate::locator::Locator;
use crate::registry::ResidentId;

/// Non-empty item identity; uuid v7 minting is the effect layer's.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ApprovalId(String);

impl ApprovalId {
    pub fn new(raw: impl Into<String>) -> Option<ApprovalId> {
        let raw = raw.into();
        if raw.is_empty() {
            None
        } else {
            Some(ApprovalId(raw))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Who raised it: a gate pre-block (the run waits, no tokens burn) or the
/// model's own question (batched, never blocking the current action).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalSource {
    Gate,
    Agent,
}

/// What kind of decision this is. Wire data (cluster keys serialize), so
/// open for extension; kernel's own matches stay exhaustive in-crate.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ApprovalClass {
    Commitment,
    BudgetLimit,
    DiscardEscalate,
    AgentQuestion,
    /// Rewriting the rules a scope's own runs are judged by. Like
    /// `Delegation`, no `PolicyClass` variant: a standing rule that
    /// waives changes to the rules is a rule that repeals itself.
    Governance,
    /// Handing work to a second agent. No `PolicyClass` variant, which
    /// is the type-level half of "a standing rule never grants this":
    /// the answer holds for the cluster the person was shown and
    /// expires with the process, because a permanent waiver on
    /// delegation is the one waiver that can spend without asking again.
    Delegation,
}

/// The clustering key: class + free detail. One human verdict on a key
/// can become a Policy — for the one class that admits policies.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ClusterKey {
    pub class: ApprovalClass,
    pub detail: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ApprovalItem {
    pub id: ApprovalId,
    pub source: ApprovalSource,
    pub actor: String,
    pub action_desc: String,
    pub artifact: Locator,
    pub cluster_key: ClusterKey,
    pub created: TimeMs,
    /// C15's marker bit: a tainted item takes no policy and no delegate.
    pub tainted: bool,
}

/// The classes a Policy may match. Commitment, BudgetLimit and
/// DiscardEscalate have no variant here — the type-level half of "never
/// waivable" (9.1); gate code never needs a runtime check for it.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyClass {
    AgentQuestion,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PolicyMatcher {
    pub class: PolicyClass,
    /// Matches cluster-key details by prefix; empty matches the class.
    pub detail_prefix: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyVerdict {
    Allow,
    Deny,
}

/// A waiver that expires. No `revocable` field: it is always true, and a
/// field that can only hold one value is a place to store a lie.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Policy {
    pub id: String,
    pub matcher: PolicyMatcher,
    pub verdict: PolicyVerdict,
    /// The human verdict this policy was minted from.
    pub source: ApprovalId,
    pub created: TimeMs,
    pub last_hit: Option<TimeMs>,
}

/// Deliberately exhaustive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyApplication {
    Applies(PolicyVerdict),
    NotApplicable,
}

fn class_matches(policy: PolicyClass, item: ApprovalClass) -> bool {
    match policy {
        PolicyClass::AgentQuestion => matches!(item, ApprovalClass::AgentQuestion),
    }
}

/// Tainted items never take a policy (C15); class and detail prefix must
/// both hold.
pub fn match_item(policy: &Policy, item: &ApprovalItem) -> PolicyApplication {
    if item.tainted {
        return PolicyApplication::NotApplicable;
    }
    if !class_matches(policy.matcher.class, item.cluster_key.class) {
        return PolicyApplication::NotApplicable;
    }
    if !item
        .cluster_key
        .detail
        .starts_with(&policy.matcher.detail_prefix)
    {
        return PolicyApplication::NotApplicable;
    }
    PolicyApplication::Applies(policy.verdict)
}

/// Deliberately exhaustive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PolicyExpiry {
    Active,
    Expired,
}

/// Idle policies expire: an unused waiver is standing
/// risk, and a stock of them fakes the coverage metric. Clock skew (now
/// before the last reference) reads Active — expiry is for age, not for
/// broken clocks.
pub fn expiry(policy: &Policy, now: TimeMs) -> PolicyExpiry {
    let reference = policy
        .last_hit
        .map_or(policy.created, |hit| hit.max(policy.created));
    let Some(idle_ms) = now.value().checked_sub(reference.value()) else {
        return PolicyExpiry::Active;
    };
    let threshold_ms = u64::from(POLICY_IDLE_DAYS).saturating_mul(86_400_000);
    if idle_ms >= threshold_ms {
        PolicyExpiry::Expired
    } else {
        PolicyExpiry::Active
    }
}

/// Why a policy left the books (`policy_revoked` payload).
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PolicyRevocation {
    Revoked,
    Expired,
    Superseded,
}

/// Who answers the Approval Inbox (9.2). Never touches gate decisions —
/// C15's byte-identical gate sequences are citysim's to assert (P2).
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Autonomy {
    Owner,
    Delegate(ResidentId),
    Deferred,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Answerer {
    Human,
    Resident(ResidentId),
}

/// Deliberately exhaustive.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnswerVerdict {
    May,
    /// The three classes and every tainted item: humans only (C15).
    HumanOnly,
    /// Self-approval is no approval.
    SelfApprovalBarred,
    /// Only the appointed delegate answers; nobody else's ruling counts.
    NotTheDelegate,
}

fn human_only(item: &ApprovalItem) -> bool {
    item.tainted
        || matches!(
            item.cluster_key.class,
            ApprovalClass::Commitment | ApprovalClass::BudgetLimit | ApprovalClass::DiscardEscalate
        )
}

/// The answering rule. Humans answer everything; a resident answers only
/// as the appointed delegate, never the three classes, never tainted
/// items, never its own actions.
pub fn may_answer(autonomy: &Autonomy, item: &ApprovalItem, answerer: &Answerer) -> AnswerVerdict {
    match answerer {
        Answerer::Human => AnswerVerdict::May,
        Answerer::Resident(resident) => {
            let appointed = matches!(autonomy, Autonomy::Delegate(d) if d == resident);
            if !appointed {
                return AnswerVerdict::NotTheDelegate;
            }
            if human_only(item) {
                return AnswerVerdict::HumanOnly;
            }
            if item.actor == resident.as_str() {
                return AnswerVerdict::SelfApprovalBarred;
            }
            AnswerVerdict::May
        }
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

    fn item(class: ApprovalClass, tainted: bool, actor: &str) -> ApprovalItem {
        ApprovalItem {
            id: ApprovalId::new("item-1").unwrap(),
            source: ApprovalSource::Gate,
            actor: actor.into(),
            action_desc: "send the release mail".into(),
            artifact: Locator::parse(&format!("cas:b3-{}", "ef".repeat(32))).unwrap(),
            cluster_key: ClusterKey {
                class,
                detail: "mail:release".into(),
            },
            created: TimeMs::new(1_000),
            tainted,
        }
    }

    fn policy(prefix: &str) -> Policy {
        Policy {
            id: "p-1".into(),
            matcher: PolicyMatcher {
                class: PolicyClass::AgentQuestion,
                detail_prefix: prefix.into(),
            },
            verdict: PolicyVerdict::Allow,
            source: ApprovalId::new("item-0").unwrap(),
            created: TimeMs::new(0),
            last_hit: None,
        }
    }

    fn resident(name: &str) -> ResidentId {
        ResidentId::new(name).unwrap()
    }

    #[test]
    fn policies_match_class_and_prefix_but_never_tainted_items() {
        let p = policy("mail:");
        assert_eq!(
            match_item(&p, &item(ApprovalClass::AgentQuestion, false, "a")),
            PolicyApplication::Applies(PolicyVerdict::Allow)
        );
        assert_eq!(
            match_item(&p, &item(ApprovalClass::AgentQuestion, true, "a")),
            PolicyApplication::NotApplicable
        );
        assert_eq!(
            match_item(
                &policy("web:"),
                &item(ApprovalClass::AgentQuestion, false, "a")
            ),
            PolicyApplication::NotApplicable
        );
        // Commitment/BudgetLimit/DiscardEscalate: no PolicyClass variant
        // exists to even write such a matcher — pinned by S2.11 trybuild.
        assert_eq!(
            match_item(&p, &item(ApprovalClass::Commitment, false, "a")),
            PolicyApplication::NotApplicable
        );
    }

    #[test]
    fn idle_policies_expire_and_skewed_clocks_do_not() {
        const NINETY_DAYS_MS: u64 = 90 * 86_400_000;
        let mut p = policy("");
        assert_eq!(expiry(&p, TimeMs::new(0)), PolicyExpiry::Active);
        assert_eq!(
            expiry(&p, TimeMs::new(NINETY_DAYS_MS)),
            PolicyExpiry::Expired
        );
        p.last_hit = Some(TimeMs::new(NINETY_DAYS_MS));
        assert_eq!(
            expiry(&p, TimeMs::new(NINETY_DAYS_MS.saturating_add(10))),
            PolicyExpiry::Active
        );
        // now before created: skew reads Active, never a panic.
        let skewed = Policy {
            created: TimeMs::new(100),
            ..policy("")
        };
        assert_eq!(expiry(&skewed, TimeMs::new(50)), PolicyExpiry::Active);
    }

    #[test]
    fn the_answer_matrix_holds() {
        let delegate = resident("judge@b.1");
        let autonomy = Autonomy::Delegate(delegate.clone());
        let question = item(ApprovalClass::AgentQuestion, false, "worker@b.2");
        // Humans always may.
        assert_eq!(
            may_answer(&Autonomy::Owner, &question, &Answerer::Human),
            AnswerVerdict::May
        );
        // The appointed delegate may answer an ordinary question.
        assert_eq!(
            may_answer(&autonomy, &question, &Answerer::Resident(delegate.clone())),
            AnswerVerdict::May
        );
        // Not appointed: someone else's ruling counts for nothing.
        assert_eq!(
            may_answer(
                &autonomy,
                &question,
                &Answerer::Resident(resident("other@b.9"))
            ),
            AnswerVerdict::NotTheDelegate
        );
        // Owner mode: no resident is appointed.
        assert_eq!(
            may_answer(
                &Autonomy::Owner,
                &question,
                &Answerer::Resident(delegate.clone())
            ),
            AnswerVerdict::NotTheDelegate
        );
        // The three classes and tainted items are human-only.
        for class in [
            ApprovalClass::Commitment,
            ApprovalClass::BudgetLimit,
            ApprovalClass::DiscardEscalate,
        ] {
            assert_eq!(
                may_answer(
                    &autonomy,
                    &item(class, false, "worker@b.2"),
                    &Answerer::Resident(delegate.clone())
                ),
                AnswerVerdict::HumanOnly
            );
        }
        assert_eq!(
            may_answer(
                &autonomy,
                &item(ApprovalClass::AgentQuestion, true, "worker@b.2"),
                &Answerer::Resident(delegate.clone())
            ),
            AnswerVerdict::HumanOnly
        );
        // Self-approval is no approval.
        assert_eq!(
            may_answer(
                &autonomy,
                &item(ApprovalClass::AgentQuestion, false, "judge@b.1"),
                &Answerer::Resident(delegate)
            ),
            AnswerVerdict::SelfApprovalBarred
        );
    }
}
