// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The production `Vfs`: `std::fs`, with one piece of state.
//!
//! Zero policy. Every method is the std call the trait names, and the
//! one thing this adapter remembers — the handle on the file it is
//! appending to — exists because opening a file costs a syscall per
//! call rather than per byte, and a ledger appends to one segment over
//! and over (V3.07).

use std::io;
use std::path::{Path, PathBuf};

use crate::vfs::Vfs;

/// std::fs, with one piece of state: the file it is appending to stays
/// open between calls.
///
/// Opening a file is a syscall whose cost is paid per call rather than
/// per byte, and the ledger appends to one segment over and over.
/// Reopening it for the append and again for the sync put two of those
/// under every record. Measured on windows-x86_64 NVMe, 200-byte
/// records: 979.7 us per record when both reopened, 576.6 us when both
/// use a held handle, of which 574 us is the barrier itself. This is the
/// write side of what `LineReader` does on the read side, for the same
/// reason and in the same shape.
///
/// The handle is let go before anything that renames, truncates or
/// removes the file, because this process's own open handle is enough to
/// make Windows refuse all three.
pub(crate) struct RealFs {
    open: Option<OpenAppend>,
}

/// The file `RealFs` is appending to, and where it lives.
struct OpenAppend {
    path: PathBuf,
    file: std::fs::File,
}

impl RealFs {
    pub(crate) fn new() -> RealFs {
        RealFs { open: None }
    }

    /// The handle on `path`, opening it when the one held is for another
    /// file. Append mode, so the offset is the end of the file at every
    /// write and no position has to be remembered across calls.
    fn writer(&mut self, path: &Path) -> io::Result<&mut std::fs::File> {
        if self.open.as_ref().is_none_or(|held| held.path != path) {
            let file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)?;
            self.open = Some(OpenAppend {
                path: path.to_path_buf(),
                file,
            });
        }
        match self.open.as_mut() {
            Some(held) => Ok(&mut held.file),
            // The branch above has just filled it, so this is a defect in
            // this function rather than a state a caller can reach. It
            // still returns rather than panicking, because a storage
            // adapter that can panic makes every writer above it able to.
            None => Err(io::Error::other(
                "the segment handle vanished after opening it",
            )),
        }
    }

    /// Lets go of the handle when it is the one on `path`.
    fn release(&mut self, path: &Path) {
        if self.open.as_ref().is_some_and(|held| held.path == path) {
            self.open = None;
        }
    }
}

impl Vfs for RealFs {
    fn create_dir_all(&mut self, dir: &Path) -> io::Result<()> {
        std::fs::create_dir_all(dir)
    }

    fn list(&self, dir: &Path) -> io::Result<Vec<PathBuf>> {
        let mut files = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_file() {
                files.push(path);
            }
        }
        files.sort();
        Ok(files)
    }

    fn list_dirs(&self, dir: &Path) -> io::Result<Vec<PathBuf>> {
        let mut dirs = Vec::new();
        for entry in std::fs::read_dir(dir)? {
            let path = entry?.path();
            if path.is_dir() {
                dirs.push(path);
            }
        }
        dirs.sort();
        Ok(dirs)
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        std::fs::read(path)
    }

    fn append(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        use std::io::Write as _;
        self.writer(path)?.write_all(bytes)
    }

    fn truncate(&mut self, path: &Path, len: u64) -> io::Result<()> {
        self.release(path);
        let file = std::fs::OpenOptions::new().write(true).open(path)?;
        file.set_len(len)
    }

    /// The same handle the append went through, so the bytes made
    /// durable are the ones this adapter just wrote.
    fn sync_data(&mut self, path: &Path) -> io::Result<()> {
        self.writer(path)?.sync_data()
    }

    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        self.release(from);
        self.release(to);
        std::fs::rename(from, to)
    }

    #[cfg(windows)]
    fn sync_dir(&mut self, _dir: &Path) -> io::Result<()> {
        Ok(())
    }

    #[cfg(not(windows))]
    fn sync_dir(&mut self, dir: &Path) -> io::Result<()> {
        std::fs::File::open(dir)?.sync_all()
    }

    fn remove_file(&mut self, path: &Path) -> io::Result<()> {
        self.release(path);
        std::fs::remove_file(path)
    }

    fn exists(&self, path: &Path) -> bool {
        path.is_file()
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
    use std::fs;

    /// A handle names a file, not a path, so one kept across a rename or
    /// a removal writes into the file that moved or the file that is
    /// gone - and the path the caller asked for stays empty. Both moves
    /// happen on a file this adapter has just written: CAS names an
    /// object by renaming the temporary file it wrote, and tail recovery
    /// removes the segment it could not repair.
    ///
    /// Windows does not refuse either operation - Rust opens files
    /// sharing delete and rename - so nothing stops the stale handle.
    /// What catches it is asking, afterwards, where the bytes went.
    #[test]
    fn a_handle_kept_across_a_rename_or_a_removal_would_write_into_the_wrong_file() {
        let dir = tempfile::tempdir().unwrap();
        let mut fs = RealFs::new();
        let half_written = dir.path().join("half-written");
        let named = dir.path().join("named");

        fs.append(&half_written, b"first\n").unwrap();
        fs.sync_data(&half_written).unwrap();
        fs.rename(&half_written, &named).unwrap();
        // The same path again is a new file, not more of the old one.
        fs.append(&half_written, b"second\n").unwrap();
        fs.sync_data(&half_written).unwrap();
        assert_eq!(
            fs::read(&named).unwrap(),
            b"first\n",
            "the renamed file is closed"
        );
        assert_eq!(fs::read(&half_written).unwrap(), b"second\n");

        // And after a removal the next write recreates the file rather
        // than disappearing into the one that was deleted.
        fs.remove_file(&named).unwrap();
        fs.append(&named, b"third\n").unwrap();
        fs.sync_data(&named).unwrap();
        assert_eq!(fs::read(&named).unwrap(), b"third\n");

        // Truncation is the third repair, and the write after it must
        // land at the shortened end.
        fs.truncate(&named, 2).unwrap();
        fs.append(&named, b"ird\n").unwrap();
        assert_eq!(fs::read(&named).unwrap(), b"third\n");
    }

    /// One handle serves a stretch of appends to one file and moves when
    /// the file does. A segment roll changes the path mid-wave, and an
    /// adapter that kept writing through the handle it already had would
    /// put the second half of the wave in the first segment.
    #[test]
    fn the_handle_follows_the_file_when_a_wave_rolls_to_a_new_segment() {
        let dir = tempfile::tempdir().unwrap();
        let mut fs = RealFs::new();
        let first = dir.path().join("first");
        let second = dir.path().join("second");
        fs.append(&first, b"a").unwrap();
        fs.append(&second, b"b").unwrap();
        fs.append(&first, b"c").unwrap();
        fs.sync_data(&first).unwrap();
        fs.sync_data(&second).unwrap();
        assert_eq!(fs::read(&first).unwrap(), b"ac");
        assert_eq!(fs::read(&second).unwrap(), b"b");
    }
}
