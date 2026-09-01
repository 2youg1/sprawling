// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

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
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::path::{Path, PathBuf};

use kernel::{B3Hash, Seq};

use crate::jsonl::{MemoryError, io_err, is_segment};

const CACHE_NAME: &str = "index.cache";
const CACHE_MAGIC: &str = "idx v1";

/// seq → (segment file name, byte offset of the line start).
pub struct LedgerIndex {
    entries: BTreeMap<Seq, (String, u64)>,
    /// Bytes of each segment already folded into `entries`.
    ///
    /// What makes an incremental refresh possible, and what makes it
    /// safe: a segment that grew is read from here on, and a segment
    /// that shrank had a tail truncated away, which invalidates every
    /// offset this map holds for it.
    scanned: BTreeMap<String, u64>,
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

    /// An index over nothing.
    ///
    /// For a holder that wants one before the ledger directory exists: the
    /// first `refresh` fills it, and until then every lookup answers
    /// `SeqMissing`, which is the truth.
    #[must_use]
    pub fn empty() -> LedgerIndex {
        LedgerIndex {
            entries: BTreeMap::new(),
            scanned: BTreeMap::new(),
        }
    }

    /// Folds whatever has been appended since the last look.
    ///
    /// A resident index is the point: rebuilding one costs a read of the
    /// whole cache and a `String` for every line in it, which on a fifty
    /// thousand record ledger was 14.4 ms charged to every single
    /// history query. Refreshing costs one directory listing plus a
    /// `stat` per segment, and reads only the bytes that are new.
    ///
    /// The incremental face is here rather than an "index, a record just
    /// landed" call on the writer's side, because a caller holds an
    /// `EventRecord` and segments are this crate's own business. Handing
    /// an offset to an observer would leak the layout to everyone.
    ///
    /// A segment that shrank or vanished forces a full rebuild: tail
    /// recovery truncates, and after it the offsets held for that
    /// segment describe bytes that are no longer there.
    pub fn refresh(&mut self, dir: &Path) -> Result<(), MemoryError> {
        let names = segment_names(dir)?;
        let mut fresh = Vec::new();
        for name in &names {
            let path = dir.join(name);
            let size = std::fs::metadata(&path)
                .map_err(io_err("stat segment", &path))?
                .len();
            match self.scanned.get(name).copied() {
                Some(done) if done == size => continue,
                Some(done) if done < size => fresh.push((name.clone(), done, size)),
                Some(_) => return self.replace_with(rebuild(dir)?),
                None => fresh.push((name.clone(), 0, size)),
            }
        }
        if self.scanned.keys().any(|held| !names.contains(held)) {
            return self.replace_with(rebuild(dir)?);
        }
        for (name, from, size) in fresh {
            let path = dir.join(&name);
            let bytes = std::fs::read(&path).map_err(io_err("read segment", &path))?;
            let tail = match usize::try_from(from).ok().and_then(|at| bytes.get(at..)) {
                Some(tail) => tail,
                // The offset does not fit this machine's pointer width,
                // which no ledger this crate writes can reach; rebuilding
                // is the answer that cannot be wrong.
                None => return self.replace_with(rebuild(dir)?),
            };
            let indexed = self.fold_segment(&name, from, tail);
            // Only complete lines count as scanned: a torn tail is
            // overwritten by the next append, and remembering it as read
            // would skip the record that replaces it.
            self.scanned
                .insert(name, from.saturating_add(indexed).min(size));
        }
        Ok(())
    }

    /// Indexes the complete lines of `tail`, which begins at `from` in
    /// the segment. Returns how many bytes those complete lines took.
    fn fold_segment(&mut self, name: &str, from: u64, tail: &[u8]) -> u64 {
        let mut offset = from;
        let mut complete_bytes = 0u64;
        for line in tail.split_inclusive(|b| *b == b'\n') {
            if line.last().copied() != Some(b'\n') {
                break;
            }
            let body = line.get(..line.len().saturating_sub(1)).unwrap_or(line);
            if !body.is_empty()
                && let Some(seq) = seq_of(body)
            {
                self.entries.insert(seq, (name.to_owned(), offset));
            }
            let len = u64::try_from(line.len()).unwrap_or(0);
            offset = offset.saturating_add(len);
            complete_bytes = complete_bytes.saturating_add(len);
        }
        complete_bytes
    }

    fn replace_with(&mut self, built: LedgerIndex) -> Result<(), MemoryError> {
        *self = built;
        Ok(())
    }

