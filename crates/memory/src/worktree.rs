// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! One node, one working tree.
//!
//! Concurrency inside a building is not held apart by discipline but by
//! the filesystem: each node works in its own checkout, so two agents
//! cannot see each other's half-finished state, and what comes back
//! comes back through the PR flow.
//!
//! The trees are git worktrees of the repository `crate::checkpoint`
//! already keeps, so objects are shared and only the working files are
//! duplicated. A tree is refused before it is created, never halfway
//! through: the size of the tree to be copied is measured against
//! `WORKTREE_MAX_BYTES` first, and the refusal states both numbers.
//!
//! Copy-on-write cloning (reflink) is the cheaper path on filesystems
//! that offer it, and this module does not attempt it: there is no CoW
//! interface without unsafe FFI or a new dependency, so today every tree
//! is a full checkout under the ceiling. That is the fallback arm of the
//! design, stated as the current state rather than as the design.

use std::path::{Path, PathBuf};

use kernel::{ByteLen, Payload, consts_policy::WORKTREE_MAX_BYTES};
use serde_json::{Map, Value};

use crate::error::MemoryError;

/// Where the trees live: inside the reserved subtree, because they are
/// the city's own machinery rather than anybody's writable space. What a
/// run may write is judged against the tree it works in, not against
/// where the tree sits.
const WORKTREE_DIR: &str = "worktrees";

/// A node's name for its tree. A newtype because this string becomes a
/// directory name and a git reference: the two ways it can be malformed
/// are checked once, here.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
pub struct WorktreeName(String);

impl WorktreeName {
    /// # Errors
    /// Refuses an empty name, a path separator, a leading dot, and any
    /// character outside `[A-Za-z0-9._-]`. A name that walks out of its
    /// directory is the whole isolation guarantee walking out with it.
    pub fn parse(raw: &str) -> Result<WorktreeName, MemoryError> {
        let refuse = |detail: &str| MemoryError::Worktree {
            op: "name a worktree",
            detail: format!("{raw}: {detail}"),
        };
        if raw.is_empty() {
            return Err(refuse("a tree with no name cannot be released either"));
        }
        if raw.starts_with('.') {
            return Err(refuse("names do not start with a dot"));
        }
        if !raw
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
        {
            return Err(refuse("letters, digits, dot, underscore and dash only"));
        }
        Ok(WorktreeName(raw.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// A live tree: what it is called, where it is, and what it costs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorktreeLease {
    name: WorktreeName,
    path: PathBuf,
    disk: ByteLen,
}

impl WorktreeLease {
    #[must_use]
    pub fn name(&self) -> &WorktreeName {
        &self.name
    }

    /// The root a run in this node writes against.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// What the tree occupied when it was opened. `status` reports this,
    /// which is why it is measured rather than estimated.
    #[must_use]
    pub fn disk(&self) -> ByteLen {
        self.disk
    }

    /// The `worktree_opened` payload: a name and a size, no path. An
    /// absolute path is a fact about this machine, and a history that
    /// carries one does not survive being moved to another.
    ///
    /// # Errors
    /// Propagates the payload's refusal to hold what it was given.
    pub fn opened_payload(&self) -> Result<Payload, MemoryError> {
        let mut map = Map::new();
        map.insert(
            "name".to_owned(),
            Value::String(self.name.as_str().to_owned()),
        );
        map.insert(
            "disk_bytes".to_owned(),
            Value::Number(self.disk.get().into()),
        );
        Payload::new(map).map_err(|source| MemoryError::Draft { source })
    }
}

/// The city's trees.
pub struct Worktrees {
    repo: git2::Repository,
    home: PathBuf,
    ceiling: ByteLen,
}

// Hand-written because a git repository handle has no Debug: what a
// reader of a failure needs is where the trees go and what they may
// cost, not the handle.
impl std::fmt::Debug for Worktrees {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Worktrees")
            .field("home", &self.home)
            .field("ceiling", &self.ceiling)
            .finish()
    }
}

/// A merge that has been decided and not yet made.
///
/// The commit the trunk will land on is settled at construction, and
/// every refusal has already happened, so the line announcing this merge
/// can be written before the trunk moves. [`PlannedMerge::apply`] is the
/// only way to move it, and this value has no other source than
/// [`Worktrees::plan_merge`] - writing the two in the wrong order means
/// obtaining something that cannot be obtained.
pub struct PlannedMerge<'a> {
    trees: &'a Worktrees,
    target: git2::Oid,
}

/// Names the decision, not the repository holding it: a `Worktrees` has
/// no useful `Debug` and printing one would say nothing about which
/// merge this is.
impl std::fmt::Debug for PlannedMerge<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PlannedMerge")
            .field("commit", &self.target)
            .finish()
    }
}

