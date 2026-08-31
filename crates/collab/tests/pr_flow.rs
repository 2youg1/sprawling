// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The P2 losing line, end to end: a node works in its own tree, someone
//! else verifies it, and only then does anything reach the building.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

use std::path::Path;

use collab::{Claim, NodeId, Pr};
use kernel::{B3Hash, Locator, TimeMs};
use memory::{Checkpoint, WorktreeName, Worktrees};

/// A city with one checkpoint, which is what a node's tree branches from.
fn city(root: &Path) -> Worktrees {
    std::fs::create_dir_all(root.join("lab")).unwrap();
    std::fs::write(root.join("lab").join("notes.md"), b"before\n").unwrap();
    Checkpoint::open(root)
        .unwrap()
        .wave_pre("lab", TimeMs::new(1_000), "owner")
        .unwrap();
    Worktrees::open(root).unwrap()
}

#[test]
fn a_node_reaches_the_building_only_through_someone_elses_verification() {
    let dir = tempfile::tempdir().unwrap();
    let trees = city(dir.path());
    let node = NodeId::parse("node-1").unwrap();
    let tree = trees
        .claim(&WorktreeName::parse("node-1").unwrap())
        .unwrap();

    // The implementer works in its own tree and commits there. The
    // building has not moved: nobody can see this yet.
    let produced = b"after: measured in metres\n";
    std::fs::write(tree.path().join("lab").join("notes.md"), produced).unwrap();
    Checkpoint::open(tree.path())
        .unwrap()
        .wave_pre("lab", TimeMs::new(2_000), "lab/room1")
        .unwrap();
    assert_eq!(
        std::fs::read_to_string(dir.path().join("lab").join("notes.md")).unwrap(),
        "before\n",
        "work in a node's tree is invisible to the building until it is merged"
    );

    let request = Pr::open(
        node.clone(),
        "lab/room1".to_owned(),
        tree.name().as_str().to_owned(),
    )
    .unwrap();

    // A test resident runs the done check and produces the artifact.
    let digest = B3Hash::digest(produced);
    let claim = Claim::new(
        node,
        Locator::parse(&format!("cas:b3-{digest}")).unwrap(),
        digest,
        "lab/room1".to_owned(),
    );
    let artifact = claim.verified(true, "lab/tests").unwrap();
    let verified = request.verified(&artifact).unwrap();
    assert_eq!(verified.verified_by(), "lab/tests");

    // Only now does anything move, and the record says who checked it.
    let planned = trees
        .plan_merge(&WorktreeName::parse(verified.branch()).unwrap())
        .unwrap();
    let commit = planned.commit();
    planned.apply().unwrap();
    let merged = verified.merged(commit);
    assert_eq!(
        std::fs::read_to_string(dir.path().join("lab").join("notes.md")).unwrap(),
        "after: measured in metres\n",
        "the building now stands on the node's work"
    );
    let record = merged.merged_payload().unwrap();
    assert_eq!(
        record.as_map().get("verified_by").and_then(|v| v.as_str()),
        Some("lab/tests")
    );
    assert_eq!(
        record.as_map().get("commit").and_then(|v| v.as_str()),
        Some(merged.commit())
    );
}
