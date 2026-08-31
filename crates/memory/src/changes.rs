// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! What changed between two checkpoints, as paths and counts.
//!
//! The write side of this city has been git-native since S3.07 — every
//! tool wave is fenced by a real commit — and nothing anywhere could read
//! two of those commits back and say what moved between them. This is
//! that read.
//!
//! **The scope is already right, and it is right for free.** A checkpoint
//! stages `<scope>/*` and nothing wider, so the difference between two of
//! them cannot contain a file the Run merely read. Other harnesses report
//! a session diff that includes everything the session opened, and their
//! users cannot tell what the agent actually wrote; the fence being the
//! write domain is what spares this one that.
//!
//! **Counts, never patch text.** `checkpoint::scan_staged` refuses to
//! echo the bytes that matched a secret shape, because printing them to
//! prove a leak is the leak. Patch text is file content on a socket, and
//! that is the same question — so a hunk has to be its own request,
//! answered through the same scan, and it is not this module.

use std::cell::{Cell, RefCell};
use std::path::Path;

use kernel::{FileChange, GitOid, How, Lines};

use crate::jsonl::MemoryError;

/// The far end of the comparison.
///
/// Exhaustive rather than `Option<GitOid>`: "against the working tree" is
/// a statement about now, and spelling it as an absent commit invites the
/// caller to read it as "against nothing".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Head {
    Commit(GitOid),
    WorkingTree,
}

fn git_err(op: &'static str) -> impl FnOnce(git2::Error) -> MemoryError {
    move |err| MemoryError::Checkpoint {
        op,
        detail: err.message().to_owned(),
    }
}

/// What moved between `base` and `head`, one row per file, path order.
///
/// Path order rather than size order: a reader looking for one file finds
/// it in the same place every time, and a list that reorders itself as
/// the numbers change is a list nobody can scan twice.
///
/// # Errors
/// Propagates whatever opening the repository, finding the commits, or
/// walking the difference reports.
pub fn between(city_root: &Path, base: GitOid, head: Head) -> Result<Vec<FileChange>, MemoryError> {
    let repo = git2::Repository::open(city_root).map_err(git_err("open the city repository"))?;
    let find = |oid: GitOid| -> Result<git2::Tree<'_>, MemoryError> {
        let parsed =
            git2::Oid::from_str(&oid.to_string()).map_err(git_err("parse a checkpoint"))?;
        repo.find_commit(parsed)
            .map_err(git_err("find a checkpoint"))?
            .tree()
            .map_err(git_err("read a checkpoint tree"))
    };
    let old = find(base)?;
    let mut options = git2::DiffOptions::new();
    // Renames are detected below rather than assumed here; this only asks
    // git not to expand a whole file into a hunk it will never show.
    options.context_lines(0);
    let mut diff = match head {
        Head::Commit(oid) => {
            let new = find(oid)?;
            repo.diff_tree_to_tree(Some(&old), Some(&new), Some(&mut options))
        }
        // Index included, so a file the wave staged but has not committed
        // still counts as moved: what a person is looking at is the tree
        // on disk, not the tree git last recorded.
        Head::WorkingTree => repo.diff_tree_to_workdir_with_index(Some(&old), Some(&mut options)),
    }
    .map_err(git_err("compare two checkpoints"))?;
    diff.find_similar(None)
        .map_err(git_err("look for renames"))?;
    collect(&diff)
}

/// Reads one prepared diff into rows.
///
/// Separate from [`between`] because everything above is git and
/// everything here is this module's own reading of it: which deltas earn
/// a row, and what a row says when git has no line count for it.
fn collect(diff: &git2::Diff<'_>) -> Result<Vec<FileChange>, MemoryError> {
    let mut rows: Vec<FileChange> = Vec::new();
    let stats: Vec<(u32, u32)> = line_counts(diff)?;
    for (at, delta) in diff.deltas().enumerate() {
        let Some(path) = delta
            .new_file()
            .path()
            .or_else(|| delta.old_file().path())
            .and_then(Path::to_str)
        else {
            // A path this platform cannot spell is left out rather than
            // shown under a mangled name; the count of what is listed is
            // always the count of what a reader can act on.
            continue;
        };
        let how = match delta.status() {
            git2::Delta::Added | git2::Delta::Copied => How::Added,
            git2::Delta::Deleted => How::Deleted,
            git2::Delta::Renamed => How::Renamed {
                from: delta
                    .old_file()
                    .path()
                    .and_then(Path::to_str)
                    .unwrap_or_default()
                    .to_owned(),
            },
            _ => How::Modified,
        };
        // `is_binary` is only set once git has looked at the content,
        // which `find_similar` above has made it do.
        let binary = delta.new_file().is_binary() || delta.old_file().is_binary();
        let lines = match (binary, stats.get(at)) {
            (false, Some(&(added, removed))) => Lines::Counted { added, removed },
            _ => Lines::Binary,
        };
        rows.push(FileChange {
            path: path.to_owned(),
            how,
            lines,
        });
    }
    rows.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(rows)
}