impl PlannedMerge<'_> {
    /// The commit the city trunk will point at, for the line that says so.
    pub fn commit(&self) -> String {
        self.target.to_string()
    }

    /// Brings a node's committed work into the city's own trunk.
    ///
    /// # Errors
    /// Propagates a trunk that cannot be moved or checked out. The
    /// fast-forward judgement is not repeated: it was made, and refused
    /// if it had to be, before this value existed.
    pub fn apply(self) -> Result<(), MemoryError> {
        self.trees.fast_forward(self.target)
    }
}

impl Worktrees {
    /// Opens the city's repository as the source every tree branches
    /// from.
    ///
    /// # Errors
    /// Refuses a city with no repository. Initialising one here would
    /// make two modules able to create the city's history.
    pub fn open(city_root: &Path) -> Result<Worktrees, MemoryError> {
        let repo = git2::Repository::open(city_root).map_err(|err| MemoryError::Worktree {
            op: "open the city repository",
            detail: format!("{}: {err}", city_root.display()),
        })?;
        Ok(Worktrees {
            repo,
            home: city_root.join(kernel::RESERVED_PREFIX).join(WORKTREE_DIR),
            ceiling: ByteLen::new(WORKTREE_MAX_BYTES),
        })
    }

    /// Opens a tree for one node.
    ///
    /// # Errors
    /// Refuses a name already in use, a city whose working tree exceeds
    /// the ceiling, and a repository with no commit to branch from.
    pub fn claim(&self, name: &WorktreeName) -> Result<WorktreeLease, MemoryError> {
        if self.live()?.contains(name) {
            return Err(MemoryError::WorktreeBusy {
                name: name.as_str().to_owned(),
                detail: "another node holds this tree".to_owned(),
            });
        }
        let source = self.repo.workdir().ok_or_else(|| MemoryError::Worktree {
            op: "find the city working tree",
            detail: "the repository is bare".to_owned(),
        })?;
        let size = measure(source)?;
        if size.get() > self.ceiling.get() {
            return Err(MemoryError::WorktreeBusy {
                name: name.as_str().to_owned(),
                detail: format!(
                    "the city working tree is {} bytes and the ceiling is {}",
                    size.get(),
                    self.ceiling.get()
                ),
            });
        }
        if self.repo.head().is_err() {
            return Err(MemoryError::Worktree {
                op: "branch a worktree",
                detail: "the city has no checkpoint yet, and a tree branches from a commit"
                    .to_owned(),
            });
        }
        std::fs::create_dir_all(&self.home).map_err(|source| MemoryError::Io {
            op: "create the worktree home",
            path: self.home.clone(),
            source,
        })?;
        let path = self.home.join(name.as_str());
        // A node that has held a tree before still has its branch: the
        // tree is a materialization, the branch is the line of work.
        // Reattaching is what makes releasing a tree cheap enough to do
        // between sessions.
        let branch = self
            .repo
            .find_branch(name.as_str(), git2::BranchType::Local)
            .ok();
        let mut opts = git2::WorktreeAddOptions::new();
        if let Some(branch) = branch.as_ref() {
            opts.reference(Some(branch.get()));
        }
        self.repo
            .worktree(name.as_str(), &path, Some(&opts))
            .map_err(|err| MemoryError::Worktree {
                op: "add a worktree",
                detail: format!("{}: {err}", name.as_str()),
            })?;
        Ok(WorktreeLease {
            name: name.clone(),
            path: path.clone(),
            disk: measure(&path)?,
        })
    }

