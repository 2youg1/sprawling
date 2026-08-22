// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! AxError, the one error shape of the whole city, and
//! AxCode, the closed set of 35 error codes.
//!
//! Invariants owned here:
//! - every wire spelling (`E_…`) has exactly one authority: [`AxCode::as_str`];
//!   serde delegates to it in both directions, so enum and wire cannot drift.
//! - a gate refusal always carries the three mandatory parts (rule,
//!   violation, alternative): [`AxError::refusal`] is the only path that
//!   sets them and [`AxError::failure`] cannot.
//! - `retriable` defaults to false; a caller must opt in explicitly
//!   (fail-closed).
//!
//! The carrier-event declaration (`AxCode::carrier`) lands together with
//! `kernel::event` (card S1.03) because it names `EventKind`.

use serde::{Deserialize, Serialize};

use crate::event::EventKind;

/// Where an AxCode surfaces in history: its carrier event, or the loadtime
/// class (the closed C9 exception: the Ledger is not writable yet).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Carrier {
    Event(EventKind),
    Loadtime,
}

/// Closed set of error codes.
/// Extension is additive only; the wire spelling lives in [`AxCode::as_str`].
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum AxCode {
    // Base table (14).
    PathNotFound,
    ToolUnknown,
    ToolUnavailable,
    InvalidArgs,
    OutsideWriteDomain,
    VersionConflict,
    GateDenied,
    BudgetExhausted,
    Timeout,
    Provider,
    EvidenceMissing,
    LoopSuspected,
    LocatorInvalid,
    SandboxDenied,
    // Collaboration (5). A sixth, `SignalUnknown`, was defined away in
    // P3.01: signal payloads are written and read by one module, the
    // kind is an exhaustive enum, and a kind this version does not know
    // can only come from a newer binary's ledger — which the version
    // door already refuses.
    DraftStale,
    GoalConflict,
    TaintedAction,
    RepairBusy,
    DelegationDepth,
    // Governance and facilities (13).
    ApprovalPending,
    ApprovalDenied,
    CrossBuildingDenied,
    DigestSuspect,
    CredentialMissing,
    ConfigInvalid,
    CasCorrupt,
    StorageFatal,
    WorktreeBusy,
    BrowserUnavailable,
    EndpointDialectUnsupported,
    WireMismatch,
    LogVersionUnsupported,
    // Privacy and Discard (2).
    SecretEgress,
    DiscardIrreversible,
    // Unknown outcome (1).
    ToolOutcomeUnknown,
}

impl AxCode {
    /// Every code, in the order the SPEC table lists them. Data face for tests and
    /// (from S2 on) `xtask specalign`.
    pub const ALL: [AxCode; 35] = [
        AxCode::PathNotFound,
        AxCode::ToolUnknown,
        AxCode::ToolUnavailable,
        AxCode::InvalidArgs,
        AxCode::OutsideWriteDomain,
        AxCode::VersionConflict,
        AxCode::GateDenied,
        AxCode::BudgetExhausted,
        AxCode::Timeout,
        AxCode::Provider,
        AxCode::EvidenceMissing,
        AxCode::LoopSuspected,
        AxCode::LocatorInvalid,
        AxCode::SandboxDenied,
        AxCode::DraftStale,
        AxCode::GoalConflict,
        AxCode::TaintedAction,
        AxCode::RepairBusy,
        AxCode::DelegationDepth,
        AxCode::ApprovalPending,
        AxCode::ApprovalDenied,
        AxCode::CrossBuildingDenied,
        AxCode::DigestSuspect,
        AxCode::CredentialMissing,
        AxCode::ConfigInvalid,
        AxCode::CasCorrupt,
        AxCode::StorageFatal,
        AxCode::WorktreeBusy,
        AxCode::BrowserUnavailable,
        AxCode::EndpointDialectUnsupported,
        AxCode::WireMismatch,
        AxCode::LogVersionUnsupported,
        AxCode::SecretEgress,
        AxCode::DiscardIrreversible,
        AxCode::ToolOutcomeUnknown,
    ];