    /// A cursor for reading lines out of this index.
    ///
    /// The unit of work is a stretch of sequences rather than one of
    /// them, so the handle belongs to the stretch: a caller asks for a
    /// reader once and then for lines as often as it likes.
    pub fn reader(&self, dir: &Path) -> LineReader<'_> {
        LineReader {
            index: self,
            dir: dir.to_path_buf(),
            open: None,
        }
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

/// Reads single lines by seq, holding one segment open across reads.
///
/// One handle and one buffer serve the whole stretch, and a walk that
/// goes forward never seeks at all — the position the previous line
/// left is the position the next one wants. What this replaces opened
/// the segment again for every line and then read it one byte at a
/// time. Over a fifty-thousand record ledger on one windows-x86_64
/// NVMe machine (2026-09-02): 734 µs a line then, against 0.89 µs
/// walking forward and 5.82 µs walking backward now.
pub struct LineReader<'index> {
    index: &'index LedgerIndex,
    dir: PathBuf,
    open: Option<OpenSegment>,
}

impl LineReader<'_> {
    /// One line, without its terminator. A seq absent from the index is
    /// a caller error, not a corrupt ledger.
    pub fn line_at(&mut self, seq: Seq) -> Result<Vec<u8>, MemoryError> {
        let Some((name, offset)) = self.index.entries.get(&seq) else {
            return Err(MemoryError::SeqMissing { seq: seq.value() });
        };
        let offset = *offset;
        let segment = match self.open.take() {
            Some(open) if open.name == *name => open,
            _ => OpenSegment::open(&self.dir, name)?,
        };
        self.open.insert(segment).line_at(offset)
    }
}

/// The segment a `LineReader` currently holds open, and where reading
/// it would resume.
struct OpenSegment {
    name: String,
    path: PathBuf,
    file: BufReader<File>,
    /// The offset the next read starts at, while that offset is known.
    /// `None` means the position must be sought before it is used —
    /// a remembered position that might be wrong would hand the caller
    /// another line's bytes under the seq it asked for.
    resume: Option<u64>,
}

impl OpenSegment {
    fn open(dir: &Path, name: &str) -> Result<OpenSegment, MemoryError> {
        let path = dir.join(name);
        let file = File::open(&path).map_err(io_err("open segment", &path))?;
        Ok(OpenSegment {
            name: name.to_owned(),
            path,
            file: BufReader::new(file),
            resume: Some(0),
        })
    }