    /// Decides a merge without making it.
    ///
    /// Fast-forward only. A node whose trunk moved underneath it does
    /// not get its work merged on top of somebody else's by a machine:
    /// it rebuilds on the trunk as it now stands and is verified again.
    /// This is the same stance the draft desk takes about a room that
    /// moved, and for the same reason - the party who knows whether the
    /// work is still right is the one who did it.
    ///
    /// Every refusal happens here, before anything moves, and the commit
    /// the trunk will point at is already known - so a caller can write
    /// the line that announces the merge before the merge exists, and
    /// still never announce one that was going to be refused.
    ///
    /// # Errors
    /// Refuses an unknown branch, a repository with no commit, and a
    /// merge that is not a fast-forward.
    pub fn plan_merge(&self, name: &WorktreeName) -> Result<PlannedMerge<'_>, MemoryError> {
        let refuse = |op: &'static str, detail: String| MemoryError::Worktree { op, detail };
        let branch = self
            .repo
            .find_branch(name.as_str(), git2::BranchType::Local)
            .map_err(|err| refuse("find a node branch", format!("{}: {err}", name.as_str())))?;
        let theirs = branch
            .get()
            .peel_to_commit()
            .map_err(|err| refuse("read a node branch", err.to_string()))?;
        let head = self
            .repo
            .head()
            .map_err(|err| refuse("read the city trunk", err.to_string()))?;
        let ours = head
            .peel_to_commit()
            .map_err(|err| refuse("read the city trunk", err.to_string()))?;
        if theirs.id() != ours.id()
            && !self
                .repo
                .graph_descendant_of(theirs.id(), ours.id())
                .unwrap_or(false)
        {
            return Err(MemoryError::MergeStale {
                name: name.as_str().to_owned(),
                detail: format!("the trunk moved to {} after this node branched", ours.id()),
            });
        }
        Ok(PlannedMerge {
            trees: self,
            target: theirs.id(),
        })
    }

    /// Moves the trunk to a commit [`Worktrees::plan_merge`] settled on.
    fn fast_forward(&self, target: git2::Oid) -> Result<(), MemoryError> {
        let refuse = |op: &'static str, detail: String| MemoryError::Worktree { op, detail };
        let mut head = self
            .repo
            .head()
            .map_err(|err| refuse("read the city trunk", err.to_string()))?;
        head.set_target(target, "sprawling: merge a verified node")
            .map_err(|err| refuse("move the city trunk", err.to_string()))?;
        let mut checkout = git2::build::CheckoutBuilder::new();
        checkout.force();
        self.repo
            .checkout_head(Some(&mut checkout))
            .map_err(|err| refuse("check out the merged trunk", err.to_string()))?;
        Ok(())
    }

    /// Gives a tree back: the files go, then the repository forgets it.
    ///
    /// # Errors
    /// Propagates a directory that cannot be removed and a repository
    /// that refuses to prune.
    pub fn release(&self, lease: WorktreeLease) -> Result<(), MemoryError> {
        if lease.path().exists() {
            std::fs::remove_dir_all(lease.path()).map_err(|source| MemoryError::Io {
                op: "remove a worktree",
                path: lease.path().to_path_buf(),
                source,
            })?;
        }
        let tree = self
            .repo
            .find_worktree(lease.name().as_str())
            .map_err(|err| MemoryError::Worktree {
                op: "find a worktree",
                detail: format!("{}: {err}", lease.name().as_str()),
            })?;
        let mut opts = git2::WorktreePruneOptions::new();
        opts.valid(true).working_tree(true);
        tree.prune(Some(&mut opts))
            .map_err(|err| MemoryError::Worktree {
                op: "prune a worktree",
                detail: format!("{}: {err}", lease.name().as_str()),
            })
    }

    /// Every tree the repository knows about, sorted.
    ///
    /// # Errors
    /// Propagates a repository that cannot list its worktrees.
    pub fn live(&self) -> Result<Vec<WorktreeName>, MemoryError> {
        let names = self.repo.worktrees().map_err(|err| MemoryError::Worktree {
            op: "list worktrees",
            detail: err.to_string(),
        })?;
        let mut out = Vec::new();
        // A name git cannot render as UTF-8 was not written by this
        // module, and it is not a tree this city can address.
        for name in names.iter().flatten().flatten() {
            out.push(WorktreeName::parse(name)?);
        }
        out.sort();
        Ok(out)
    }
}