    /// The wire spelling. Sole spelling authority; serde and Display reuse it.
    pub fn as_str(&self) -> &'static str {
        match self {
            AxCode::PathNotFound => "E_PATH_NOT_FOUND",
            AxCode::ToolUnknown => "E_TOOL_UNKNOWN",
            AxCode::ToolUnavailable => "E_TOOL_UNAVAILABLE",
            AxCode::InvalidArgs => "E_INVALID_ARGS",
            AxCode::OutsideWriteDomain => "E_OUTSIDE_WRITE_DOMAIN",
            AxCode::VersionConflict => "E_VERSION_CONFLICT",
            AxCode::GateDenied => "E_GATE_DENIED",
            AxCode::BudgetExhausted => "E_BUDGET_EXHAUSTED",
            AxCode::Timeout => "E_TIMEOUT",
            AxCode::Provider => "E_PROVIDER",
            AxCode::EvidenceMissing => "E_EVIDENCE_MISSING",
            AxCode::LoopSuspected => "E_LOOP_SUSPECTED",
            AxCode::LocatorInvalid => "E_LOCATOR_INVALID",
            AxCode::SandboxDenied => "E_SANDBOX_DENIED",
            AxCode::DraftStale => "E_DRAFT_STALE",
            AxCode::GoalConflict => "E_GOAL_CONFLICT",
            AxCode::TaintedAction => "E_TAINTED_ACTION",
            AxCode::RepairBusy => "E_REPAIR_BUSY",
            AxCode::DelegationDepth => "E_DELEGATION_DEPTH",
            AxCode::ApprovalPending => "E_APPROVAL_PENDING",
            AxCode::ApprovalDenied => "E_APPROVAL_DENIED",
            AxCode::CrossBuildingDenied => "E_CROSS_BUILDING_DENIED",
            AxCode::DigestSuspect => "E_DIGEST_SUSPECT",
            AxCode::CredentialMissing => "E_CREDENTIAL_MISSING",
            AxCode::ConfigInvalid => "E_CONFIG_INVALID",
            AxCode::CasCorrupt => "E_CAS_CORRUPT",
            AxCode::StorageFatal => "E_STORAGE_FATAL",
            AxCode::WorktreeBusy => "E_WORKTREE_BUSY",
            AxCode::BrowserUnavailable => "E_BROWSER_UNAVAILABLE",
            AxCode::EndpointDialectUnsupported => "E_ENDPOINT_DIALECT_UNSUPPORTED",
            AxCode::WireMismatch => "E_WIRE_MISMATCH",
            AxCode::LogVersionUnsupported => "E_LOG_VERSION_UNSUPPORTED",
            AxCode::SecretEgress => "E_SECRET_EGRESS",
            AxCode::DiscardIrreversible => "E_DISCARD_IRREVERSIBLE",
            AxCode::ToolOutcomeUnknown => "E_TOOL_OUTCOME_UNKNOWN",
        }
    }

    /// Wire spelling back to code; `None` is the caller's fail-closed branch.
    pub fn parse(raw: &str) -> Option<AxCode> {
        AxCode::ALL.into_iter().find(|code| code.as_str() == raw)
    }

    /// The carrier declaration (C9): which event carries this code into
    /// history. Sole declaration site, exhaustive on purpose — a new code
    /// without a carrier decision is a compile error. The loadtime arm is
    /// the closed five-code whitelist and must not grow (fifth code by
    /// S2 stage-opening verdict: storage write failure is process-fatal).
    pub fn carrier(&self) -> Carrier {
        match self {
            AxCode::GateDenied
            | AxCode::OutsideWriteDomain
            | AxCode::TaintedAction
            | AxCode::CrossBuildingDenied
            | AxCode::DiscardIrreversible
            | AxCode::SecretEgress
            | AxCode::DelegationDepth => Carrier::Event(EventKind::GateDenied),
            AxCode::ApprovalPending => Carrier::Event(EventKind::ApprovalRequested),
            AxCode::ApprovalDenied => Carrier::Event(EventKind::ApprovalResolved),
            AxCode::BudgetExhausted => Carrier::Event(EventKind::BudgetLimit),
            AxCode::LoopSuspected => Carrier::Event(EventKind::WatchdogFired),
            AxCode::Provider => Carrier::Event(EventKind::ProviderDegraded),
            AxCode::EndpointDialectUnsupported => Carrier::Event(EventKind::EndpointLost),
            AxCode::ConfigInvalid
            | AxCode::CasCorrupt
            | AxCode::StorageFatal
            | AxCode::WireMismatch
            | AxCode::LogVersionUnsupported => Carrier::Loadtime,
            AxCode::PathNotFound
            | AxCode::ToolUnknown
            | AxCode::ToolUnavailable
            | AxCode::InvalidArgs
            | AxCode::VersionConflict
            | AxCode::Timeout
            | AxCode::EvidenceMissing
            | AxCode::LocatorInvalid
            | AxCode::SandboxDenied
            | AxCode::DraftStale
            | AxCode::GoalConflict
            | AxCode::RepairBusy
            | AxCode::DigestSuspect
            | AxCode::CredentialMissing
            | AxCode::WorktreeBusy
            | AxCode::BrowserUnavailable
            | AxCode::ToolOutcomeUnknown => Carrier::Event(EventKind::ToolResult),
        }
    }
}

