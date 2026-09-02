// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! CAS: content-addressed storage under BLAKE3.
//!
//! Three engineering facts owned here:
//! - writes go tmp + rename: power loss leaves half-done temp files only,
//!   never a corrupt named object (A3 point 2). Temp names derive from
//!   the content hash — concurrent writers of the same bytes converge,
//!   no randomness anywhere.
//! - dedup is existence: a second put of the same bytes returns the hash
//!   without re-materializing.
//! - range retrieval follows the Locator grammar: `L` 1-based closed
//!   (interior newlines kept, final terminator excluded), `B` 0-based
//!   closed; out of bounds refuses, never clamps.
//!
//! Full reads re-verify the hash (BLAKE3 runs at GB/s); range reads trust
//! the object as verified at put.

use std::path::{Path, PathBuf};

use kernel::{B3Hash, Range};

use crate::error::{MemoryError, io_err};
use crate::real_fs::RealFs;
use crate::vfs::Vfs;

pub struct Cas {
    vfs: Box<dyn Vfs>,
    dir: PathBuf,
}

impl Cas {
    /// Production entrance: std filesystem underneath.
    pub fn open(dir: &Path) -> Result<Cas, MemoryError> {
        Cas::open_with(Box::new(RealFs::new()), dir)
    }

    /// Injection point for the fault adapter (tests; citysim gets a
    /// public constructor with the `fault` feature when S4 needs it).
    pub(crate) fn open_with(mut vfs: Box<dyn Vfs>, dir: &Path) -> Result<Cas, MemoryError> {
        let objects = dir.join("b3");
        let tmp = dir.join("tmp");
        vfs.create_dir_all(&objects)
            .map_err(io_err("create cas dir", &objects))?;
        vfs.create_dir_all(&tmp)
            .map_err(io_err("create cas tmp dir", &tmp))?;
        // Sweep crash residue: rename is atomic, so anything in tmp is a
        // half-done write whose put never returned Ok.
        let leftovers = vfs.list(&tmp).map_err(io_err("list cas tmp", &tmp))?;
        for leftover in leftovers {
            vfs.remove_file(&leftover)
                .map_err(io_err("sweep cas tmp", &leftover))?;
        }
        Ok(Cas {
            vfs,
            dir: dir.to_path_buf(),
        })
    }

    fn object_path(&self, hash: &B3Hash) -> (PathBuf, PathBuf) {
        let hex = hash.to_string();
        let shard = hex.get(..2).unwrap_or("00");
        let shard_dir = self.dir.join("b3").join(shard);
        let path = shard_dir.join(&hex);
        (shard_dir, path)
    }

    /// Content-addressed put: hash, dedup by existence, tmp + sync +
    /// rename + dir sync. `Ok` means the named object is durable.
    pub fn put(&mut self, bytes: &[u8]) -> Result<B3Hash, MemoryError> {
        let hash = B3Hash::from_bytes(*blake3::hash(bytes).as_bytes());
        let (shard_dir, path) = self.object_path(&hash);
        if self.vfs.exists(&path) {
            return Ok(hash);
        }
        self.vfs
            .create_dir_all(&shard_dir)
            .map_err(io_err("create cas shard", &shard_dir))?;
        let tmp = self.dir.join("tmp").join(format!("{hash}.part"));
        if self.vfs.exists(&tmp) {
            self.vfs
                .truncate(&tmp, 0)
                .map_err(io_err("reset cas tmp", &tmp))?;
        }
        self.vfs
            .append(&tmp, bytes)
            .map_err(io_err("write cas tmp", &tmp))?;
        self.vfs
            .sync_data(&tmp)
            .map_err(io_err("sync cas tmp", &tmp))?;
        self.vfs
            .rename(&tmp, &path)
            .map_err(io_err("rename cas object", &path))?;
        self.vfs
            .sync_dir(&shard_dir)
            .map_err(io_err("sync cas shard", &shard_dir))?;
        Ok(hash)
    }

    /// Existence judgment; no failure surface.
    pub fn contains(&self, hash: &B3Hash) -> bool {
        let (_, path) = self.object_path(hash);
        self.vfs.exists(&path)
    }