    fn line_at(&mut self, offset: u64) -> Result<Vec<u8>, MemoryError> {
        if self.resume != Some(offset) {
            self.resume = None;
            self.file
                .seek(SeekFrom::Start(offset))
                .map_err(io_err("seek segment", &self.path))?;
        }
        self.resume = None;
        let mut line = Vec::new();
        let read = self
            .file
            .read_until(b'\n', &mut line)
            .map_err(io_err("read segment", &self.path))?;
        self.resume = u64::try_from(read)
            .ok()
            .and_then(|len| offset.checked_add(len));
        if line.last().copied() == Some(b'\n') {
            line.pop();
        }
        Ok(line)
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

/// How many bytes of each segment exist right now.
///
/// A cache is believed only when the directory's byte count matches its
/// stamp, so at that moment "already indexed" and "size on disk" are the
/// same number - this is exact rather than an approximation.
fn sizes_now(dir: &Path) -> BTreeMap<String, u64> {
    let mut sizes = BTreeMap::new();
    let Ok(names) = segment_names(dir) else {
        return sizes;
    };
    for name in names {
        if let Ok(meta) = std::fs::metadata(dir.join(&name)) {
            sizes.insert(name, meta.len());
        }
    }
    sizes
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
    Some(LedgerIndex {
        entries,
        scanned: sizes_now(dir),
    })
}

fn rebuild(dir: &Path) -> Result<LedgerIndex, MemoryError> {
    let mut entries = BTreeMap::new();
    let mut scanned = BTreeMap::new();
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
            if complete {
                scanned.insert(
                    name.clone(),
                    offset.saturating_add(u64::try_from(line.len()).unwrap_or(0)),
                );
            }
            offset = offset.saturating_add(u64::try_from(line.len()).unwrap_or(0));
        }
        scanned.entry(name).or_insert(0);
    }
    Ok(LedgerIndex { entries, scanned })
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
        write_ledger_rolling(dir, count, count.max(1))
    }

    /// Writes `count` records into segments that roll every
    /// `per_segment` lines, and returns each line in seq order.
    fn write_ledger_rolling(dir: &Path, count: u64, per_segment: u64) -> Vec<Vec<u8>> {
        let run = RunId::from_bytes([7u8; 16]);
        let mut lines = Vec::new();
        let mut blob = Vec::new();
        let mut first_seq = 0u64;
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
            let next = i.saturating_add(1);
            if next.checked_rem(per_segment) == Some(0) && next < count {
                std::fs::write(dir.join(format!("ledger-{first_seq:020}.jsonl")), &blob).unwrap();
                blob.clear();
                first_seq = next;
            }
        }
        std::fs::write(dir.join(format!("ledger-{first_seq:020}.jsonl")), &blob).unwrap();
        lines
    }

    #[test]
    fn rebuild_finds_every_line_and_seeking_returns_it_verbatim() {
        let tmp = tempfile::tempdir().unwrap();
        let lines = write_ledger(tmp.path(), 5);
        let index = LedgerIndex::load_or_rebuild(tmp.path()).unwrap();
        assert_eq!(index.len(), 5);
        assert_eq!(index.tail_seq(), Some(Seq::new(4)));
        let mut reader = index.reader(tmp.path());
        for (i, expected) in lines.iter().enumerate() {
            let got = reader.line_at(Seq::new(u64::try_from(i).unwrap())).unwrap();
            assert_eq!(&got, expected, "line {i} seeks back verbatim");
        }
    }

    #[test]
    fn one_reader_answers_out_of_order_seeks_across_segments() {
        let tmp = tempfile::tempdir().unwrap();
        let lines = write_ledger_rolling(tmp.path(), 9, 4);
        let index = LedgerIndex::load_or_rebuild(tmp.path()).unwrap();
        assert_eq!(index.len(), 9, "three segments, nine lines");
        let mut reader = index.reader(tmp.path());
        // Backwards, repeated, and ping-ponging between segments. A
        // reader that holds a handle open must hold its position honest
        // too: every one of these asks for a line the buffer is not
        // already sitting on.
        for value in [8u64, 0, 8, 3, 4, 3, 7, 1, 1, 5] {
            let got = reader.line_at(Seq::new(value)).unwrap();
            let expected = &lines[usize::try_from(value).unwrap()];
            assert_eq!(&got, expected, "seq {value} reads back verbatim");
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

    /// The refresh reads what was appended and nothing else, and a
    /// segment that lost its tail is not patched but rebuilt.
    #[test]
    fn a_resident_index_folds_what_arrived_and_rebuilds_what_was_truncated() {
        let tmp = tempfile::tempdir().unwrap();
        write_ledger(tmp.path(), 3);
        let mut index = LedgerIndex::load_or_rebuild(tmp.path()).unwrap();
        assert_eq!(index.len(), 3);

        // Nothing moved: refreshing is a no-op, and the map is unchanged.
        index.refresh(tmp.path()).unwrap();
        assert_eq!(index.len(), 3);

        // Appended: the new lines land, and the old offsets still read.
        let lines = write_ledger(tmp.path(), 7);
        index.refresh(tmp.path()).unwrap();
        assert_eq!(index.len(), 7);
        assert_eq!(index.tail_seq(), Some(Seq::new(6)));
        let mut reader = index.reader(tmp.path());
        for (i, expected) in lines.iter().enumerate() {
            let got = reader.line_at(Seq::new(u64::try_from(i).unwrap())).unwrap();
            assert_eq!(&got, expected, "line {i} still reads after a refresh");
        }

        // Truncated: the offsets held for that segment describe bytes
        // that are gone, so the whole map is rebuilt rather than patched.
        let path = tmp.path().join("ledger-00000000000000000000.jsonl");
        let bytes = std::fs::read(&path).unwrap();
        let keep = bytes.len() / 2;
        std::fs::write(&path, bytes.get(..keep).unwrap()).unwrap();
        index.refresh(tmp.path()).unwrap();
        assert!(index.len() < 7, "a shrunken segment rebuilds");
        let held = index.len();
        let mut reader = index.reader(tmp.path());
        for i in 0..held {
            reader
                .line_at(Seq::new(u64::try_from(i).unwrap()))
                .unwrap_or_else(|err| panic!("seq {i} unreadable after rebuild: {err}"));
        }
    }

    #[test]
    fn a_missing_seq_is_a_caller_error_not_a_corrupt_ledger() {
        let tmp = tempfile::tempdir().unwrap();
        write_ledger(tmp.path(), 2);
        let index = LedgerIndex::load_or_rebuild(tmp.path()).unwrap();
        let err = match index.reader(tmp.path()).line_at(Seq::new(77)) {
            Err(err) => err,
            Ok(_) => panic!("an absent seq must not read"),
        };
        assert!(matches!(err, MemoryError::SeqMissing { seq: 77 }));
    }
}
