// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

#![no_main]

use libfuzzer_sys::fuzz_target;

// Property under fuzz: opening a ledger directory whose only segment is
// arbitrary bytes never panics; when it opens, tail recovery has done its
// job - a second open is clean (no further truncation), and the surviving
// lines verify as a chain.
fuzz_target!(|data: &[u8]| {
    let dir = tempfile::tempdir().expect("temp dir");
    let seg = dir.path().join("ledger-00000000000000000000.jsonl");
    std::fs::write(&seg, data).expect("write fuzz segment");

    match memory::JsonlLedger::open(dir.path(), kernel::TimeMs::new(0)) {
        Ok((ledger, _)) => {
            let lines = ledger.read_raw_lines().expect("read back");
            drop(ledger);
            let (reopened, report) = memory::JsonlLedger::open(dir.path(), kernel::TimeMs::new(1))
                .expect("second open after recovery");
            assert!(
                report.recovered.is_none(),
                "tail recovery must be idempotent"
            );
            assert_eq!(reopened.read_raw_lines().expect("read back"), lines);
        }
        Err(_) => {} // refusal is a legal outcome; panicking is not
    }
});
