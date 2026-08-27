// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

// A Handoff without a must-read list cannot be spelled: fields are
// private and the sole constructor refuses an empty list — the crossing
// point always names what the successor must read first.

fn main() {
    let _ = runtime::handoff::Handoff {
        overview: String::new(),
        progress: String::new(),
        context: String::new(),
        next_step: String::new(),
    };
}