impl std::fmt::Display for AxCode {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl Serialize for AxCode {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(self.as_str())
    }
}

impl<'de> Deserialize<'de> for AxCode {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        AxCode::parse(&raw)
            .ok_or_else(|| serde::de::Error::custom(format_args!("unknown AxCode `{raw}`")))
    }
}

/// The three mandatory parts of a gate refusal: rule | violation |
/// compliant alternative (three-part refusal).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GateRefusal {
    rule: String,
    violation: String,
    alternative: String,
}

impl GateRefusal {
    pub fn new(
        rule: impl Into<String>,
        violation: impl Into<String>,
        alternative: impl Into<String>,
    ) -> Self {
        GateRefusal {
            rule: rule.into(),
            violation: violation.into(),
            alternative: alternative.into(),
        }
    }

    pub fn rule(&self) -> &str {
        &self.rule
    }

    pub fn violation(&self) -> &str {
        &self.violation
    }

    pub fn alternative(&self) -> &str {
        &self.alternative
    }
}

/// The unified error shape: seven wire fields, serialized in declaration
/// order (determinism rule 6). The model is the recovery subject: `nearby`
/// and `recovery` must hold directly executable information, not apologies.
///
/// Everything but `code` sits behind one Box so the type stays cheap in
/// every seam's return slot (`result_large_err`); serde flatten keeps the
/// wire shape flat and the field order unchanged.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, thiserror::Error)]
#[error("{code}: cannot {} on {}", detail.action, detail.subject)]
pub struct AxError {
    code: AxCode,
    #[serde(flatten)]
    detail: Box<ErrorDetail>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
struct ErrorDetail {
    action: String,
    subject: String,
    nearby: Vec<String>,
    recovery: String,
    retriable: bool,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    gate: Option<GateRefusal>,
}

impl AxError {
    /// Non-gate failure. `retriable` starts false (fail-closed); `nearby`
    /// and `recovery` start empty and grow via the builders.
    pub fn failure(code: AxCode, action: impl Into<String>, subject: impl Into<String>) -> Self {
        AxError {
            code,
            detail: Box::new(ErrorDetail {
                action: action.into(),
                subject: subject.into(),
                nearby: Vec::new(),
                recovery: String::new(),
                retriable: false,
                gate: None,
            }),
        }
    }

    /// Gate refusal: the only constructor that sets the three mandatory
    /// parts. Gate-carrier codes must come through here (kernel::gate is
    /// their sole producer from S2 on).
    pub fn refusal(
        code: AxCode,
        action: impl Into<String>,
        subject: impl Into<String>,
        gate: GateRefusal,
    ) -> Self {
        let mut err = AxError::failure(code, action, subject);
        err.detail.gate = Some(gate);
        err
    }

    pub fn with_nearby(mut self, nearby: Vec<String>) -> Self {
        self.detail.nearby = nearby;
        self
    }

    pub fn with_recovery(mut self, recovery: impl Into<String>) -> Self {
        self.detail.recovery = recovery.into();
        self
    }

    /// Declares the action safe to retry as-is. Explicit opt-in only.
    pub fn retriable(mut self) -> Self {
        self.detail.retriable = true;
        self
    }

