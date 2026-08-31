// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

// C14's type half: a Discard without a restoration plan cannot be
// spelled — the fields are private and the sole constructor demands a
// Restoration.

fn main() {
    let _ = kernel::Discard {
        paths: vec![],
        taint: kernel::TaintSet::empty(),
        total_bytes: kernel::ByteLen::new(0),
    };
}
