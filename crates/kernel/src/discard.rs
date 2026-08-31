// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Discard: deletion as an effect class, judged by effect and never by
//! intent. C14 in the type: a Discard without a
//! restoration plan cannot be constructed; the unplanned path exists only
//! as a request shape for exec forecasts — and the door denies it.
//! This module is the fifth door's sole authority; gate::discard
//! delegates wholly and only shapes the refusal.

use serde::{Deserialize, Serialize};

use crate::address::Address;
use crate::budget::ByteLen;
use crate::consts_policy::{DISCARD_BYTES_MAX, DISCARD_FILES_MAX};
use crate::error::{AxCode, AxError};
use crate::locator::Locator;
use crate::registry::Registry;
use crate::taint::TaintSet;
use crate::tool::ExecArm;

/// The three restoration routes, closed grammar reuse (frozen surface):
/// Tracked rides git (`file:`), Interred rides CAS (`cas:`), Rebuildable
/// names its reason. No fourth storage authority exists.
#[non_exhaustive]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Restoration {
    Tracked(Locator),
    Interred(Locator),
    Rebuildable { reason: String },
}

/// A planned discard. Private fields; the sole constructor checks the
/// plan's scheme — C14's type half.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Discard {
    paths: Vec<Address>,
    plan: Restoration,
    taint: TaintSet,
    total_bytes: ByteLen,
}

impl Discard {
    /// Restoration is mandatory and scheme-checked: Tracked wants
    /// `file:`, Interred wants `cas:`, Rebuildable wants a non-empty
    /// reason — violations are `E_DISCARD_IRREVERSIBLE` (an unparsable
    /// plan is no plan). Empty paths are `E_INVALID_ARGS`.
    pub fn new(
        paths: Vec<Address>,
        plan: Restoration,
        taint: TaintSet,
        total_bytes: ByteLen,
    ) -> Result<Discard, AxError> {
        if paths.is_empty() {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "construct discard",
                "empty path list",
            ));
        }
        let plan_ok = match &plan {
            Restoration::Tracked(locator) => matches!(locator, Locator::File { .. }),
            Restoration::Interred(locator) => matches!(locator, Locator::Cas { .. }),
            Restoration::Rebuildable { reason } => !reason.is_empty(),
        };
        if !plan_ok {
            return Err(AxError::failure(
                AxCode::DiscardIrreversible,
                "construct discard",
                "restoration plan does not resolve",
            )
            .with_recovery(
                "Tracked cites a file: locator, Interred a cas: locator, \
                 Rebuildable a non-empty reason",
            ));
        }
        Ok(Discard {
            paths,
            plan,
            taint,
            total_bytes,
        })
    }

    pub fn paths(&self) -> &[Address] {
        &self.paths
    }

    pub fn plan(&self) -> &Restoration {
        &self.plan
    }

    pub fn taint(&self) -> &TaintSet {
        &self.taint
    }

    pub fn total_bytes(&self) -> ByteLen {
        self.total_bytes
    }
}

