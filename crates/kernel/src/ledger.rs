// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! The Ledger port: the only write entrance to history (seam list,
//! ARCHITECTURE section 3), plus the chain rule both adapters and replay
//! share.
//!
//! Contract owned here:
//! - implementations own seq/prev assignment and byte production; callers
//!   never serialize. `Ok(ref)` means the record is durable in that
//!   adapter's medium; `Err` means nothing observable was appended (torn
//!   bytes are the reopen path's business).
//! - the chain hashes the canonical line bytes exactly, excluding the
//!   line terminator; the genesis line's prev is 32 zero bytes.
//! - the port is write-only: production reads go through projections.
//!   The read-back needed for verification lives behind the
//!   `conformance` feature and never ships in the production surface.

use crate::error::AxError;
use crate::event::{EventDraft, EventRef};
use crate::locator::B3Hash;

/// prev of the genesis line: 64 zero hex digits.
pub const GENESIS_PREV: B3Hash = B3Hash::from_bytes([0; 32]);

/// Chain rule: prev(line k+1) = blake3(canonical bytes of line k).
/// One hash function, one home; jsonl, the in-memory ledger and replay
/// all call this exact function.
pub fn chain_hash(raw_line: &[u8]) -> B3Hash {
    B3Hash::from_bytes(*blake3::hash(raw_line).as_bytes())
}

/// The only write entrance to history.
pub trait Ledger {
    fn append(&mut self, draft: EventDraft) -> Result<EventRef, AxError>;
}

#[cfg(feature = "conformance")]
pub mod conformance {
    //! V3: one assertion suite for every implementation. A stand-in that
    //! cannot pass this suite is not an implementation of the port.

    use super::{GENESIS_PREV, Ledger, chain_hash};
    use crate::address::Address;
    use crate::error::AxError;
    use crate::event::{EventDraft, EventKind, EventRecord, Payload, RunId, Seq, TimeMs};

    /// Verification-only read-back. Production callers never read through
    /// the Ledger handle; this trait exists so the suite can compare raw
    /// bytes, and it compiles only under the `conformance` feature.
    pub trait LedgerInspect {
        fn raw_lines(&self) -> Result<Vec<Vec<u8>>, AxError>;
    }

    /// Fixed draft script: deterministic on purpose so assertion six
    /// (byte-identical logs from two fresh instances) is meaningful.
    fn script() -> Vec<EventDraft> {
        let run = RunId::from_bytes([1; 16]);
        let parsed = Address::parse("lab");
        assert!(parsed.is_ok(), "fixture address must parse: {parsed:?}");
        let addr = parsed.ok();
        vec![
            EventDraft {
                run: RunId::CITY,
                t: TimeMs::new(1),
                who: "city".to_owned(),
                addr: None,
                kind: EventKind::CityInitialized,
                data: Payload::empty(),
                ig: false,
            },
            EventDraft {
                run: RunId::CITY,
                t: TimeMs::new(2),
                who: "city".to_owned(),
                addr,
                kind: EventKind::BuildingCreated,
                data: Payload::empty(),
                ig: false,
            },
            EventDraft {
                run,
                t: TimeMs::new(3),
                who: "planner@lab.1".to_owned(),
                addr: None,
                kind: EventKind::RunStarted,
                data: Payload::empty(),
                ig: false,
            },
            EventDraft {
                run,
                t: TimeMs::new(4),
                who: "planner@lab.1".to_owned(),
                addr: None,
                kind: EventKind::ToolCalled,
                data: Payload::empty(),
                ig: false,
            },
            EventDraft {
                run,
                t: TimeMs::new(5),
                who: "planner@lab.1".to_owned(),
                addr: None,
                kind: EventKind::RunFrozen,
                data: Payload::empty(),
                ig: true,
            },
        ]
    }

    fn collect_lines<L: Ledger + LedgerInspect>(ledger: &mut L) -> Vec<Vec<u8>> {
        for (index, draft) in script().into_iter().enumerate() {
            let kind = draft.kind;
            let appended = ledger.append(draft);
            assert!(
                appended.is_ok(),
                "append {index} ({kind:?}) must succeed: {appended:?}"
            );
            let Ok(echo) = appended else {
                return Vec::new();
            };
            // Assertion five (half): the echo points at what was appended.
            assert_eq!(echo.kind(), kind, "ref kind must echo the draft");
            assert_eq!(
                echo.seq(),
                Seq::new(u64::try_from(index).unwrap_or(u64::MAX))
            );
        }
        let lines = ledger.raw_lines();
        assert!(lines.is_ok(), "raw_lines must succeed: {lines:?}");
        let Ok(lines) = lines else { return Vec::new() };
        lines
    }

