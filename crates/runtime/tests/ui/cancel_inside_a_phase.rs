// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

// A9's structural half: there is no way to interrupt inside a phase —
// interrupts are transition parameters, and no cancel method exists.

fn main() {
    let turn = runtime::turn::Turn::begin(
        kernel::RunId::CITY,
        "who".to_owned(),
        kernel::TimeMs::new(0),
    );
    turn.cancel();
}
