// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The filesystem seam this crate reaches disk through, and nothing else.
//!
//! One trait, two adapters: [`crate::real_fs::RealFs`] over `std::fs` in
//! production, and `FaultFs` (S1.08) over a deterministic power-loss
//! model. The second adapter is what makes this a seam rather than a
//! layer of indirection — a power cut is a property of filesystem
//! semantics, so it is injected here and every module above stays free
//! of test hooks (memory-SPEC 8.5, design A).
//!
//! **The seam stays inner.** `Vfs` is `pub(crate)`: `JsonlLedger` holds
//! a `Box<dyn Vfs>` and never names it in a public signature, because a
//! private trait in a public signature does not compile (E0445) and
//! because what a city stores its history on is not a decision this
//! crate delegates outward.

use std::io;
use std::path::{Path, PathBuf};

/// Crate-internal filesystem seam. Two adapters:
/// [`crate::real_fs::RealFs`] (std) and `FaultFs` (fault injection,
/// S1.08). Never public: the seam stays inner.
pub(crate) trait Vfs {
    fn create_dir_all(&mut self, dir: &Path) -> io::Result<()>;
    /// Files only, sorted by path: deterministic traversal.
    fn list(&self, dir: &Path) -> io::Result<Vec<PathBuf>>;
    /// Subdirectories only, sorted. Shallow like `list`, so a caller
    /// that wants a tree walks it with an explicit worklist rather than
    /// by recursion, and cannot overflow a stack on a deep city.
    fn list_dirs(&self, dir: &Path) -> io::Result<Vec<PathBuf>>;
    fn read(&self, path: &Path) -> io::Result<Vec<u8>>;
    /// Creates the file when absent.
    fn append(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()>;
    fn truncate(&mut self, path: &Path, len: u64) -> io::Result<()>;
    fn sync_data(&mut self, path: &Path) -> io::Result<()>;
    /// Atomic replace; durability of the new entry still needs sync_dir.
    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()>;
    /// Durability of directory entries; explicit no-op on Windows (no
    /// directory-handle sync primitive there — memory-SPEC 3-3).
    fn sync_dir(&mut self, dir: &Path) -> io::Result<()>;
    fn remove_file(&mut self, path: &Path) -> io::Result<()>;
    fn exists(&self, path: &Path) -> bool;
}