/// What reaches the door: a planned discard, or an unplanned request
/// from the exec forecast path (text prediction cannot mint plans).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscardRequest {
    Planned(Discard),
    Unplanned {
        paths: Vec<Address>,
        taint: TaintSet,
        total_bytes: ByteLen,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EscalateReason {
    /// Tainted discards never auto-pass, whatever the size (C15).
    Tainted,
    FilesOverMax,
    BytesOverMax,
    RegistryAsset,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyReason {
    NoRestoration,
}

/// Deliberately exhaustive verdict.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscardVerdict {
    Allow,
    Escalate { reason: EscalateReason },
    Deny { reason: DenyReason },
}

/// The decision table (7.2), in fixed order: no plan denies; taint
/// escalates always; then scale (files, bytes); then registry assets;
/// the rest passes — a restorable, small, clean-handed delete is not
/// worth an interruption.
pub fn decide(req: &DiscardRequest, registry: &Registry) -> DiscardVerdict {
    let (paths, taint, total_bytes) = match req {
        DiscardRequest::Unplanned { .. } => {
            return DiscardVerdict::Deny {
                reason: DenyReason::NoRestoration,
            };
        }
        DiscardRequest::Planned(discard) => {
            (discard.paths(), discard.taint(), discard.total_bytes())
        }
    };
    if !taint.is_empty() {
        return DiscardVerdict::Escalate {
            reason: EscalateReason::Tainted,
        };
    }
    let over_files = u32::try_from(paths.len()).map_or(true, |count| count > DISCARD_FILES_MAX);
    if over_files {
        return DiscardVerdict::Escalate {
            reason: EscalateReason::FilesOverMax,
        };
    }
    if total_bytes.get() > DISCARD_BYTES_MAX {
        return DiscardVerdict::Escalate {
            reason: EscalateReason::BytesOverMax,
        };
    }
    if paths.iter().any(|p| registry.is_asset_at(p)) {
        return DiscardVerdict::Escalate {
            reason: EscalateReason::RegistryAsset,
        };
    }
    DiscardVerdict::Allow
}

/// Suspicious-command snippets for the text arms (data face, pub(crate)).
/// Text prediction is obfuscatable by design — hits route conservatively,
/// and the git checkpoint net (S3) is the honest backstop.
pub(crate) const SUSPECT_SNIPPETS_TEXT: [&str; 5] =
    ["rm ", "rmdir", "-delete", "git reset --hard", "git clean"];

pub(crate) const SUSPECT_SNIPPETS_PYTHON: [&str; 3] = ["os.remove", "shutil.rmtree", "os.unlink"];

/// Deliberately exhaustive forecast.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DiscardForecast {
    Clear,
    Suspected { pattern: String },
}

fn program_basename(path: &str) -> &str {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    name.strip_suffix(".exe").unwrap_or(name)
}

/// The pre-judgment (7.2): Program reads `(path, args)` whole — a
/// whitelisted name with poisoned args is no trust at all; Python and
/// Shell get substring conservatism.
pub fn forecast(arm: &ExecArm) -> DiscardForecast {
    match arm {
        ExecArm::Program { path, args } => {
            let base = program_basename(path);
            if matches!(base, "rm" | "rmdir" | "del") {
                return DiscardForecast::Suspected {
                    pattern: base.to_owned(),
                };
            }
            let joined = args.join(" ");
            if base == "git" && (joined.contains("reset --hard") || joined.contains("clean")) {
                return DiscardForecast::Suspected {
                    pattern: format!("git {joined}"),
                };
            }
            if base == "find" && args.iter().any(|a| a == "-delete") {
                return DiscardForecast::Suspected {
                    pattern: "find -delete".to_owned(),
                };
            }
            DiscardForecast::Clear
        }
        ExecArm::Python { code } => {
            for pattern in SUSPECT_SNIPPETS_TEXT
                .iter()
                .chain(SUSPECT_SNIPPETS_PYTHON.iter())
            {
                if code.contains(pattern) {
                    return DiscardForecast::Suspected {
                        pattern: (*pattern).to_owned(),
                    };
                }
            }
            if code.contains("open(") && (code.contains("'w'") || code.contains("\"w\"")) {
                return DiscardForecast::Suspected {
                    pattern: "open(..., 'w')".to_owned(),
                };
            }
            DiscardForecast::Clear
        }
        ExecArm::Shell { text } => {
            for pattern in SUSPECT_SNIPPETS_TEXT.iter() {
                if text.contains(pattern) {
                    return DiscardForecast::Suspected {
                        pattern: (*pattern).to_owned(),
                    };
                }
            }
            if text.contains('>') && !text.contains(">>") {
                return DiscardForecast::Suspected {
                    pattern: "> truncation".to_owned(),
                };
            }
            DiscardForecast::Clear
        }
    }
}

#[cfg(kani)]
mod verification {
    //! V5: the fifth door fails closed — no plan never allows, taint
    //! never allows.

    use super::*;
    use crate::taint::TaintSource;

    #[kani::proof]
    fn unplanned_never_allows() {
        let registry = Registry::new();
        let req = DiscardRequest::Unplanned {
            paths: vec![Address::parse("b/x").unwrap()],
            taint: TaintSet::empty(),
            total_bytes: ByteLen::new(kani::any()),
        };
        assert!(matches!(
            decide(&req, &registry),
            DiscardVerdict::Deny { .. }
        ));
    }

