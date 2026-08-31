// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

// The losing line of P2 as a type: a pull request nobody verified has no
// method that merges it. Verification is not a step a caller can skip,
// because the phase that can be merged is the one verification produces.

fn main() {
    let request = collab::Pr::open(
        collab::NodeId::parse("node-1").unwrap(),
        "lab/room1".to_owned(),
        "node-1".to_owned(),
    )
    .unwrap();
    let _ = request.merged("abc123".to_owned());
}
