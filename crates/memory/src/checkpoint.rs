// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Checkpoint fences around a tool wave.
//!
//! Before the wave, everything inside the write domain is committed;
//! after it, every file the wave deleted is recorded as discarded with
//! that commit as its restoration address. The order is the whole
//! point — a deletion recorded against a commit that does not yet exist
//! is not restorable, so the fence goes up first and the accounting
//! follows.
//!
//! Two boundaries are hard. The scope is the write domain and nothing
//! wider: a whole-tree `add` would stage files the Run was never given,
//! which this design refuses outright. And time is a parameter,
//! never sampled — the git signature carries the injected instant, so
//! the same script replays to the same commits.
//!
//! Staged content is scanned before it can be committed. A hit refuses
//! the commit and reports positions only; echoing the matched bytes to
//! prove a secret leaked would be the leak.

use std::path::Path;

use kernel::{Payload, TimeMs, scan};
use serde_json::{Map, Value};

use crate::jsonl::MemoryError;

const IDENTITY_NAME: &str = "sprawling";
const IDENTITY_EMAIL: &str = "sprawling@local";

pub struct Checkpoint {
    repo: git2::Repository,
}

fn git_err(op: &'static str) -> impl FnOnce(git2::Error) -> MemoryError {
    move |err| MemoryError::Checkpoint {
        op,
        detail: err.message().to_owned(),
    }
}

impl Checkpoint {
    /// Opens the city repository, initialising one when absent. The
    /// genesis commit is the first wave's, not this call's: an empty
    /// repository is a valid state, and inventing history here would
    /// make the first checkpoint unattributable.
    pub fn open(city_root: &Path) -> Result<Checkpoint, MemoryError> {
        let repo = match git2::Repository::open(city_root) {
            Ok(repo) => repo,
            Err(_) => git2::Repository::init(city_root).map_err(git_err("init repository"))?,
        };
        // The city's files round-trip byte for byte, whatever this
        // machine's git is configured to do to other people's
        // repositories. A checkout that rewrote line endings would make
        // a file disagree with the hash the ledger holds for it, and the
        // disagreement would look like corruption rather than like a
        // setting. Set on every open, because the setting is a property
        // of this repository rather than of the moment it was created.
        repo.config()
            .and_then(|mut config| config.set_bool("core.autocrlf", false))
            .map_err(git_err("pin the repository's line endings"))?;
        Ok(Checkpoint { repo })
    }

    /// Makes sure the city has one commit, and makes no more than that.
    ///
    /// A worktree branches from a commit, so a city that has never been
    /// fenced cannot lend a tree. Committing on every dispatch would
    /// instead move the trunk under every request already waiting, and a
    /// fast-forward merge would then refuse work nobody had touched.
    /// Returns the commit it made, or `None` when there already was one.
    ///
    /// # Errors
    /// Propagates whatever staging and committing report.
    pub fn ensure_base(
        &mut self,
        scope: &str,
        t: TimeMs,
        who: &str,
    ) -> Result<Option<Payload>, MemoryError> {
        if self.repo.head().is_ok() {
            return Ok(None);
        }
        self.wave_pre(scope, t, who).map(Some)
    }

    /// The pre-wave fence: stage everything under `scope`, scan it, and
    /// commit at the injected time. Returns the `checkpoint_committed`
    /// payload.
    pub fn wave_pre(&mut self, scope: &str, t: TimeMs, who: &str) -> Result<Payload, MemoryError> {
        let files = self.stage_scope(scope)?;
        self.scan_staged()?;
        let oid = self.commit(t, who, &format!("checkpoint: {scope}"))?;
        let mut map = Map::new();
        map.insert("oid".to_owned(), Value::String(oid));
        map.insert("scope".to_owned(), Value::String(scope.to_owned()));
        map.insert(
            "files".to_owned(),
            Value::Array(files.into_iter().map(Value::String).collect()),
        );
        Payload::new(map).map_err(|source| MemoryError::Draft { source })
    }

