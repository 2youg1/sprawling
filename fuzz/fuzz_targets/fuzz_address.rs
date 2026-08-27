// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

#![no_main]

use libfuzzer_sys::fuzz_target;

// Property under fuzz: Address::parse never panics, and whatever it accepts
// round-trips byte-exactly and never smuggles a banned component through.
fuzz_target!(|data: &[u8]| {
    if let Ok(raw) = std::str::from_utf8(data) {
        if let Ok(addr) = kernel::Address::parse(raw) {
            assert_eq!(addr.as_str(), raw);
            assert!(!raw.is_empty());
            assert!(!raw.starts_with('/'));
            assert!(!raw.contains('\\'));
            assert!(!raw.contains(':'));
            assert!(
                !raw.split('/')
                    .any(|seg| seg.is_empty() || seg == "." || seg == "..")
            );
        }
    }
});
