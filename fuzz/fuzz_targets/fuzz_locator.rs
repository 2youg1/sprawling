// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

#![no_main]

use libfuzzer_sys::fuzz_target;

// Property under fuzz: Locator::parse never panics; accepted inputs are
// canonical (Display echoes the input byte-exactly) and `secret:` never
// parses (separate parser by design).
fuzz_target!(|data: &[u8]| {
    if let Ok(raw) = std::str::from_utf8(data) {
        match kernel::Locator::parse(raw) {
            Ok(loc) => {
                assert_eq!(loc.to_string(), raw);
                assert!(!raw.starts_with("secret:"));
            }
            Err(err) => {
                assert_eq!(err.code(), &kernel::AxCode::LocatorInvalid);
            }
        }
    }
});