    /// The post-wave sweep: every path present at `pre_oid` and gone
    /// from the working tree becomes a `file_discarded` payload whose
    /// restoration points back into that commit.
    pub fn wave_post(&mut self, pre_oid: &str) -> Result<Vec<Payload>, MemoryError> {
        let oid = git2::Oid::from_str(pre_oid).map_err(git_err("parse checkpoint oid"))?;
        let commit = self
            .repo
            .find_commit(oid)
            .map_err(git_err("find checkpoint commit"))?;
        let tree = commit.tree().map_err(git_err("read checkpoint tree"))?;
        // git already knows what is missing, so it is asked once instead
        // of being told the answer file by file. Walking the whole tree
        // and calling `exists` on every blob cost one `format!`, one
        // `PathBuf` and one filesystem stat per tracked file, after every
        // wave, whether or not that wave touched anything.
        let mut options = git2::DiffOptions::new();
        options.include_typechange(true);
        let diff = self
            .repo
            .diff_tree_to_workdir(Some(&tree), Some(&mut options))
            .map_err(git_err("diff checkpoint against the work tree"))?;
        let mut deleted: Vec<String> = Vec::new();
        for delta in diff.deltas() {
            if delta.status() != git2::Delta::Deleted {
                continue;
            }
            if let Some(path) = delta.old_file().path().and_then(|p| p.to_str()) {
                deleted.push(path.replace('\\', "/"));
            }
        }
        // Sorted here rather than trusted from the diff: the order these
        // records land in is part of what a replay reproduces.
        deleted.sort();
        let mut payloads = Vec::new();
        for path in deleted {
            let mut map = Map::new();
            map.insert(
                "paths".to_owned(),
                Value::Array(vec![Value::String(format!("file:{path}"))]),
            );
            let mut restoration = Map::new();
            restoration.insert(
                "tracked".to_owned(),
                Value::String(format!("file:{path}@{pre_oid}")),
            );
            map.insert("restoration".to_owned(), Value::Object(restoration));
            payloads.push(Payload::new(map).map_err(|source| MemoryError::Draft { source })?);
        }
        Ok(payloads)
    }

    /// Scans staged content. Reports how many shapes matched and where,
    /// never what matched.
    pub fn scan_staged(&mut self) -> Result<(), MemoryError> {
        let index = self.repo.index().map_err(git_err("read index"))?;
        let mut hits: Vec<String> = Vec::new();
        for entry in index.iter() {
            let blob = match self.repo.find_blob(entry.id) {
                Ok(blob) => blob,
                Err(_) => continue,
            };
            let spans = scan(blob.content());
            if spans.is_empty() {
                continue;
            }
            let path = String::from_utf8_lossy(&entry.path).into_owned();
            for span in spans {
                hits.push(format!("{path}:{}+{}", span.start, span.len));
            }
        }
        if hits.is_empty() {
            return Ok(());
        }
        Err(MemoryError::SecretEgress {
            locations: hits.join(" "),
        })
    }

    /// Stages every file under `scope`, including deletions. Paths
    /// outside the scope are never touched — the write domain is the
    /// boundary, and a wider `add` would stage what the Run never held.
    fn stage_scope(&mut self, scope: &str) -> Result<Vec<String>, MemoryError> {
        let mut index = self.repo.index().map_err(git_err("read index"))?;
        let pattern = if scope.is_empty() || scope == "." {
            "*".to_owned()
        } else {
            format!("{}/*", scope.trim_end_matches('/'))
        };
        index
            .add_all([&pattern], git2::IndexAddOption::DEFAULT, None)
            .map_err(git_err("stage scope"))?;
        index
            .update_all([&pattern], None)
            .map_err(git_err("stage deletions"))?;
        index.write().map_err(git_err("write index"))?;
        let mut files: Vec<String> = index
            .iter()
            .map(|entry| String::from_utf8_lossy(&entry.path).into_owned())
            .collect();
        files.sort();
        Ok(files)
    }

    /// Commits the index at the injected time. An unchanged tree still
    /// commits: a rebuildable chain is worth more than a saved object.
    fn commit(&mut self, t: TimeMs, who: &str, message: &str) -> Result<String, MemoryError> {
        let mut index = self.repo.index().map_err(git_err("read index"))?;
        let tree_oid = index.write_tree().map_err(git_err("write tree"))?;
        let tree = self
            .repo
            .find_tree(tree_oid)
            .map_err(git_err("find staged tree"))?;
        let seconds = i64::try_from(t.value().saturating_div(1000)).unwrap_or(0);
        let when = git2::Time::new(seconds, 0);
        let signature = git2::Signature::new(IDENTITY_NAME, IDENTITY_EMAIL, &when)
            .map_err(git_err("build signature"))?;
        let parents: Vec<git2::Commit> = match self.repo.head() {
            Ok(head) => match head.peel_to_commit() {
                Ok(commit) => vec![commit],
                Err(_) => Vec::new(),
            },
            Err(_) => Vec::new(),
        };
        let parent_refs: Vec<&git2::Commit> = parents.iter().collect();
        let full_message = format!("{message}\n\nactor: {who}\n");
        let oid = self
            .repo
            .commit(
                Some("HEAD"),
                &signature,
                &signature,
                &full_message,
                &tree,
                &parent_refs,
            )
            .map_err(git_err("commit checkpoint"))?;
        Ok(oid.to_string())
    }
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