/// Added and removed lines per delta, in delta order.
///
/// Walked once with a callback rather than asked per file: `git2` counts
/// lines while it walks, so one pass answers for every row and a second
/// pass per file would diff the same trees again.
fn line_counts(diff: &git2::Diff<'_>) -> Result<Vec<(u32, u32)>, MemoryError> {
    // Shared by two callbacks that `git2` calls one after the other and
    // never at the same time, which is what makes a `Cell` the right
    // amount of machinery here.
    let counts = RefCell::new(vec![(0u32, 0u32); diff.deltas().len()]);
    let at = Cell::new(0usize);
    let started = Cell::new(false);
    diff.foreach(
        &mut |_, _| {
            // The file callback opens each delta, so the first one is row
            // zero and every later one steps forward.
            if started.replace(true) {
                at.set(at.get().saturating_add(1));
            }
            true
        },
        None,
        None,
        Some(&mut |_, _, line| {
            if let Ok(mut rows) = counts.try_borrow_mut()
                && let Some(row) = rows.get_mut(at.get())
            {
                match line.origin() {
                    '+' => row.0 = row.0.saturating_add(1),
                    '-' => row.1 = row.1.saturating_add(1),
                    _ => {}
                }
            }
            true
        }),
    )
    .map_err(git_err("count the lines that moved"))?;
    Ok(counts.into_inner())
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]
mod tests {
    use super::*;
    use kernel::TimeMs;

    use crate::checkpoint::Checkpoint;

    fn write(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }

    fn oid_of(payload: &kernel::Payload) -> GitOid {
        let raw = serde_json::to_value(payload).unwrap()["oid"]
            .as_str()
            .unwrap()
            .to_owned();
        GitOid::parse(&raw).expect("a checkpoint names a git object")
    }

    /// Two fences, and what a person wants to know about the wave between
    /// them: which files, and how much of each.
    #[test]
    fn two_checkpoints_report_the_files_between_them_and_how_much_moved() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "lab/lex.rs", "one\ntwo\nthree\n");
        write(root, "lab/keep.rs", "unchanged\n");
        let mut fence = Checkpoint::open(root).unwrap();
        let base = oid_of(
            &fence
                .wave_pre("lab", TimeMs::new(1_000), "lab/parser")
                .unwrap(),
        );

        write(root, "lab/lex.rs", "one\ntwo\nthree\nfour\nfive\n");
        write(root, "lab/new.rs", "fresh\n");
        std::fs::remove_file(root.join("lab/keep.rs")).unwrap();
        let head = oid_of(
            &fence
                .wave_pre("lab", TimeMs::new(2_000), "lab/parser")
                .unwrap(),
        );

        let rows = between(root, base, Head::Commit(head)).unwrap();
        let paths: Vec<&str> = rows.iter().map(|row| row.path.as_str()).collect();
        assert_eq!(paths, vec!["lab/keep.rs", "lab/lex.rs", "lab/new.rs"]);

        assert_eq!(rows[0].how, How::Deleted);
        assert_eq!(rows[1].how, How::Modified);
        assert_eq!(rows[2].how, How::Added);
        assert_eq!(
            rows[1].lines,
            Lines::Counted {
                added: 2,
                removed: 0
            },
            "two lines were appended to the lexer"
        );
    }

    /// A wave that changed nothing is not a wave that could not be read.
    #[test]
    fn two_identical_checkpoints_report_nothing_rather_than_failing() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "lab/lex.rs", "one\n");
        let mut fence = Checkpoint::open(root).unwrap();
        let base = oid_of(&fence.wave_pre("lab", TimeMs::new(1), "lab/parser").unwrap());
        let head = oid_of(&fence.wave_pre("lab", TimeMs::new(2), "lab/parser").unwrap());
        assert!(between(root, base, Head::Commit(head)).unwrap().is_empty());
    }

    /// What the person is looking at is the tree on disk. A wave still
    /// running has written files that no checkpoint holds yet, and a
    /// change list that ignored them would describe the session as it was
    /// one fence ago.
    #[test]
    fn work_not_yet_fenced_still_counts_against_the_working_tree() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "lab/lex.rs", "one\n");
        let mut fence = Checkpoint::open(root).unwrap();
        let base = oid_of(&fence.wave_pre("lab", TimeMs::new(1), "lab/parser").unwrap());

        write(root, "lab/lex.rs", "one\ntwo\n");
        let rows = between(root, base, Head::WorkingTree).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].path, "lab/lex.rs");
        assert_eq!(
            rows[0].lines,
            Lines::Counted {
                added: 1,
                removed: 0
            }
        );
    }

    /// A binary file has no line count, and saying `+0 −0` would be a
    /// measurement nobody made.
    #[test]
    fn a_binary_file_says_so_rather_than_reporting_nothing_moved() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        std::fs::create_dir_all(root.join("lab")).unwrap();
        std::fs::write(root.join("lab/blob.bin"), [0u8, 1, 2, 0, 3]).unwrap();
        let mut fence = Checkpoint::open(root).unwrap();
        let base = oid_of(&fence.wave_pre("lab", TimeMs::new(1), "lab/parser").unwrap());

        std::fs::write(root.join("lab/blob.bin"), [0u8, 9, 9, 0, 7, 7]).unwrap();
        let rows = between(root, base, Head::WorkingTree).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].lines, Lines::Binary, "{:?}", rows[0]);
    }

    /// A checkpoint that is not in this repository is a refusal, never an
    /// empty change list: "nothing moved" and "I could not look" are
    /// different answers and a reader acts differently on each.
    #[test]
    fn a_checkpoint_this_city_does_not_hold_is_refused_rather_than_read_as_empty() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path();
        write(root, "lab/lex.rs", "one\n");
        let mut fence = Checkpoint::open(root).unwrap();
        let base = oid_of(&fence.wave_pre("lab", TimeMs::new(1), "lab/parser").unwrap());
        let stranger = GitOid::from_bytes([7u8; 20]);
        assert!(between(root, stranger, Head::Commit(base)).is_err());
    }
}
