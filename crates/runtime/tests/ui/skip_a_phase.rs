// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

// Phase order is the type: an Assembling turn has no call method — the
// provider cannot be reached before prompt assembly is on the ledger.

fn main() {
    let turn = runtime::turn::Turn::begin(
        kernel::RunId::CITY,
        "who".to_owned(),
        kernel::TimeMs::new(0),
    );
    let _ = turn.call();
}