    #[kani::proof]
    fn tainted_never_allows() {
        let registry = Registry::new();
        let source = TaintSource::new("web:x").unwrap();
        let discard = Discard::new(
            vec![Address::parse("b/x").unwrap()],
            Restoration::Rebuildable {
                reason: "cargo target".to_owned(),
            },
            TaintSet::of(source),
            ByteLen::new(kani::any()),
        )
        .unwrap();
        assert!(matches!(
            decide(&DiscardRequest::Planned(discard), &registry),
            DiscardVerdict::Escalate {
                reason: EscalateReason::Tainted
            }
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
    use super::*;
    use crate::taint::TaintSource;
    use proptest::prelude::*;

    fn addr(raw: &str) -> Address {
        Address::parse(raw).unwrap()
    }

    fn tracked() -> Restoration {
        Restoration::Tracked(Locator::parse(&format!("file:b/x.md@{}", "ab".repeat(20))).unwrap())
    }

    fn interred() -> Restoration {
        Restoration::Interred(Locator::parse(&format!("cas:b3-{}", "cd".repeat(32))).unwrap())
    }

    fn clean(paths: Vec<Address>, bytes: u64) -> Discard {
        Discard::new(paths, tracked(), TaintSet::empty(), ByteLen::new(bytes)).unwrap()
    }

    #[test]
    fn restoration_schemes_are_checked_at_construction() {
        assert!(
            Discard::new(
                vec![addr("b/x.md")],
                tracked(),
                TaintSet::empty(),
                ByteLen::new(1)
            )
            .is_ok()
        );
        assert!(
            Discard::new(
                vec![addr("b/x.md")],
                interred(),
                TaintSet::empty(),
                ByteLen::new(1)
            )
            .is_ok()
        );
        // Tracked with a cas: locator is an unresolvable plan.
        let wrong =
            Restoration::Tracked(Locator::parse(&format!("cas:b3-{}", "cd".repeat(32))).unwrap());
        let err = Discard::new(
            vec![addr("b/x.md")],
            wrong,
            TaintSet::empty(),
            ByteLen::new(1),
        )
        .unwrap_err();
        assert_eq!(err.code(), &AxCode::DiscardIrreversible);
        let empty_reason = Restoration::Rebuildable {
            reason: String::new(),
        };
        assert!(
            Discard::new(
                vec![addr("b/x.md")],
                empty_reason,
                TaintSet::empty(),
                ByteLen::new(1)
            )
            .is_err()
        );
        let no_paths = Discard::new(vec![], tracked(), TaintSet::empty(), ByteLen::new(1));
        assert_eq!(no_paths.unwrap_err().code(), &AxCode::InvalidArgs);
    }

    #[test]
    fn the_decision_table_holds_in_order() {
        let registry = Registry::new();
        // Allow: small, clean, planned.
        assert_eq!(
            decide(
                &DiscardRequest::Planned(clean(vec![addr("b/x.md")], 100)),
                &registry
            ),
            DiscardVerdict::Allow
        );
        // Tainted first, regardless of scale.
        let tainted = Discard::new(
            vec![addr("b/x.md")],
            tracked(),
            TaintSet::of(TaintSource::new("web:evil").unwrap()),
            ByteLen::new(1),
        )
        .unwrap();
        assert_eq!(
            decide(&DiscardRequest::Planned(tainted), &registry),
            DiscardVerdict::Escalate {
                reason: EscalateReason::Tainted
            }
        );
        // Files over max.
        let many: Vec<Address> = (0..17).map(|i| addr(&format!("b/f{i}.md"))).collect();
        assert_eq!(
            decide(&DiscardRequest::Planned(clean(many, 1)), &registry),
            DiscardVerdict::Escalate {
                reason: EscalateReason::FilesOverMax
            }
        );
        // Bytes over max.
        assert_eq!(
            decide(
                &DiscardRequest::Planned(clean(vec![addr("b/big.bin")], 1_048_577)),
                &registry
            ),
            DiscardVerdict::Escalate {
                reason: EscalateReason::BytesOverMax
            }
        );
        // Unplanned denies.
        assert_eq!(
            decide(
                &DiscardRequest::Unplanned {
                    paths: vec![addr("b/x.md")],
                    taint: TaintSet::empty(),
                    total_bytes: ByteLen::new(1)
                },
                &registry
            ),
            DiscardVerdict::Deny {
                reason: DenyReason::NoRestoration
            }
        );
    }

    #[test]
    fn registry_assets_escalate() {
        use crate::event::{EventDraft, EventKind, EventRecord, Payload, RunId, Seq, TimeMs};
        use crate::ledger::GENESIS_PREV;
        use crate::registry::{Artifact, Claim};
        let mut registry = Registry::new();
        let locator = Locator::parse(&format!("file:b/asset.md@{}", "ab".repeat(20))).unwrap();
        let evidence = EventRecord::from_draft(
            EventDraft {
                run: RunId::CITY,
                t: TimeMs::new(0),
                who: "city".into(),
                addr: None,
                kind: EventKind::ToolResult,
                data: Payload::empty(),
                ig: false,
            },
            Seq::FIRST,
            GENESIS_PREV,
        )
        .to_ref();
        let artifact = Artifact::verify(
            Claim {
                locator: locator.clone(),
                by: "worker".into(),
            },
            evidence,
        )
        .unwrap();
        registry.register_artifact(artifact);
        registry.promote_asset(&locator).unwrap();
        assert_eq!(
            decide(
                &DiscardRequest::Planned(clean(vec![addr("b/asset.md")], 10)),
                &registry
            ),
            DiscardVerdict::Escalate {
                reason: EscalateReason::RegistryAsset
            }
        );
    }

    #[test]
    fn program_forecast_reads_path_and_args_whole() {
        let rm = ExecArm::Program {
            path: "/usr/bin/rm".into(),
            args: vec!["-rf".into(), "docs".into()],
        };
        assert!(matches!(forecast(&rm), DiscardForecast::Suspected { .. }));
        let git_ok = ExecArm::Program {
            path: "git".into(),
            args: vec!["status".into()],
        };
        assert_eq!(forecast(&git_ok), DiscardForecast::Clear);
        let git_hard = ExecArm::Program {
            path: "git".into(),
            args: vec!["reset".into(), "--hard".into()],
        };
        assert!(matches!(
            forecast(&git_hard),
            DiscardForecast::Suspected { .. }
        ));
        let find_delete = ExecArm::Program {
            path: "find.exe".into(),
            args: vec![".".into(), "-delete".into()],
        };
        assert!(matches!(
            forecast(&find_delete),
            DiscardForecast::Suspected { .. }
        ));
    }

    #[test]
    fn text_arms_are_conservative() {
        let py = ExecArm::Python {
            code: "import shutil\nshutil.rmtree('build')".into(),
        };
        assert!(matches!(forecast(&py), DiscardForecast::Suspected { .. }));
        let py_write = ExecArm::Python {
            code: "open('x.txt', 'w').write('hi')".into(),
        };
        assert!(matches!(
            forecast(&py_write),
            DiscardForecast::Suspected { .. }
        ));
        let py_read = ExecArm::Python {
            code: "print(open('x.txt').read())".into(),
        };
        assert_eq!(forecast(&py_read), DiscardForecast::Clear);
        let sh_trunc = ExecArm::Shell {
            text: "echo hi > file.txt".into(),
        };
        assert!(matches!(
            forecast(&sh_trunc),
            DiscardForecast::Suspected { .. }
        ));
        let sh_append = ExecArm::Shell {
            text: "echo hi >> log.txt".into(),
        };
        assert_eq!(forecast(&sh_append), DiscardForecast::Clear);
    }

    proptest! {
        /// Kani mirror: Allow implies planned, clean-handed, in-scale.
        #[test]
        fn allow_implies_every_guard_passed(files in 1usize..24, bytes in any::<u64>(),
                                            tainted in any::<bool>()) {
            let registry = Registry::new();
            let paths: Vec<Address> = (0..files).map(|i| addr(&format!("b/f{i}"))).collect();
            let taint = if tainted {
                TaintSet::of(TaintSource::new("web:x").unwrap())
            } else {
                TaintSet::empty()
            };
            let discard = Discard::new(paths, tracked(), taint, ByteLen::new(bytes)).unwrap();
            let verdict = decide(&DiscardRequest::Planned(discard), &registry);
            if verdict == DiscardVerdict::Allow {
                prop_assert!(!tainted);
                prop_assert!(files <= 16);
                prop_assert!(bytes <= 1_048_576);
            }
        }
    }
}