    pub fn code(&self) -> &AxCode {
        &self.code
    }

    pub fn action(&self) -> &str {
        &self.detail.action
    }

    pub fn subject(&self) -> &str {
        &self.detail.subject
    }

    pub fn nearby(&self) -> &[String] {
        &self.detail.nearby
    }

    pub fn recovery(&self) -> &str {
        &self.detail.recovery
    }

    pub fn is_retriable(&self) -> bool {
        self.detail.retriable
    }

    pub fn gate(&self) -> Option<&GateRefusal> {
        self.detail.gate.as_ref()
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
    use std::collections::BTreeSet;

    #[test]
    fn axcode_is_35_and_spelling_is_bijective() {
        assert_eq!(AxCode::ALL.len(), 35);
        let spellings: BTreeSet<&str> = AxCode::ALL.iter().map(AxCode::as_str).collect();
        assert_eq!(spellings.len(), 35);
        for s in &spellings {
            assert!(s.starts_with("E_"));
        }
    }

    #[test]
    fn axcode_parse_roundtrips_every_variant() {
        for code in AxCode::ALL {
            assert_eq!(AxCode::parse(code.as_str()), Some(code));
        }
        assert_eq!(AxCode::parse("E_NO_SUCH_CODE"), None);
    }

    #[test]
    fn axcode_serde_uses_the_wire_spelling() {
        let json = serde_json::to_string(&AxCode::PathNotFound).unwrap();
        assert_eq!(json, "\"E_PATH_NOT_FOUND\"");
        let back: AxCode = serde_json::from_str("\"E_LOCATOR_INVALID\"").unwrap();
        assert_eq!(back, AxCode::LocatorInvalid);
        assert!(serde_json::from_str::<AxCode>("\"E_BOGUS\"").is_err());
    }

    #[test]
    fn failure_has_no_gate_and_is_not_retriable_by_default() {
        let err = AxError::failure(AxCode::PathNotFound, "read file", "docs/a.md");
        assert!(err.gate().is_none());
        assert!(!err.is_retriable());
        assert_eq!(err.code(), &AxCode::PathNotFound);
    }

    #[test]
    fn refusal_carries_the_three_mandatory_parts() {
        let gate = GateRefusal::new("rule", "violation", "alternative");
        let err = AxError::refusal(AxCode::GateDenied, "wire funds", "acct-9", gate);
        let got = err.gate().expect("gate refusal must carry the three parts");
        assert_eq!(got.rule(), "rule");
        assert_eq!(got.violation(), "violation");
        assert_eq!(got.alternative(), "alternative");
    }

    #[test]
    fn builders_extend_without_touching_the_rest() {
        let err = AxError::failure(AxCode::ToolUnknown, "call tool", "grep")
            .with_nearby(vec!["exec".into(), "edit".into(), "status".into()])
            .with_recovery("use an L0 tool")
            .retriable();
        assert!(err.is_retriable());
        let json = serde_json::to_value(&err).unwrap();
        assert_eq!(json["code"], "E_TOOL_UNKNOWN");
        assert_eq!(json["nearby"][0], "exec");
        assert_eq!(json["recovery"], "use an L0 tool");
        assert_eq!(json.get("gate"), None);
    }

    #[test]
    fn axerror_serde_field_order_is_declaration_order() {
        let err = AxError::failure(AxCode::Timeout, "run exec", "build.sh");
        let json = serde_json::to_string(&err).unwrap();
        let code_at = json.find("\"code\"").unwrap();
        let action_at = json.find("\"action\"").unwrap();
        let subject_at = json.find("\"subject\"").unwrap();
        let retriable_at = json.find("\"retriable\"").unwrap();
        assert!(code_at < action_at && action_at < subject_at && subject_at < retriable_at);
    }

    #[test]
    fn display_names_code_action_subject() {
        let err = AxError::failure(AxCode::BudgetExhausted, "call model", "run-1");
        let text = err.to_string();
        assert!(text.contains("E_BUDGET_EXHAUSTED"));
        assert!(text.contains("call model"));
        assert!(text.contains("run-1"));
    }
}
