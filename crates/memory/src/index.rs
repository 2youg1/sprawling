// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! The ledger's side index: seq to (segment, byte
//! offset), so one line costs an open plus a seek instead of a scan.
//!
//! Everything here is disposable. The on-disk cache is believed only
//! when its stamp matches what the directory looks like right now; any
//! doubt — parse failure, byte-count drift, checksum mismatch — rebuilds
//! in silence rather than reporting. A side artifact that lies is worse
//! than one that is missing, so this module never lets a stale cache
//! survive a comparison it cannot pass.

use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::Path;

use kernel::{B3Hash, Seq};

use crate::jsonl::{MemoryError, io_err, is_segment};

const CACHE_NAME: &str = "index.cache";
const CACHE_MAGIC: &str = "idx v1";

/// seq → (segment file name, byte offset of the line start).
pub struct LedgerIndex {
    entries: BTreeMap<Seq, (String, u64)>,
}

impl LedgerIndex {
    /// Loads the cache when it is checksum-fresh, otherwise scans the
    /// directory. Both paths yield the same map; only the cost differs.
    pub fn load_or_rebuild(dir: &Path) -> Result<LedgerIndex, MemoryError> {
        let stamp = directory_stamp(dir)?;
        if let Some(index) = load_cache(dir, &stamp) {
            return Ok(index);
        }
        rebuild(dir)
    }

    /// Constant-time single-line read: open the segment, seek, read to
    /// the newline. A seq absent from the index is a caller error, not a
    /// corrupt ledger.
    pub fn line_at(&self, dir: &Path, seq: Seq) -> Result<Vec<u8>, MemoryError> {
        let Some((segment, offset)) = self.entries.get(&seq) else {
            return Err(MemoryError::SeqMissing { seq: seq.value() });
        };
        let path = dir.join(segment);
        let mut file = std::fs::File::open(&path).map_err(io_err("open segment", &path))?;
        file.seek(SeekFrom::Start(*offset))
            .map_err(io_err("seek segment", &path))?;
        let mut line = Vec::new();
        let mut byte = [0u8; 1];
        loop {
            match file.read(&mut byte) {
                Ok(0) => break,
                Ok(_) => {
                    if byte.first().copied() == Some(b'\n') {
                        break;
                    }
                    line.extend_from_slice(&byte);
                }
                Err(err) => return Err(io_err("read segment", &path)(err)),
            }
        }
        Ok(line)
    }

    /// Writes the cache. Failure is not fatal — the index rebuilds next
    /// time, and a disposable artifact must never block the main path.
    pub fn persist(&self, dir: &Path) -> Result<(), MemoryError> {
        let stamp = directory_stamp(dir)?;
        let mut out = format!("{CACHE_MAGIC} {} {}\n", stamp.bytes, stamp.digest);
        for (seq, (segment, offset)) in &self.entries {
            out.push_str(&format!("{} {segment} {offset}\n", seq.value()));
        }
        let path = dir.join(CACHE_NAME);
        std::fs::write(&path, out.as_bytes()).map_err(io_err("write index cache", &path))
    }