    fn write(root: &Path, rel: &str, body: &str) {
        let path = root.join(rel);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(path, body).unwrap();
    }

    fn oid_of(payload: &Payload) -> String {
        serde_json::to_value(payload).unwrap()["oid"]
            .as_str()
            .unwrap()
            .to_owned()
    }

    #[test]
    fn a14_the_fence_precedes_the_deletion_it_restores() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "work/keep.txt", "kept");
        write(tmp.path(), "work/doomed.txt", "about to go");
        let mut checkpoint = Checkpoint::open(tmp.path()).unwrap();

        let pre = checkpoint
            .wave_pre("work", TimeMs::new(1_700_000_000_000), "resident")
            .unwrap();
        let pre_oid = oid_of(&pre);
        let files = serde_json::to_value(&pre).unwrap();
        assert_eq!(files["files"].as_array().unwrap().len(), 2);

        // The wave deletes a file.
        std::fs::remove_file(tmp.path().join("work/doomed.txt")).unwrap();

        let discards = checkpoint.wave_post(&pre_oid).unwrap();
        assert_eq!(discards.len(), 1);
        let value = serde_json::to_value(&discards[0]).unwrap();
        assert_eq!(value["paths"][0], "file:work/doomed.txt");
        assert_eq!(
            value["restoration"]["tracked"],
            format!("file:work/doomed.txt@{pre_oid}"),
            "the restoration names a commit that already exists"
        );
    }

    #[test]
    fn the_scope_is_the_boundary_and_outside_it_nothing_is_staged() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "work/mine.txt", "in domain");
        write(tmp.path(), "elsewhere/theirs.txt", "not mine");
        let mut checkpoint = Checkpoint::open(tmp.path()).unwrap();
        let pre = checkpoint
            .wave_pre("work", TimeMs::new(1_700_000_000_000), "resident")
            .unwrap();
        let files = serde_json::to_value(&pre).unwrap();
        let staged: Vec<String> = files["files"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| v.as_str().unwrap().to_owned())
            .collect();
        assert_eq!(staged, vec!["work/mine.txt".to_owned()]);
    }

    #[test]
    fn a_staged_secret_refuses_the_commit_and_never_echoes_it() {
        let tmp = tempfile::tempdir().unwrap();
        let token = ["sk-ant-api03-", "Zx9yQ2mK4pL7", "vB1nC5tR8sD3"].concat();
        write(tmp.path(), "work/leak.env", &format!("KEY={token}"));
        let mut checkpoint = Checkpoint::open(tmp.path()).unwrap();
        let err = match checkpoint.wave_pre("work", TimeMs::new(0), "resident") {
            Err(err) => err,
            Ok(_) => panic!("a staged secret must refuse the commit"),
        };
        let rendered = err.to_string();
        assert!(rendered.contains("work/leak.env"), "{rendered}");
        assert!(
            !rendered.contains(&token) && !rendered.contains("Zx9yQ2mK4pL7"),
            "positions only, never the bytes: {rendered}"
        );
        let ax = err.into_ax();
        assert_eq!(*ax.code(), kernel::AxCode::SecretEgress);
    }

    #[test]
    fn an_unchanged_wave_still_commits_so_the_chain_rebuilds() {
        let tmp = tempfile::tempdir().unwrap();
        write(tmp.path(), "work/steady.txt", "unchanged");
        let mut checkpoint = Checkpoint::open(tmp.path()).unwrap();
        let first = oid_of(
            &checkpoint
                .wave_pre("work", TimeMs::new(1_000), "resident")
                .unwrap(),
        );
        let second = oid_of(
            &checkpoint
                .wave_pre("work", TimeMs::new(2_000), "resident")
                .unwrap(),
        );
        assert_ne!(first, second, "each fence is its own commit");
        assert!(checkpoint.wave_post(&second).unwrap().is_empty());
    }

    #[test]
    fn the_same_script_at_the_same_time_produces_the_same_commit() {
        let build = |dir: &Path| -> String {
            write(dir, "work/a.txt", "alpha");
            let mut checkpoint = Checkpoint::open(dir).unwrap();
            oid_of(
                &checkpoint
                    .wave_pre("work", TimeMs::new(1_700_000_000_000), "resident")
                    .unwrap(),
            )
        };
        let one = tempfile::tempdir().unwrap();
        let two = tempfile::tempdir().unwrap();
        assert_eq!(
            build(one.path()),
            build(two.path()),
            "time is a parameter, so the oid is reproducible"
        );
    }
}
