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

use std::collections::{BTreeMap, BTreeSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Seek, SeekFrom};
use std::ops::Bound;
use std::path::{Path, PathBuf};

use kernel::{B3Hash, RunId, Seq};

use crate::error::{MemoryError, io_err};
use crate::jsonl::is_segment;

const CACHE_NAME: &str = "index.cache";
/// Bumped to v2 when the cache started carrying each line's run. An
/// older cache fails this comparison and rebuilds in silence, which is
/// this module's standing answer to any doubt — so no migration code
/// exists, and none is needed.
const CACHE_MAGIC: &str = "idx v2";
/// What a cache row writes where the run belongs when the line named
/// none. Not a valid uuid, so it can never be read back as one.
const NO_RUN: &str = "-";

/// seq → (segment file name, byte offset of the line start).
pub struct LedgerIndex {
    entries: BTreeMap<Seq, (String, u64)>,
    /// run → the sequences it wrote.
    ///
    /// The same map read the other way round, and the reason one
    /// session's history costs its own length rather than the ledger's:
    /// without it, answering "the newest twenty lines of this session"
    /// meant reading every line back to wherever the answer ran out and
    /// discarding what belonged to somebody else.
    runs: BTreeMap<RunId, BTreeSet<Seq>>,
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
            runs: BTreeMap::new(),
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
            if !body.is_empty() {
                self.insert_line(name, offset, body);
            }
            let len = u64::try_from(line.len()).unwrap_or(0);
            offset = offset.saturating_add(len);
            complete_bytes = complete_bytes.saturating_add(len);
        }
        complete_bytes
    }

    /// What indexing one line means, in one place: where it sits, and
    /// whose it is. A line that does not parse is skipped rather than
    /// reported — the caller is looking at a ledger that may be damaged,
    /// and an index is how such a ledger gets repaired.
    fn insert_line(&mut self, name: &str, offset: u64, body: &[u8]) {
        let Some(located) = locate(body) else {
            return;
        };
        self.entries.insert(located.seq, (name.to_owned(), offset));
        if let Some(run) = located.run {
            self.runs.entry(run).or_default().insert(located.seq);
        }
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

    /// The sequences one run wrote, newest first, below `before`.
    ///
    /// Newest first because what a reader opens a session for is its end;
    /// a caller that wants them oldest first takes what it needs and
    /// reverses a list it already holds, which also lets it read the
    /// segment forwards.
    ///
    /// `before` is exclusive, the same word and the same meaning the wire
    /// gives `HistoryAnswer::earlier`, so paging by handing one answer's
    /// cursor to the next question can neither repeat nor skip a record.
    ///
    /// A run this index never saw yields nothing, which is the truth and
    /// needs no separate answer.
    pub fn run_seqs_before(&self, run: RunId, before: Option<Seq>) -> impl Iterator<Item = Seq> {
        let upper = match before {
            Some(seq) => Bound::Excluded(seq),
            None => Bound::Unbounded,
        };
        self.runs
            .get(&run)
            .into_iter()
            .flat_map(move |seqs| seqs.range((Bound::Unbounded, upper)).rev().copied())
    }

    /// Writes the cache. Failure is not fatal — the index rebuilds next
    /// time, and a disposable artifact must never block the main path.
    pub fn persist(&self, dir: &Path) -> Result<(), MemoryError> {
        let stamp = directory_stamp(dir)?;
        // The run map inverted for the length of this write. Held here
        // rather than resident because a cache row is the only reader of
        // "which run owns this seq", and a second resident copy would be
        // a second thing to keep in step.
        let mut owner: BTreeMap<Seq, RunId> = BTreeMap::new();
        for (run, seqs) in &self.runs {
            for seq in seqs {
                owner.insert(*seq, *run);
            }
        }
        let mut out = format!("{CACHE_MAGIC} {} {}\n", stamp.bytes, stamp.digest);
        for (seq, (segment, offset)) in &self.entries {
            let run = match owner.get(seq) {
                Some(run) => run.to_string(),
                None => NO_RUN.to_owned(),
            };
            out.push_str(&format!("{} {segment} {offset} {run}\n", seq.value()));
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
    let mut runs: BTreeMap<RunId, BTreeSet<Seq>> = BTreeMap::new();
    for line in lines {
        let mut parts = line.split(' ');
        let seq = Seq::new(parts.next()?.parse::<u64>().ok()?);
        let segment = parts.next()?.to_owned();
        let offset = parts.next()?.parse::<u64>().ok()?;
        // A row that names no run is how the writer spells a line whose
        // own run was unreadable; anything else that will not parse as a
        // run id makes the whole cache suspect, so the caller rebuilds.
        let owner = match parts.next()? {
            NO_RUN => None,
            raw => Some(RunId::parse(raw).ok()?),
        };
        if parts.next().is_some() {
            return None;
        }
        entries.insert(seq, (segment, offset));
        if let Some(run) = owner {
            runs.entry(run).or_default().insert(seq);
        }
    }
    Some(LedgerIndex {
        entries,
        runs,
        scanned: sizes_now(dir),
    })
}

fn rebuild(dir: &Path) -> Result<LedgerIndex, MemoryError> {
    let mut index = LedgerIndex::empty();
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
            if complete && !body.is_empty() {
                index.insert_line(&name, offset, body);
            }
            if complete {
                index.scanned.insert(
                    name.clone(),
                    offset.saturating_add(u64::try_from(line.len()).unwrap_or(0)),
                );
            }
            offset = offset.saturating_add(u64::try_from(line.len()).unwrap_or(0));
        }
        index.scanned.entry(name).or_insert(0);
    }
    Ok(index)
}