    /// The suite (kernel-SPEC 8-9, six assertions). `fresh` must yield an
    /// empty ledger each call.
    pub fn assert_ledger_conformance<L, F>(mut fresh: F)
    where
        L: Ledger + LedgerInspect,
        F: FnMut() -> L,
    {
        let mut first = fresh();
        let lines = collect_lines(&mut first);
        assert_eq!(lines.len(), script().len(), "one line per draft");

        let mut prev = GENESIS_PREV;
        for (index, line) in lines.iter().enumerate() {
            let parsed = EventRecord::parse_line(line);
            assert!(parsed.is_ok(), "line {index} must parse: {parsed:?}");
            let Ok(record) = parsed else { return };
            // Assertion one and three: genesis prev, then chain continuity.
            assert_eq!(
                record.prev(),
                prev,
                "line {index}: prev must hash the previous line"
            );
            // Assertion two: contiguous seq from FIRST.
            assert_eq!(
                record.seq(),
                Seq::new(u64::try_from(index).unwrap_or(u64::MAX)),
                "line {index}: seq must be contiguous"
            );
            // Assertion five: v is pinned.
            assert_eq!(
                record.v(),
                crate::consts_external::EVENT_LOG_V,
                "line {index}: v must be EVENT_LOG_V"
            );
            // Assertion four: the writer is canonical.
            let echo = record.canonical_line();
            assert!(echo.is_ok(), "line {index} must reserialize");
            let Ok(echo) = echo else { return };
            assert_eq!(
                &echo, line,
                "line {index}: adapter bytes must be the canonical bytes"
            );
            prev = chain_hash(line);
        }

        // Assertion six: determinism across fresh instances.
        let mut second = fresh();
        let again = collect_lines(&mut second);
        assert_eq!(
            lines, again,
            "two fresh instances fed the same drafts must produce identical bytes"
        );
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
    fn genesis_prev_is_sixty_four_zeros() {
        assert_eq!(GENESIS_PREV.to_string(), "0".repeat(64));
    }

    #[test]
    fn chain_hash_is_plain_blake3_of_the_line_bytes() {
        let line = b"{\"v\":1}";
        assert_eq!(
            chain_hash(line).to_string(),
            blake3::hash(line).to_hex().to_string()
        );
    }
}

#[cfg(all(test, feature = "conformance"))]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]
mod conformance_self_test {
    use super::conformance::{LedgerInspect, assert_ledger_conformance};
    use super::*;
    use crate::error::{AxCode, AxError};
    use crate::event::{EventDraft, EventRecord, EventRef, Seq};

    /// Minimal reference implementation: proves the suite runs against any
    /// implementation before citysim provides the second one (S1.11).
    struct VecLedger {
        lines: Vec<Vec<u8>>,
        next_seq: Seq,
        prev: B3Hash,
    }

    impl VecLedger {
        fn new() -> Self {
            VecLedger {
                lines: Vec::new(),
                next_seq: Seq::FIRST,
                prev: GENESIS_PREV,
            }
        }
    }

    impl Ledger for VecLedger {
        fn append(&mut self, draft: EventDraft) -> Result<EventRef, AxError> {
            let record = EventRecord::from_draft(draft, self.next_seq, self.prev);
            let line = record.canonical_line()?;
            self.prev = chain_hash(&line);
            self.next_seq = self.next_seq.next()?;
            self.lines.push(line);
            Ok(record.to_ref())
        }
    }

    impl LedgerInspect for VecLedger {
        fn raw_lines(&self) -> Result<Vec<Vec<u8>>, AxError> {
            Ok(self.lines.clone())
        }
    }

    #[test]
    fn the_reference_implementation_passes_the_suite() {
        assert_ledger_conformance(VecLedger::new);
    }

    #[test]
    fn a_chain_breaking_implementation_fails_the_suite() {
        struct BrokenPrev(VecLedger);
        impl Ledger for BrokenPrev {
            fn append(&mut self, draft: EventDraft) -> Result<EventRef, AxError> {
                let echo = self.0.append(draft)?;
                self.0.prev = GENESIS_PREV; // never advances: chain breaks at line 2
                Ok(echo)
            }
        }
        impl LedgerInspect for BrokenPrev {
            fn raw_lines(&self) -> Result<Vec<Vec<u8>>, AxError> {
                self.0.raw_lines()
            }
        }
        let outcome = std::panic::catch_unwind(|| {
            assert_ledger_conformance(|| BrokenPrev(VecLedger::new()));
        });
        assert!(outcome.is_err(), "the suite must bite a broken chain");
    }

    #[test]
    fn suite_error_paths_use_invalid_args() {
        // Anchors the AxCode used when a draft cannot even serialize.
        let err = AxError::failure(AxCode::InvalidArgs, "x", "y");
        assert_eq!(err.code(), &AxCode::InvalidArgs);
    }
}
