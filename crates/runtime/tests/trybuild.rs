// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! V0: "unrepresentable" is itself tested. Every file under tests/ui must
//! fail to compile; the stderr snapshots pin the reason.

#[test]
fn phase_and_prefix_discipline_is_compile_checked() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/*.rs");
}
