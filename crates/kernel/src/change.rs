// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! What moved between two checkpoints, as a fact rather than as a patch.
//!
//! Here rather than beside the git call that produces it, for the reason
//! `Restoration` is here: three places need this shape — `memory` reads
//! it off two trees, `channels` carries it, and the interface draws it —
//! and `channels` cannot see `memory`. Defining it twice and converting
//! between the copies would be two definitions of what a file change is,
//! and the one that drifts is always the one nobody is looking at.
//!
//! Paths and counts only. Patch text is file content, and file content
//! leaving this machine is the question `secret::scan` exists to answer;
//! a hunk therefore has to be asked for on its own and scanned like
//! anything else, which is why no field here can hold one.

use serde::{Deserialize, Serialize};

/// How much of a file moved.
///
/// An enum rather than two numbers, because a binary file has no line
/// count. Rendering one as `+0 −0` would be the interface reporting a
/// measurement nobody made, and a reader would take it for a file that
/// was touched and left alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Lines {
    Counted { added: u32, removed: u32 },
    Binary,
}

/// What happened to the file.
///
/// `Renamed` is its own arm because a rename and a delete-plus-add are
/// different facts about the same two trees, and somebody deciding
/// whether an agent moved code or rewrote it needs the difference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum How {
    Added,
    Modified,
    Deleted,
    Renamed { from: String },
}

/// One file, as the difference between two trees describes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    pub path: String,
    pub how: How,
    pub lines: Lines,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, reason = "test code")]
mod tests {
    use super::*;

    /// A binary file and an untouched one must not spell the same row.
    #[test]
    fn a_binary_file_is_not_a_file_that_moved_nothing() {
        let binary = serde_json::to_string(&Lines::Binary).unwrap();
        let nothing = serde_json::to_string(&Lines::Counted {
            added: 0,
            removed: 0,
        })
        .unwrap();
        assert_ne!(binary, nothing);
    }

    #[test]
    fn a_rename_carries_where_the_file_came_from() {
        let moved = FileChange {
            path: "lab/lex.rs".to_owned(),
            how: How::Renamed {
                from: "lab/lexer.rs".to_owned(),
            },
            lines: Lines::Counted {
                added: 0,
                removed: 0,
            },
        };
        let bytes = serde_json::to_vec(&moved).unwrap();
        let back: FileChange = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(back, moved);
    }
}
