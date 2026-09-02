// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! A plan node's address: the dotted path that says where it hangs.
//!
//! **A value, and that is why it is not in `kernel::plan`.** The tree
//! decides — what may be started, what a branch is worth, which of the
//! two exits a held node takes. This only says what a well-formed
//! address is, and refuses everything else at the one construction
//! point, so nothing downstream has to ask again. `plan` is shape 1;
//! this is shape 2 (ARCHITECTURE.md section 9).
//!
//! The hand-written `Deserialize` is the load-bearing part. A derived
//! one would accept any string off the wire and hand back a `NodeId`
//! that never passed `parse`, which is the whole of what this type is
//! for.

use serde::{Deserialize, Serialize};

use crate::error::{AxCode, AxError};

/// The longest branch a plan may grow. Ten levels of `1.1.1.…` is a
/// depth no plan has ever needed and a length past which the index
/// column stops being readable; refusing it also bounds every walk in
/// this module.
pub const NODE_DEPTH_MAX: usize = 10;

/// A node's place in the plan, as the first column spells it: `2`,
/// `2.3`, `2.3.1`. The parent is the prefix, so the tree needs no second
/// field to say where a node hangs.
///
/// Ordering is the reading order of the table — `1`, `1.1`, `1.2`, `2` —
/// because segment-wise comparison is exactly that order.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct NodeId(String);

impl NodeId {
    /// Reads an index cell.
    ///
    /// # Errors
    /// Refuses an empty cell, a segment that is not a number, a zero
    /// segment (the table is one-based, and `0` next to `1` reads as a
    /// different node than it is), a leading zero (`01` and `1` would be
    /// two spellings of one node), and a branch deeper than
    /// [`NODE_DEPTH_MAX`].
    pub fn parse(raw: &str) -> Result<NodeId, AxError> {
        let text = raw.trim();
        let refuse = |why: String| {
            AxError::failure(AxCode::InvalidArgs, "read a plan index", why)
                .with_recovery("write the index as dotted numbers from one, such as `2.3.1`")
        };
        if text.is_empty() {
            return Err(refuse("the cell is empty".to_owned()));
        }
        let segments: Vec<&str> = text.split('.').collect();
        if segments.len() > NODE_DEPTH_MAX {
            return Err(refuse(format!(
                "{} levels deep, the plan holds {NODE_DEPTH_MAX}",
                segments.len()
            )));
        }
        for segment in &segments {
            if segment.is_empty() || !segment.bytes().all(|byte| byte.is_ascii_digit()) {
                return Err(refuse(format!("`{segment}` is not a number")));
            }
            if segment.starts_with('0') {
                return Err(refuse(format!("`{segment}` has a leading zero")));
            }
            if segment.parse::<u32>().is_err() {
                return Err(refuse(format!("`{segment}` does not fit a plan index")));
            }
        }
        Ok(NodeId(segments.join(".")))
    }

    /// The node this one hangs under, or `None` for a top-level node.
    #[must_use]
    pub fn parent(&self) -> Option<NodeId> {
        self.0
            .rsplit_once('.')
            .map(|(head, _)| NodeId(head.to_owned()))
    }

    /// The last segment: which child of its parent this is.
    #[must_use]
    pub fn ordinal(&self) -> u32 {
        self.0
            .rsplit_once('.')
            .map_or(self.0.as_str(), |(_, tail)| tail)
            .parse()
            .unwrap_or(0)
    }

    /// How many levels down this node sits; a top-level node is 1.
    #[must_use]
    pub fn depth(&self) -> usize {
        self.0.split('.').count()
    }

    /// Whether `other` hangs below this node, at any depth. A node is
    /// not its own ancestor.
    #[must_use]
    pub fn is_ancestor_of(&self, other: &NodeId) -> bool {
        other.0.len() > self.0.len()
            && other.0.starts_with(&self.0)
            && other.0.as_bytes().get(self.0.len()) == Some(&b'.')
    }

    /// Every node between the root and this one, closest last.
    #[must_use]
    pub fn ancestors(&self) -> Vec<NodeId> {
        let mut found = Vec::new();
        let mut walking = self.parent();
        while let Some(held) = walking {
            walking = held.parent();
            found.push(held);
        }
        found.reverse();
        found
    }

    /// This node's `n`-th child.
    ///
    /// # Errors
    /// Refuses an ordinal of zero and a child past [`NODE_DEPTH_MAX`].
    pub fn child(&self, ordinal: u32) -> Result<NodeId, AxError> {
        NodeId::parse(&format!("{}.{ordinal}", self.0))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for NodeId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for NodeId {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        NodeId::parse(&raw).map_err(serde::de::Error::custom)
    }
}