    /// Full read, re-verified against the address.
    pub fn get(&self, hash: &B3Hash) -> Result<Vec<u8>, MemoryError> {
        let (_, path) = self.object_path(hash);
        if !self.vfs.exists(&path) {
            return Err(MemoryError::CasMissing {
                hash: hash.to_string(),
            });
        }
        let bytes = self
            .vfs
            .read(&path)
            .map_err(io_err("read cas object", &path))?;
        let echo = blake3::hash(&bytes);
        if echo.as_bytes() != hash.as_bytes() {
            return Err(MemoryError::CasCorrupt {
                hash: hash.to_string(),
                path,
            });
        }
        Ok(bytes)
    }

    /// Range retrieval per the Locator grammar; out of bounds refuses.
    pub fn get_range(&self, hash: &B3Hash, range: &Range) -> Result<Vec<u8>, MemoryError> {
        let bytes = self.get(hash)?;
        let out_of_bounds = || MemoryError::RangeOutOfBounds {
            hash: hash.to_string(),
        };
        match range {
            Range::Bytes { from, to } => {
                let from = usize::try_from(*from).map_err(|_| out_of_bounds())?;
                let to = usize::try_from(*to).map_err(|_| out_of_bounds())?;
                let end = to.checked_add(1).ok_or_else(out_of_bounds)?;
                bytes
                    .get(from..end)
                    .map(<[u8]>::to_vec)
                    .ok_or_else(out_of_bounds)
            }
            Range::Lines { from, to } => {
                let lines: Vec<&[u8]> = bytes.split(|b| *b == b'\n').collect();
                // A trailing terminator yields one empty trailing chunk;
                // it is not a line (SPEC 8-3: unterminated last line still
                // counts as one).
                let count = match lines.last() {
                    Some(&[]) => lines.len().saturating_sub(1),
                    _ => lines.len(),
                };
                let from = usize::try_from(*from).map_err(|_| out_of_bounds())?;
                let to = usize::try_from(*to).map_err(|_| out_of_bounds())?;
                if from == 0 || to > count {
                    return Err(out_of_bounds());
                }
                let start = from.saturating_sub(1);
                let picked: Vec<&[u8]> = lines.get(start..to).ok_or_else(out_of_bounds)?.to_vec();
                Ok(picked.join(&b'\n'))
            }
        }
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
    use crate::fault_fs::{FaultFs, FaultPlan, TornTail};
    use kernel::{AxCode, Range};
    use std::fs;

    #[test]
    fn put_get_roundtrip_and_reference_hash() {
        let dir = tempfile::tempdir().unwrap();
        let mut cas = Cas::open(dir.path()).unwrap();
        let hash = cas.put(b"hello world").unwrap();
        assert_eq!(
            hash.to_string(),
            blake3::hash(b"hello world").to_hex().to_string()
        );
        assert!(cas.contains(&hash));
        assert_eq!(cas.get(&hash).unwrap(), b"hello world");
    }

    #[test]
    fn dedup_skips_rematerialization_and_tmp_stays_empty() {
        let dir = tempfile::tempdir().unwrap();
        let mut cas = Cas::open(dir.path()).unwrap();
        let first = cas.put(b"same bytes").unwrap();
        let second = cas.put(b"same bytes").unwrap();
        assert_eq!(first, second);
        let tmp_entries = fs::read_dir(dir.path().join("tmp")).unwrap().count();
        assert_eq!(tmp_entries, 0, "no leftover temp files after clean puts");
    }

    #[test]
    fn get_reverifies_and_reports_corruption() {
        let dir = tempfile::tempdir().unwrap();
        let mut cas = Cas::open(dir.path()).unwrap();
        let hash = cas.put(b"precious").unwrap();
        let hex = hash.to_string();
        let path = dir.path().join("b3").join(hex.get(..2).unwrap()).join(&hex);
        let mut bytes = fs::read(&path).unwrap();
        bytes[0] ^= 0xFF;
        fs::write(&path, &bytes).unwrap();
        let err = cas.get(&hash).unwrap_err();
        assert_eq!(err.into_ax().code(), &AxCode::CasCorrupt);
    }

    #[test]
    fn missing_object_is_path_not_found() {
        let dir = tempfile::tempdir().unwrap();
        let cas = Cas::open(dir.path()).unwrap();
        let ghost = kernel::B3Hash::from_bytes([9; 32]);
        assert!(!cas.contains(&ghost));
        let err = cas.get(&ghost).unwrap_err();
        assert_eq!(err.into_ax().code(), &AxCode::PathNotFound);
    }

    #[test]
    fn byte_and_line_ranges_follow_locator_semantics() {
        let dir = tempfile::tempdir().unwrap();
        let mut cas = Cas::open(dir.path()).unwrap();
        let hash = cas.put(b"alpha\nbeta\ngamma").unwrap();

        let bytes = cas.get_range(&hash, &Range::bytes(0, 4).unwrap()).unwrap();
        assert_eq!(bytes, b"alpha");
        let tail = cas
            .get_range(&hash, &Range::bytes(11, 15).unwrap())
            .unwrap();
        assert_eq!(tail, b"gamma");

        let one = cas.get_range(&hash, &Range::lines(2, 2).unwrap()).unwrap();
        assert_eq!(one, b"beta");
        let two = cas.get_range(&hash, &Range::lines(1, 2).unwrap()).unwrap();
        assert_eq!(two, b"alpha\nbeta");
        let all = cas.get_range(&hash, &Range::lines(1, 3).unwrap()).unwrap();
        assert_eq!(all, b"alpha\nbeta\ngamma");

        for bad in [
            Range::bytes(0, 16).unwrap(),  // past the end
            Range::bytes(16, 20).unwrap(), // fully outside
            Range::lines(1, 4).unwrap(),   // more lines than exist
            Range::lines(4, 5).unwrap(),
        ] {
            let err = cas.get_range(&hash, &bad).unwrap_err();
            assert_eq!(err.into_ax().code(), &AxCode::InvalidArgs, "{bad:?}");
        }
    }

    /// A3 point 2: power loss around CAS rename. Cut at every op of a put
    /// script; named objects never corrupt, only tmp holds leftovers, and
    /// a retried put lands the object.
    #[test]
    fn power_cut_matrix_leaves_only_tmp_halfware() {
        let content: &[u8] = b"the object under power loss";

        // Baseline op count.
        let fs0 = FaultFs::new(FaultPlan {
            cut_at_op: None,
            cut_on_write: None,
            torn_tail: TornTail::None,
        });
        let mut cas = Cas::open_with(Box::new(fs0.clone()), std::path::Path::new("c")).unwrap();
        cas.put(content).unwrap();
        let total_ops = fs0.op_count();

        for torn in [TornTail::None, TornTail::KeepBytes(5)] {
            for cut in 1..=total_ops {
                let fs = FaultFs::new(FaultPlan {
                    cut_at_op: Some(cut),
                    cut_on_write: None,
                    torn_tail: torn,
                });
                let dir = std::path::Path::new("c");
                let acknowledged = match Cas::open_with(Box::new(fs.clone()), dir) {
                    Ok(mut cas) => cas.put(content).is_ok(),
                    Err(_) => false,
                };

                // Reopen (second cut impossible: plan consumed). Sweep runs.
                let mut cas = match Cas::open_with(Box::new(fs.clone()), dir) {
                    Ok(cas) => cas,
                    Err(_) => Cas::open_with(Box::new(fs.clone()), dir).unwrap(),
                };
                let hash = kernel::B3Hash::from_bytes(*blake3::hash(content).as_bytes());
                if acknowledged {
                    assert_eq!(
                        cas.get(&hash).unwrap(),
                        content,
                        "cut {cut} ({torn:?}): acknowledged object must survive"
                    );
                }
                if cas.contains(&hash) {
                    // Present but never corrupt \u2014 the A3 point-2 claim.
                    assert_eq!(cas.get(&hash).unwrap(), content);
                }
                // Retry always lands the object.
                cas.put(content).unwrap();
                assert_eq!(cas.get(&hash).unwrap(), content);
            }
        }
    }
}
