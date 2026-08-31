// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

// An artifact is what verification produces; it has no constructor of
// its own, so a producer cannot hand one to the join by building it.

fn main() {
    let _ = collab::Artifact {
        node: collab::NodeId::parse("node-1").unwrap(),
        verified_by: "lab/room1".to_owned(),
    };
}
