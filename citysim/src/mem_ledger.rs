// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! The in-memory Ledger: second adapter of the kernel port. Bytes come from the same canonical producer as the durable
//! adapter, so simulation histories and real histories are comparable
//! byte for byte.

use kernel::conformance::LedgerInspect;
use kernel::{
    AxError, B3Hash, EventDraft, EventRecord, EventRef, GENESIS_PREV, Ledger, Seq, chain_hash,
};

pub struct MemLedger {
    lines: Vec<Vec<u8>>,
    next_seq: Seq,
    prev: B3Hash,
}

impl MemLedger {
    pub fn new() -> Self {
        MemLedger {
            lines: Vec::new(),
            next_seq: Seq::FIRST,
            prev: GENESIS_PREV,
        }
    }

    /// The inherent read face (citysim-SPEC 8): the executor's report and
    /// byte comparisons read here; the `LedgerInspect` impl below stays
    /// the conformance suite's door.
    pub fn raw_lines(&self) -> &[Vec<u8>] {
        &self.lines
    }
}

impl Default for MemLedger {
    fn default() -> Self {
        MemLedger::new()
    }
}

impl Ledger for MemLedger {
    fn append(&mut self, draft: EventDraft) -> Result<EventRef, AxError> {
        let record = EventRecord::from_draft(draft, self.next_seq, self.prev);
        let line = record.canonical_line()?;
        self.prev = chain_hash(&line);
        self.next_seq = self.next_seq.next()?;
        self.lines.push(line);
        Ok(record.to_ref())
    }
}

impl LedgerInspect for MemLedger {
    fn raw_lines(&self) -> Result<Vec<Vec<u8>>, AxError> {
        Ok(self.lines.clone())
    }
}