    pub fn tail_seq(&self) -> Option<Seq> {
        self.entries.keys().next_back().copied()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

/// What the directory looks like right now: total bytes across segments
/// plus a digest of the segment names and sizes. Coarse on purpose — a
/// side artifact should rebuild too often rather than be trusted once
/// too many.
struct Stamp {
    bytes: u64,
    digest: String,
}

fn segment_names(dir: &Path) -> Result<Vec<String>, MemoryError> {
    let mut names = Vec::new();
    let entries = std::fs::read_dir(dir).map_err(io_err("list ledger dir", dir))?;
    for entry in entries {
        let entry = entry.map_err(io_err("list ledger dir", dir))?;
        let path = entry.path();
        if !is_segment(&path) {
            continue;
        }
        if let Some(name) = path.file_name().and_then(|n| n.to_str()) {
            names.push(name.to_owned());
        }
    }
    // Zero-padded names sort lexically the way they sort numerically;
    // sorting here makes the order ours, not the filesystem's.
    names.sort();
    Ok(names)
}

fn directory_stamp(dir: &Path) -> Result<Stamp, MemoryError> {
    let mut total: u64 = 0;
    let mut material = String::new();
    for name in segment_names(dir)? {
        let path = dir.join(&name);
        let size = std::fs::metadata(&path)
            .map_err(io_err("stat segment", &path))?
            .len();
        total = total.saturating_add(size);
        material.push_str(&format!("{name}:{size}\n"));
    }
    let digest = B3Hash::digest(material.as_bytes()).to_string();
    let short = digest.get(..16).unwrap_or(&digest).to_owned();
    Ok(Stamp {
        bytes: total,
        digest: short,
    })
}

fn load_cache(dir: &Path, stamp: &Stamp) -> Option<LedgerIndex> {
    let raw = std::fs::read_to_string(dir.join(CACHE_NAME)).ok()?;
    let mut lines = raw.lines();
    let header = lines.next()?;
    let expected = format!("{CACHE_MAGIC} {} {}", stamp.bytes, stamp.digest);
    if header != expected {
        return None;
    }
    let mut entries = BTreeMap::new();
    for line in lines {
        let mut parts = line.split(' ');
        let seq = parts.next()?.parse::<u64>().ok()?;
        let segment = parts.next()?.to_owned();
        let offset = parts.next()?.parse::<u64>().ok()?;
        if parts.next().is_some() {
            return None;
        }
        entries.insert(Seq::new(seq), (segment, offset));
    }
    Some(LedgerIndex { entries })
}

fn rebuild(dir: &Path) -> Result<LedgerIndex, MemoryError> {
    let mut entries = BTreeMap::new();
    for name in segment_names(dir)? {
        let path = dir.join(&name);
        let bytes = std::fs::read(&path).map_err(io_err("read segment", &path))?;
        let mut offset: u64 = 0;
        for line in bytes.split_inclusive(|b| *b == b'\n') {
            let complete = line.last().copied() == Some(b'\n');
            let body = if complete {
                line.get(..line.len().saturating_sub(1)).unwrap_or(line)
            } else {
                line
            };
            // A torn tail carries no seq we can trust; it is skipped, and
            // the next append overwrites it (jsonl owns that repair).
            if complete
                && !body.is_empty()
                && let Some(seq) = seq_of(body)
            {
                entries.insert(seq, (name.clone(), offset));
            }
            offset = offset.saturating_add(u64::try_from(line.len()).unwrap_or(0));
        }
    }
    Ok(LedgerIndex { entries })
}

/// Reads only the seq field. Indexing must not depend on the record
/// parsing cleanly — an index over a damaged ledger is exactly what a
/// repair path needs.
fn seq_of(line: &[u8]) -> Option<Seq> {
    let value: serde_json::Value = serde_json::from_slice(line).ok()?;
    value.get("seq")?.as_u64().map(Seq::new)
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
    use kernel::{EventDraft, EventKind, EventRecord, Payload, RunId, TimeMs};

    fn write_ledger(dir: &Path, count: u64) -> Vec<Vec<u8>> {
        let run = RunId::from_bytes([7u8; 16]);
        let mut lines = Vec::new();
        let mut blob = Vec::new();
        let mut prev = B3Hash::digest(b"");
        for i in 0..count {
            let draft = EventDraft {
                run,
                t: TimeMs::new(i),
                who: "tester".to_owned(),
                addr: None,
                kind: EventKind::RunStarted,
                data: Payload::new(serde_json::Map::new()).unwrap(),
                ig: false,
            };
            let record = EventRecord::from_draft(draft, Seq::new(i), prev);
            let line = record.canonical_line().unwrap();
            prev = B3Hash::digest(&line);
            blob.extend_from_slice(&line);
            blob.push(b'\n');
            lines.push(line);
        }
        std::fs::write(dir.join("ledger-00000000000000000000.jsonl"), &blob).unwrap();
        lines
    }

    #[test]
    fn rebuild_finds_every_line_and_seeking_returns_it_verbatim() {
        let tmp = tempfile::tempdir().unwrap();
        let lines = write_ledger(tmp.path(), 5);
        let index = LedgerIndex::load_or_rebuild(tmp.path()).unwrap();
        assert_eq!(index.len(), 5);
        assert_eq!(index.tail_seq(), Some(Seq::new(4)));
        for (i, expected) in lines.iter().enumerate() {
            let got = index
                .line_at(tmp.path(), Seq::new(u64::try_from(i).unwrap()))
                .unwrap();
            assert_eq!(&got, expected, "line {i} seeks back verbatim");
        }
    }

    #[test]
    fn a_fresh_cache_is_believed_and_a_stale_one_is_silently_rebuilt() {
        let tmp = tempfile::tempdir().unwrap();
        write_ledger(tmp.path(), 3);
        let index = LedgerIndex::load_or_rebuild(tmp.path()).unwrap();
        index.persist(tmp.path()).unwrap();
        // Fresh: the cache round-trips into an identical map.
        let loaded = LedgerIndex::load_or_rebuild(tmp.path()).unwrap();
        assert_eq!(loaded.len(), 3);
        assert_eq!(loaded.tail_seq(), Some(Seq::new(2)));
        // Stale: growing the ledger without refreshing the cache must
        // not yield the old answer.
        write_ledger(tmp.path(), 6);
        let after = LedgerIndex::load_or_rebuild(tmp.path()).unwrap();
        assert_eq!(after.len(), 6, "byte drift forces a rebuild");
        assert_eq!(after.tail_seq(), Some(Seq::new(5)));
    }

    #[test]
    fn a_corrupt_cache_rebuilds_instead_of_reporting() {
        let tmp = tempfile::tempdir().unwrap();
        write_ledger(tmp.path(), 4);
        std::fs::write(tmp.path().join(CACHE_NAME), b"idx v1 garbage\nnot a row\n").unwrap();
        let index = LedgerIndex::load_or_rebuild(tmp.path()).unwrap();
        assert_eq!(index.len(), 4, "a disposable artifact never reports");
    }

    #[test]
    fn a_torn_tail_is_skipped_and_the_intact_prefix_still_indexes() {
        let tmp = tempfile::tempdir().unwrap();
        write_ledger(tmp.path(), 3);
        let path = tmp.path().join("ledger-00000000000000000000.jsonl");
        let mut bytes = std::fs::read(&path).unwrap();
        bytes.extend_from_slice(b"{\"seq\":99,\"partial\"");
        std::fs::write(&path, &bytes).unwrap();
        let index = LedgerIndex::load_or_rebuild(tmp.path()).unwrap();
        assert_eq!(index.len(), 3);
        assert_eq!(index.tail_seq(), Some(Seq::new(2)));
    }

    #[test]
    fn a_missing_seq_is_a_caller_error_not_a_corrupt_ledger() {
        let tmp = tempfile::tempdir().unwrap();
        write_ledger(tmp.path(), 2);
        let index = LedgerIndex::load_or_rebuild(tmp.path()).unwrap();
        let err = match index.line_at(tmp.path(), Seq::new(77)) {
            Err(err) => err,
            Ok(_) => panic!("an absent seq must not read"),
        };
        assert!(matches!(err, MemoryError::SeqMissing { seq: 77 }));
    }
}