/// Where a line sits and whose it is.
struct Located {
    seq: Seq,
    /// `None` when the line names no run that reads back as one. The
    /// line is still indexed by seq: an index over a damaged ledger is
    /// exactly what a repair path needs, and a line dropped here would
    /// be invisible to every reader.
    run: Option<RunId>,
}

/// Reads two fields off one parse. Indexing must not depend on the
/// record parsing cleanly as an `EventRecord`, so this asks the raw
/// document rather than the typed one.
fn locate(line: &[u8]) -> Option<Located> {
    let value: serde_json::Value = serde_json::from_slice(line).ok()?;
    let seq = Seq::new(value.get("seq")?.as_u64()?);
    let run = value
        .get("run")
        .and_then(serde_json::Value::as_str)
        .and_then(|raw| RunId::parse(raw).ok());
    Some(Located { seq, run })
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
        std::fs::write(
            tmp.path().join(CACHE_NAME),
            format!("{CACHE_MAGIC} garbage\nnot a row\n"),
        )
        .unwrap();
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

    /// Writes `count` records that cycle through `runs`, so no run owns a
    /// contiguous stretch of the ledger. Returns the seqs each run wrote.
    fn write_interleaved(dir: &Path, runs: &[RunId], count: u64) -> Vec<(RunId, Vec<Seq>)> {
        let mut blob = Vec::new();
        let mut prev = B3Hash::digest(b"");
        let mut owned: Vec<(RunId, Vec<Seq>)> = runs.iter().map(|run| (*run, Vec::new())).collect();
        for i in 0..count {
            let slot = usize::try_from(i)
                .unwrap()
                .checked_rem(runs.len())
                .expect("at least one run to cycle through");
            let run = runs[slot];
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
            owned[slot].1.push(Seq::new(i));
        }
        std::fs::write(dir.join("ledger-00000000000000000000.jsonl"), &blob).unwrap();
        owned
    }

    /// The whole point of the run index: one session's lines are found
    /// without reading anybody else's. The assertion is on the seqs the
    /// index hands back, because that set *is* what the reader will open
    /// the segment for - a walk that returned the other runs' seqs and
    /// left the filtering to the caller would have read them all.
    #[test]
    fn a_run_s_own_lines_are_found_without_walking_anybody_else_s() {
        let tmp = tempfile::tempdir().unwrap();
        let mine = RunId::from_bytes([1u8; 16]);
        let yours = RunId::from_bytes([2u8; 16]);
        let city = RunId::CITY;
        let owned = write_interleaved(tmp.path(), &[mine, yours, city], 30);
        let index = LedgerIndex::load_or_rebuild(tmp.path()).unwrap();

        let all: Vec<Seq> = index.run_seqs_before(mine, None).collect();
        let mut expected = owned[0].1.clone();
        expected.reverse();
        assert_eq!(all, expected, "newest first, and only this run's lines");

        // Bounded by the answer rather than by the ledger: taking two
        // reads two, however many lines the other runs wrote between.
        let newest_two: Vec<Seq> = index.run_seqs_before(mine, None).take(2).collect();
        assert_eq!(newest_two, [Seq::new(27), Seq::new(24)]);

        // `before` is exclusive, which is what the wire's `earlier`
        // cursor already means; paging with it never repeats a record.
        let older: Vec<Seq> = index
            .run_seqs_before(mine, Some(Seq::new(24)))
            .take(2)
            .collect();
        assert_eq!(older, [Seq::new(21), Seq::new(18)]);

        // A session this ledger never held has no lines, which is a
        // different answer from an empty ledger and needs no special case.
        let stranger: Vec<Seq> = index
            .run_seqs_before(RunId::from_bytes([9u8; 16]), None)
            .collect();
        assert!(stranger.is_empty());

        // The city's own records are a run like any other here.
        assert_eq!(index.run_seqs_before(city, None).count(), 10);
    }

    /// The run map rides the same refresh and the same cache as the seq
    /// map. Either one going stale on its own would make "which lines are
    /// this session's" disagree with "where is line n".
    #[test]
    fn the_run_map_survives_a_refresh_and_a_cache_round_trip() {
        let tmp = tempfile::tempdir().unwrap();
        let mine = RunId::from_bytes([1u8; 16]);
        let yours = RunId::from_bytes([2u8; 16]);
        write_interleaved(tmp.path(), &[mine, yours], 4);
        let mut index = LedgerIndex::load_or_rebuild(tmp.path()).unwrap();
        assert_eq!(index.run_seqs_before(mine, None).count(), 2);

        // Appended: the refresh folds the new lines into both maps.
        write_interleaved(tmp.path(), &[mine, yours], 10);
        index.refresh(tmp.path()).unwrap();
        assert_eq!(index.run_seqs_before(mine, None).count(), 5);

        // Persisted and read back: a cache that carried offsets but not
        // runs would answer this with nothing.
        index.persist(tmp.path()).unwrap();
        let loaded = LedgerIndex::load_or_rebuild(tmp.path()).unwrap();
        assert_eq!(loaded.len(), 10, "the cache was believed, not rebuilt");
        assert_eq!(loaded.run_seqs_before(mine, None).count(), 5);
    }

    /// A line whose `run` cannot be read still belongs in the seq map:
    /// an index over a damaged ledger is what a repair path needs, and
    /// dropping the line would hide it from every reader.
    #[test]
    fn a_line_with_no_readable_run_is_still_indexed_by_seq() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("ledger-00000000000000000000.jsonl");
        std::fs::write(&path, b"{\"seq\":0,\"run\":\"not-a-uuid\"}\n{\"seq\":1}\n").unwrap();
        let index = LedgerIndex::load_or_rebuild(tmp.path()).unwrap();
        assert_eq!(index.len(), 2, "both lines are locatable");
        assert_eq!(index.tail_seq(), Some(Seq::new(1)));
        assert_eq!(
            index.run_seqs_before(RunId::CITY, None).count(),
            0,
            "an unreadable run joins no run rather than joining the nil one"
        );
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