/// Bytes under a directory, git's own bookkeeping excluded. An explicit
/// worklist rather than recursion: a deep tree is a data-dependent depth,
/// and a stack overflow is not a failure a caller can handle.
fn measure(root: &Path) -> Result<ByteLen, MemoryError> {
    let mut total: u64 = 0;
    let mut pending = vec![root.to_path_buf()];
    while let Some(dir) = pending.pop() {
        let entries = match std::fs::read_dir(&dir) {
            Ok(entries) => entries,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
            Err(source) => {
                return Err(MemoryError::Io {
                    op: "measure a worktree",
                    path: dir.clone(),
                    source,
                });
            }
        };
        for entry in entries.flatten() {
            let path = entry.path();
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_symlink() {
                continue; // a link's target is measured where it lives
            }
            if kind.is_dir() {
                if path.file_name().is_some_and(|name| name == ".git") {
                    continue;
                }
                pending.push(path);
            } else if let Ok(meta) = entry.metadata() {
                total = total.saturating_add(meta.len());
            }
        }
    }
    Ok(ByteLen::new(total))
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
    use crate::checkpoint::Checkpoint;
    use kernel::TimeMs;

    /// A city with one checkpoint, which is what a tree branches from.
    fn city(dir: &Path) -> Worktrees {
        std::fs::create_dir_all(dir.join("lab")).unwrap();
        std::fs::write(dir.join("lab").join("notes.md"), b"first\n").unwrap();
        let mut checkpoint = Checkpoint::open(dir).unwrap();
        checkpoint
            .wave_pre("lab", TimeMs::new(1_000), "owner")
            .unwrap();
        Worktrees::open(dir).unwrap()
    }

    fn name(raw: &str) -> WorktreeName {
        WorktreeName::parse(raw).unwrap()
    }

    #[test]
    fn two_nodes_get_two_trees_and_neither_sees_the_others_work() {
        let dir = tempfile::tempdir().unwrap();
        let trees = city(dir.path());

        let first = trees.claim(&name("node-1")).unwrap();
        let second = trees.claim(&name("node-2")).unwrap();
        assert_ne!(first.path(), second.path());

        std::fs::write(first.path().join("lab").join("notes.md"), b"mine\n").unwrap();
        let other = std::fs::read_to_string(second.path().join("lab").join("notes.md")).unwrap();
        assert_eq!(other, "first\n", "the other node still sees the checkpoint");
        let city_copy = std::fs::read_to_string(dir.path().join("lab").join("notes.md")).unwrap();
        assert_eq!(city_copy, "first\n", "and so does the city");
    }

    #[test]
    fn one_node_holds_one_tree_and_the_second_claim_is_refused_by_name() {
        let dir = tempfile::tempdir().unwrap();
        let trees = city(dir.path());
        let held = trees.claim(&name("node-1")).unwrap();

        let err = trees.claim(&name("node-1")).unwrap_err();
        let ax = err.into_ax();
        assert_eq!(ax.code(), &kernel::AxCode::WorktreeBusy);
        assert!(ax.subject().contains("node-1"));
        assert!(ax.recovery().contains("release"));

        trees.release(held).unwrap();
        assert!(
            trees.live().unwrap().is_empty(),
            "a released tree is gone from the repository, not just from disk"
        );
        trees.claim(&name("node-1")).unwrap();
    }

    #[test]
    fn a_released_tree_takes_its_files_with_it() {
        let dir = tempfile::tempdir().unwrap();
        let trees = city(dir.path());
        let lease = trees.claim(&name("node-1")).unwrap();
        let path = lease.path().to_path_buf();
        assert!(path.join("lab").join("notes.md").exists());

        trees.release(lease).unwrap();
        assert!(!path.exists());
        assert!(dir.path().join("lab").join("notes.md").exists());
    }

    #[test]
    fn a_node_that_comes_back_finds_what_it_committed_and_not_what_it_did_not() {
        let dir = tempfile::tempdir().unwrap();
        let trees = city(dir.path());
        let lease = trees.claim(&name("node-1")).unwrap();
        std::fs::write(lease.path().join("lab").join("notes.md"), b"committed\n").unwrap();
        Checkpoint::open(lease.path())
            .unwrap()
            .wave_pre("lab", TimeMs::new(2_000), "node-1")
            .unwrap();
        std::fs::write(lease.path().join("lab").join("draft.md"), b"uncommitted\n").unwrap();
        trees.release(lease).unwrap();

        let again = trees.claim(&name("node-1")).unwrap();
        assert_eq!(
            std::fs::read_to_string(again.path().join("lab").join("notes.md")).unwrap(),
            "committed\n",
            "the node's branch outlives its tree"
        );
        assert!(
            !again.path().join("lab").join("draft.md").exists(),
            "what was never committed was never the node's work"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("lab").join("notes.md")).unwrap(),
            "first\n",
            "and none of it reached the city, which is what the PR flow is for"
        );
    }

    #[test]
    fn the_tree_is_measured_rather_than_estimated() {
        let dir = tempfile::tempdir().unwrap();
        let trees = city(dir.path());
        let lease = trees.claim(&name("node-1")).unwrap();
        assert!(
            lease.disk().get() >= 6,
            "the checked-out file has six bytes"
        );
        assert!(lease.disk().get() < WORKTREE_MAX_BYTES);

        let payload = lease.opened_payload().unwrap();
        let map = payload.as_map();
        assert_eq!(map.get("name").and_then(Value::as_str), Some("node-1"));
        assert!(map.contains_key("disk_bytes"));
        assert!(
            !map.contains_key("path"),
            "an absolute path is a fact about this machine, not about the city"
        );
    }

    #[test]
    fn a_city_with_no_checkpoint_is_told_why_it_cannot_have_a_tree() {
        let dir = tempfile::tempdir().unwrap();
        git2::Repository::init(dir.path()).unwrap();
        let trees = Worktrees::open(dir.path()).unwrap();

        let err = trees.claim(&name("node-1")).unwrap_err().into_ax();
        assert!(err.subject().contains("no checkpoint"));
    }

    #[test]
    fn a_verified_node_lands_in_the_city_and_a_stale_one_is_sent_back() {
        let dir = tempfile::tempdir().unwrap();
        let trees = city(dir.path());
        let lease = trees.claim(&name("node-1")).unwrap();
        std::fs::write(
            lease.path().join("lab").join("notes.md"),
            b"from the node\n",
        )
        .unwrap();
        Checkpoint::open(lease.path())
            .unwrap()
            .wave_pre("lab", TimeMs::new(2_000), "node-1")
            .unwrap();

        trees.plan_merge(lease.name()).unwrap().apply().unwrap();
        assert_eq!(
            std::fs::read_to_string(dir.path().join("lab").join("notes.md")).unwrap(),
            "from the node\n",
            "the city works on what the node produced"
        );

        // A second node that branched before the merge is now behind.
        let stale = trees.claim(&name("node-2")).unwrap();
        std::fs::write(dir.path().join("lab").join("other.md"), b"trunk moved\n").unwrap();
        Checkpoint::open(dir.path())
            .unwrap()
            .wave_pre("lab", TimeMs::new(3_000), "owner")
            .unwrap();
        std::fs::write(stale.path().join("lab").join("notes.md"), b"stale work\n").unwrap();
        Checkpoint::open(stale.path())
            .unwrap()
            .wave_pre("lab", TimeMs::new(4_000), "node-2")
            .unwrap();

        let err = trees.plan_merge(stale.name()).unwrap_err().into_ax();
        assert_eq!(err.code(), &kernel::AxCode::VersionConflict);
        assert!(err.recovery().contains("verified again"));
        assert_eq!(
            std::fs::read_to_string(dir.path().join("lab").join("notes.md")).unwrap(),
            "from the node\n",
            "a refused merge changes nothing in the city"
        );
    }

    #[test]
    fn a_city_with_no_repository_is_refused_rather_than_given_one() {
        let dir = tempfile::tempdir().unwrap();
        let err = Worktrees::open(dir.path()).unwrap_err().into_ax();
        assert_eq!(err.code(), &kernel::AxCode::StorageFatal);
    }

    #[test]
    fn a_name_that_could_walk_out_of_its_directory_is_not_a_name() {
        for raw in ["", ".hidden", "../escape", "node/1", "node 1", "nöde"] {
            assert!(
                WorktreeName::parse(raw).is_err(),
                "{raw:?} is not a worktree name"
            );
        }
        assert_eq!(name("node-1.2_3").as_str(), "node-1.2_3");
    }
}
