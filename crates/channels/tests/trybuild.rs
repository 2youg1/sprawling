// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! V0 for the process boundary: the two frames a remote peer
//! must not be able to spell are proven unspellable by the
//! compiler, not by a runtime check.

#[test]
fn the_wire_counterexamples_do_not_compile() {
    let cases = trybuild::TestCases::new();
    cases.compile_fail("tests/ui/*.rs");
}
