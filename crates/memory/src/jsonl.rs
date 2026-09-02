// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Durable Ledger adapter: jsonl segments, tail-truncation recovery,
//! direction-aware version refusal, group commit.
//!
//! Contract owned here:
//! - bytes come from `EventRecord::canonical_line` verbatim plus `\n`;
//!   this module owns seq/prev assignment and the fsync schedule, nothing
//!   about the byte shape.
//! - `append_all` is one durability barrier per wave: `Ok` means every
//!   line is written and synced; on `Err` the in-memory state has not
//!   advanced and torn bytes are the next open's tail recovery.
//! - open never rewrites while browsing: the version probe fires before
//!   any repair; only the last segment is ever truncated, and only at a
//!   torn boundary.
//! - time is a parameter (`now` feeds the `log_truncated` record); this
//!   module never samples a clock (determinism rule 2).
//!
//! The `Vfs` seam this reaches disk through is crate-internal on
//! purpose (`crate::vfs`): std fs and FaultFs are its two adapters, and
//! it never enters a public signature — `JsonlLedger` hides it behind
//! `Box<dyn Vfs>`.

use std::path::{Path, PathBuf};

use kernel::{
    AxCode, AxError, B3Hash, EventDraft, EventKind, EventRecord, EventRef, GENESIS_PREV, Payload,
    RunId, Seq, TimeMs, chain_hash,
};

use crate::error::{MemoryError, io_err};
use crate::real_fs::RealFs;
use crate::vfs::Vfs;

/// Segment rolling threshold. Internal affair: changing it changes how
/// files are cut, never any observable semantics (memory-SPEC 14).
pub(crate) const SEGMENT_ROLL_BYTES: u64 = 64 * 1024 * 1024;

/// What open found and repaired.
pub struct OpenReport {
    pub recovered: Option<TailTruncation>,
}

pub struct TailTruncation {
    pub dropped_bytes: u64,
}

/// The durable Ledger. See the module doc for the contract.
pub struct JsonlLedger {
    vfs: Box<dyn Vfs>,
    dir: PathBuf,
    seg_path: PathBuf,
    seg_len: u64,
    next_seq: Seq,
    prev: B3Hash,
    roll_bytes: u64,
    /// The write-path observer, called only after the wave is durable.
    observer: Option<WriteObserver>,
}

/// The write-path observer: what a live reader (the control surface's
/// event stream) installs to hear each line once it is durable.
pub type WriteObserver = Box<dyn FnMut(&EventRecord) + Send>;

/// Chain state at the entrance of the last segment.
struct TailBoundary {
    prev: B3Hash,
    next_seq: Seq,
    prior: Option<PriorSegment>,
}

struct PriorSegment {
    path: PathBuf,
    len: u64,
}

fn segment_file_name(first_seq: Seq) -> String {
    format!("ledger-{:020}.jsonl", first_seq.value())
}

pub(crate) fn is_segment(path: &Path) -> bool {
    match path.file_name().and_then(|n| n.to_str()) {
        Some(name) => name.starts_with("ledger-") && name.ends_with(".jsonl"),
        None => false,
    }
}

/// Complete (`\n`-terminated) lines and the leftover tail bytes.
fn complete_lines(bytes: &[u8]) -> (Vec<&[u8]>, usize) {
    let mut lines = Vec::new();
    let mut consumed = 0usize;
    let mut start = 0usize;
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'\n' {
            if let Some(line) = bytes.get(start..index) {
                lines.push(line);
            }
            start = index.saturating_add(1);
            consumed = start;
        }
    }
    (lines, consumed)
}

impl JsonlLedger {
    /// Production entrance: std filesystem underneath.
    pub fn open(dir: &Path, now: TimeMs) -> Result<(Self, OpenReport), MemoryError> {
        JsonlLedger::open_with(Box::new(RealFs::new()), dir, now)
    }

    /// The `fault` entrance: the same ledger, over the deterministic
    /// power-loss model, for a caller that needs one named write to fail.
    ///
    /// Takes the concrete adapter rather than the trait, so the `Vfs`
    /// seam stays inner (memory-SPEC 8-1) and no `pub trait` leaves this
    /// crate. What comes back is the same `JsonlLedger` production uses -
    /// a caller above this crate exercises its real code and loses only
    /// the write it named.
    ///
    /// # Errors
    /// Propagates whatever [`JsonlLedger::open`] would, and the power
    /// loss itself when the plan cuts during the open.
    #[cfg(any(test, feature = "fault"))]
    pub fn open_faulty(
        fs: crate::fault_fs::FaultFs,
        dir: &Path,
        now: TimeMs,
    ) -> Result<(Self, OpenReport), MemoryError> {
        JsonlLedger::open_with(Box::new(fs), dir, now)
    }

    /// Injection point for the second Vfs adapter; [`JsonlLedger::open`]
    /// and `open_faulty` are its two entrances.
    pub(crate) fn open_with(
        mut vfs: Box<dyn Vfs>,
        dir: &Path,
        now: TimeMs,
    ) -> Result<(Self, OpenReport), MemoryError> {
        vfs.create_dir_all(dir)
            .map_err(io_err("create ledger dir", dir))?;
        let segments: Vec<PathBuf> = vfs
            .list(dir)
            .map_err(io_err("list ledger dir", dir))?
            .into_iter()
            .filter(|p| is_segment(p))
            .collect();

        let mut ledger = JsonlLedger {
            vfs,
            dir: dir.to_path_buf(),
            seg_path: dir.join(segment_file_name(Seq::FIRST)),
            seg_len: 0,
            next_seq: Seq::FIRST,
            prev: GENESIS_PREV,
            roll_bytes: SEGMENT_ROLL_BYTES,
            observer: None,
        };

        let Some(last) = segments.last().cloned() else {
            return Ok((ledger, OpenReport { recovered: None }));
        };

        ledger.probe_version(&segments)?;
        let dropped = ledger.recover_tail(&segments, &last)?;
        let report = if dropped > 0 {
            if ledger.next_seq != Seq::FIRST {
                ledger.append_log_truncated(now, dropped)?;
            }
            OpenReport {
                recovered: Some(TailTruncation {
                    dropped_bytes: dropped,
                }),
            }
        } else {
            OpenReport { recovered: None }
        };
        Ok((ledger, report))
    }

    /// Direction-aware version refusal, before any repair or parse
    /// (never a partial read of a newer ledger).
    fn probe_version(&mut self, segments: &[PathBuf]) -> Result<(), MemoryError> {
        let Some(first) = segments.first() else {
            return Ok(());
        };
        let bytes = self
            .vfs
            .read(first)
            .map_err(io_err("read segment", first))?;
        let (lines, _) = complete_lines(&bytes);
        let Some(first_line) = lines.first() else {
            // Empty or torn-before-first-line segment: version unknowable;
            // tail recovery decides what remains.
            return Ok(());
        };
        // A mangled first line in a single-segment ledger is tail damage:
        // it carries no version information, and tail recovery owns it.
        // With more segments behind it the same damage is non-tail and
        // must refuse instead (memory-SPEC 8-1).
        let probed = serde_json::from_slice::<serde_json::Value>(first_line)
            .ok()
            .and_then(|value| value.get("v").and_then(serde_json::Value::as_u64));
        let v = match probed {
            Some(v) => v,
            None if segments.len() > 1 => {
                return Err(MemoryError::Envelope {
                    path: first.clone(),
                    line: 1,
                    source: AxError::failure(
                        AxCode::InvalidArgs,
                        "probe ledger version",
                        "first line is not a version-bearing record",
                    ),
                });
            }
            None => return Ok(()),
        };
        let current = u64::from(kernel::consts_external::EVENT_LOG_V);
        if v > current {
            return Err(MemoryError::VersionAhead {
                path: first.clone(),
                v,
            });
        }
        if v < 1 {
            return Err(MemoryError::Envelope {
                path: first.clone(),
                line: 1,
                source: AxError::failure(AxCode::InvalidArgs, "probe ledger version", "v < 1"),
            });
        }
        Ok(())
    }

    /// Boundary state entering the last segment: chain root, or the
    /// previous segment's verified last line.
    fn boundary(&mut self, segments: &[PathBuf], last: &Path) -> Result<TailBoundary, MemoryError> {
        let mut prior: Option<&PathBuf> = None;
        for seg in segments {
            if seg.as_path() == last {
                break;
            }
            prior = Some(seg);
        }
        let Some(prior) = prior else {
            return Ok(TailBoundary {
                prev: GENESIS_PREV,
                next_seq: Seq::FIRST,
                prior: None,
            });
        };
        let bytes = self
            .vfs
            .read(prior)
            .map_err(io_err("read segment", prior))?;
        let (lines, _) = complete_lines(&bytes);
        let count = u64::try_from(lines.len()).unwrap_or(u64::MAX);
        let Some(last_line) = lines.last() else {
            return Err(MemoryError::Envelope {
                path: prior.clone(),
                line: 0,
                source: AxError::failure(AxCode::InvalidArgs, "read prior segment", "empty"),
            });
        };
        let record =
            EventRecord::parse_line(last_line).map_err(|source| MemoryError::Envelope {
                path: prior.clone(),
                line: count,
                source,
            })?;
        let next = record
            .seq()
            .next()
            .map_err(|source| MemoryError::Draft { source })?;
        let len = u64::try_from(bytes.len()).unwrap_or(u64::MAX);
        Ok(TailBoundary {
            prev: chain_hash(last_line),
            next_seq: next,
            prior: Some(PriorSegment {
                path: prior.clone(),
                len,
            }),
        })
    }

    /// Tail-truncation recovery over the last segment. Returns dropped
    /// bytes; on return the ledger state points at the surviving tail.
    fn recover_tail(&mut self, segments: &[PathBuf], last: &Path) -> Result<u64, MemoryError> {
        let boundary = self.boundary(segments, last)?;
        let mut run_prev = boundary.prev;
        let mut run_seq = boundary.next_seq;
        let prior = boundary.prior;
        let bytes = self.vfs.read(last).map_err(io_err("read segment", last))?;
        let (lines, _terminated_len) = complete_lines(&bytes);

        let mut valid_len = 0usize;
        for (index, line) in lines.iter().enumerate() {
            let parsed = EventRecord::parse_line(line);
            let ok = match &parsed {
                Ok(record) => {
                    record.prev() == run_prev
                        && record.seq() == run_seq
                        && record.v() == kernel::consts_external::EVENT_LOG_V
                }
                Err(_) => false,
            };
            if !ok {
                // A tear only ever damages the tail. A bad first line is
                // refused — not silently discarded — when it is parseable
                // with a wrong chain root (foreign or damaged history) or
                // when intact records still follow it (non-tail damage).
                // Newline-bearing garbage does not count as a record.
                if index == 0 {
                    let later_intact = lines
                        .iter()
                        .skip(1)
                        .any(|l| EventRecord::parse_line(l).is_ok());
                    if parsed.is_ok() || later_intact {
                        return Err(MemoryError::Envelope {
                            path: last.to_path_buf(),
                            line: 1,
                            source: AxError::failure(
                                AxCode::InvalidArgs,
                                "verify chain root",
                                "first line does not continue the chain",
                            ),
                        });
                    }
                }
                break;
            }
            run_prev = chain_hash(line);
            run_seq = run_seq
                .next()
                .map_err(|source| MemoryError::Draft { source })?;
            valid_len = valid_len.saturating_add(line.len()).saturating_add(1);
        }

        let total = bytes.len();
        let dropped = u64::try_from(total.saturating_sub(valid_len)).unwrap_or(u64::MAX);

        if dropped > 0 {
            if valid_len == 0 {
                // Whole last segment is torn. Remove it; the tail falls
                // back to the prior segment (or to an empty city when this
                // was the only one — the genesis append never returned Ok).
                self.vfs
                    .truncate(last, 0)
                    .map_err(io_err("truncate segment", last))?;
                self.vfs
                    .sync_data(last)
                    .map_err(io_err("sync segment", last))?;
                self.vfs
                    .remove_file(last)
                    .map_err(io_err("remove empty segment", last))?;
                self.vfs
                    .sync_dir(&self.dir.clone())
                    .map_err(io_err("sync ledger dir", &self.dir.clone()))?;
                match prior {
                    Some(segment) => {
                        self.seg_path = segment.path;
                        self.seg_len = segment.len;
                    }
                    None => {
                        self.seg_path = self.dir.join(segment_file_name(Seq::FIRST));
                        self.seg_len = 0;
                    }
                }
            } else {
                let keep = u64::try_from(valid_len).unwrap_or(u64::MAX);
                self.vfs
                    .truncate(last, keep)
                    .map_err(io_err("truncate segment", last))?;
                self.vfs
                    .sync_data(last)
                    .map_err(io_err("sync segment", last))?;
                self.seg_path = last.to_path_buf();
                self.seg_len = keep;
            }
        } else {
            self.seg_path = last.to_path_buf();
            self.seg_len = u64::try_from(total).unwrap_or(u64::MAX);
        }
        self.prev = run_prev;
        self.next_seq = run_seq;
        Ok(dropped)
    }

    fn append_log_truncated(&mut self, now: TimeMs, dropped: u64) -> Result<(), MemoryError> {
        let mut map = serde_json::Map::new();
        map.insert("dropped_bytes".to_owned(), serde_json::Value::from(dropped));
        let data = Payload::new(map).map_err(|source| MemoryError::Draft { source })?;
        let draft = EventDraft {
            run: RunId::CITY,
            t: now,
            who: "system".to_owned(),
            addr: None,
            kind: EventKind::LogTruncated,
            data,
            ig: false,
        };
        self.append_all(vec![draft]).map(|_| ())
    }

    /// Group commit: one durability barrier for the whole wave
    /// (memory-SPEC 3-1: the batch is what the wave delivered).
    pub fn append_all(&mut self, drafts: Vec<EventDraft>) -> Result<Vec<EventRef>, MemoryError> {
        if drafts.is_empty() {
            return Ok(Vec::new());
        }
        let mut seq = self.next_seq;
        let mut prev = self.prev;
        let mut cur_path = self.seg_path.clone();
        let mut cur_len = self.seg_len;
        let mut writes: Vec<(PathBuf, Vec<u8>)> = Vec::new();
        let mut created: Vec<PathBuf> = Vec::new();
        let mut records = Vec::with_capacity(drafts.len());

        if cur_len == 0 && !self.vfs.exists(&cur_path) {
            created.push(cur_path.clone());
        }

        for draft in drafts {
            let record = EventRecord::from_draft(draft, seq, prev);
            let line = record
                .canonical_line()
                .map_err(|source| MemoryError::Draft { source })?;
            let line_len = u64::try_from(line.len())
                .unwrap_or(u64::MAX)
                .saturating_add(1);
            if cur_len > 0 && cur_len.saturating_add(line_len) > self.roll_bytes {
                cur_path = self.dir.join(segment_file_name(seq));
                cur_len = 0;
                created.push(cur_path.clone());
            }
            prev = chain_hash(&line);
            seq = seq.next().map_err(|source| MemoryError::Draft { source })?;
            records.push(record);
            let mut terminated = line;
            terminated.push(b'\n');
            match writes.last_mut() {
                Some((path, buffer)) if *path == cur_path => buffer.extend_from_slice(&terminated),
                _ => writes.push((cur_path.clone(), terminated)),
            }
            cur_len = cur_len.saturating_add(line_len);
        }

        for (path, bytes) in &writes {
            self.vfs
                .append(path, bytes)
                .map_err(io_err("append event line", path))?;
        }
        for (path, _) in &writes {
            self.vfs
                .sync_data(path)
                .map_err(io_err("sync segment", path))?;
        }
        if !created.is_empty() {
            self.vfs
                .sync_dir(&self.dir.clone())
                .map_err(io_err("sync ledger dir", &self.dir.clone()))?;
        }

        self.seg_path = cur_path;
        self.seg_len = cur_len;
        self.next_seq = seq;
        self.prev = prev;

        // Only now, with the bytes synced, does anyone else hear about
        // them: an observer that saw an event the disk never got would be
        // telling the interface something the history does not contain.
        if let Some(observer) = self.observer.as_mut() {
            for record in &records {
                observer(record);
            }
        }
        Ok(records.iter().map(EventRecord::to_ref).collect())
    }

    /// The position a record written now would take.
    ///
    /// A position, never content: this is what anchors a diagnostic log
    /// line to the only history (`docs/logging.md` section 4), and a
    /// reader accessor that returned records would invite decision logic
    /// to consult the ledger it is in the middle of writing.
    #[must_use]
    pub fn position(&self) -> Seq {
        self.next_seq
    }

    /// Installs the write-path observer, replacing any earlier one. The
    /// sink runs on the appending thread after durability, so it must not
    /// block; a bounded send that drops on lag is the shape this expects.
    pub fn observe(&mut self, sink: WriteObserver) {
        self.observer = Some(sink);
    }

    /// The read face for replay, fixtures and inspection: every canonical
    /// line (no terminators), segments flattened in order.
    pub fn read_raw_lines(&self) -> Result<Vec<Vec<u8>>, MemoryError> {
        let mut out = Vec::new();
        let segments: Vec<PathBuf> = self
            .vfs
            .list(&self.dir)
            .map_err(io_err("list ledger dir", &self.dir))?
            .into_iter()
            .filter(|p| is_segment(p))
            .collect();
        for seg in segments {
            let bytes = self.vfs.read(&seg).map_err(io_err("read segment", &seg))?;
            let (lines, _) = complete_lines(&bytes);
            for line in lines {
                if !line.is_empty() {
                    out.push(line.to_vec());
                }
            }
        }
        Ok(out)
    }

    #[cfg(test)]
    pub(crate) fn set_roll_bytes_for_test(&mut self, roll_bytes: u64) {
        self.roll_bytes = roll_bytes;
    }
}

/// Read-only face for replay and fixtures: never opens the ledger, never
/// repairs, never writes — replay must not mutate what it verifies
/// (runtime-SPEC 8-1). Complete lines only; a torn tail byte-run is not a
/// line and is left for `open` to judge.
pub fn read_raw_lines_at(dir: &Path) -> Result<Vec<Vec<u8>>, MemoryError> {
    let vfs = RealFs::new();
    let mut out = Vec::new();
    for seg in ledger_segments_at(dir)? {
        let bytes = vfs.read(&seg).map_err(io_err("read segment", &seg))?;
        let (lines, _) = complete_lines(&bytes);
        for line in lines {
            if !line.is_empty() {
                out.push(line.to_vec());
            }
        }
    }
    Ok(out)
}

/// The ledger segments in `dir`, in the order they must be read.
///
/// An empty result means the directory holds no ledger at all, which is a
/// different fact from a ledger that holds no events, and only this face
/// can tell them apart: `read_raw_lines_at` answers `Ok([])` to both. The
/// caller that has to tell them apart is the one that took the path from
/// a person - `sprawling replay` - and it asks here so that the segment
/// naming rule is never spelled a second time somewhere else.
pub fn ledger_segments_at(dir: &Path) -> Result<Vec<PathBuf>, MemoryError> {
    let vfs = RealFs::new();
    Ok(vfs
        .list(dir)
        .map_err(io_err("list ledger dir", dir))?
        .into_iter()
        .filter(|p| is_segment(p))
        .collect())
}

impl kernel::Ledger for JsonlLedger {
    fn append(&mut self, draft: EventDraft) -> Result<EventRef, AxError> {
        let refs = self.append_all(vec![draft]).map_err(MemoryError::into_ax)?;
        refs.into_iter()
            .next()
            .ok_or_else(|| AxError::failure(AxCode::InvalidArgs, "append event", "empty wave echo"))
    }
}

#[cfg(feature = "conformance")]
impl kernel::conformance::LedgerInspect for JsonlLedger {
    fn raw_lines(&self) -> Result<Vec<Vec<u8>>, AxError> {
        self.read_raw_lines().map_err(MemoryError::into_ax)
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

    fn draft(kind: EventKind, t: u64) -> EventDraft {
        EventDraft {
            run: RunId::CITY,
            t: TimeMs::new(t),
            who: "city".to_string(),
            addr: None,
            kind,
            data: Payload::empty(),
            ig: false,
        }
    }

    fn verify_chain(lines: &[Vec<u8>]) {
        let mut prev = GENESIS_PREV;
        for (i, line) in lines.iter().enumerate() {
            let record = EventRecord::parse_line(line).unwrap();
            assert_eq!(record.prev(), prev, "prev broken at line {i}");
            assert_eq!(record.seq(), Seq::new(u64::try_from(i).unwrap()));
            prev = chain_hash(line);
        }
    }

    #[test]
    fn empty_dir_opens_as_new_ledger_and_appends() {
        let dir = tempfile::tempdir().unwrap();
        let (mut ledger, report) = JsonlLedger::open(dir.path(), TimeMs::new(0)).unwrap();
        assert!(report.recovered.is_none());
        let refs = ledger
            .append_all(vec![
                draft(EventKind::CityInitialized, 1),
                draft(EventKind::BuildingCreated, 2),
            ])
            .unwrap();
        assert_eq!(refs.len(), 2);
        assert_eq!(refs[0].seq(), Seq::new(0));
        assert_eq!(refs[1].seq(), Seq::new(1));
        let lines = ledger.read_raw_lines().unwrap();
        assert_eq!(lines.len(), 2);
        verify_chain(&lines);

        drop(ledger);
        let (mut reopened, report) = JsonlLedger::open(dir.path(), TimeMs::new(9)).unwrap();
        assert!(report.recovered.is_none(), "clean tail must not truncate");
        reopened
            .append_all(vec![draft(EventKind::RunStarted, 3)])
            .unwrap();
        let lines = reopened.read_raw_lines().unwrap();
        assert_eq!(lines.len(), 3);
        verify_chain(&lines);
    }

    #[cfg(feature = "conformance")]
    #[test]
    fn passes_the_kernel_conformance_suite() {
        use kernel::conformance::assert_ledger_conformance;
        let keep: std::cell::RefCell<Vec<tempfile::TempDir>> = std::cell::RefCell::new(Vec::new());
        assert_ledger_conformance(|| {
            let dir = tempfile::tempdir().unwrap();
            let (ledger, _) = JsonlLedger::open(dir.path(), TimeMs::new(0)).unwrap();
            keep.borrow_mut().push(dir);
            ledger
        });
    }

    #[test]
    fn rolls_segments_without_breaking_seq_or_chain() {
        let dir = tempfile::tempdir().unwrap();
        let (mut ledger, _) = JsonlLedger::open(dir.path(), TimeMs::new(0)).unwrap();
        ledger.set_roll_bytes_for_test(1);
        for t in 0..4 {
            ledger
                .append_all(vec![draft(EventKind::GateChecked, t)])
                .unwrap();
        }
        let segments = fs::read_dir(dir.path()).unwrap().count();
        assert!(segments >= 4, "tiny roll budget must produce many segments");
        let lines = ledger.read_raw_lines().unwrap();
        assert_eq!(lines.len(), 4);
        verify_chain(&lines);

        drop(ledger);
        let (mut reopened, report) = JsonlLedger::open(dir.path(), TimeMs::new(9)).unwrap();
        assert!(report.recovered.is_none());
        reopened
            .append_all(vec![draft(EventKind::RunFrozen, 9)])
            .unwrap();
        verify_chain(&reopened.read_raw_lines().unwrap());
    }

    #[test]
    fn torn_tail_recovers_to_longest_valid_prefix_plus_log_truncated() {
        let dir = tempfile::tempdir().unwrap();
        let (mut ledger, _) = JsonlLedger::open(dir.path(), TimeMs::new(0)).unwrap();
        ledger
            .append_all(vec![
                draft(EventKind::CityInitialized, 1),
                draft(EventKind::BuildingCreated, 2),
                draft(EventKind::RunStarted, 3),
            ])
            .unwrap();
        let seg = only_segment(dir.path());
        drop(ledger);

        // Tear: cut the last line in half.
        let bytes = fs::read(&seg).unwrap();
        let cut = bytes.len() - 17;
        fs::write(&seg, &bytes[..cut]).unwrap();

        let (reopened, report) = JsonlLedger::open(dir.path(), TimeMs::new(77)).unwrap();
        let recovery = report.recovered.expect("must report the truncation");
        assert!(recovery.dropped_bytes > 0);
        let lines = reopened.read_raw_lines().unwrap();
        assert_eq!(lines.len(), 3, "two survivors plus log_truncated");
        verify_chain(&lines);
        let last = EventRecord::parse_line(&lines[2]).unwrap();
        assert_eq!(last.kind(), EventKind::LogTruncated);
        assert_eq!(last.t(), TimeMs::new(77), "time is the caller's parameter");
        assert_eq!(last.who(), "system");
    }

    #[test]
    fn garbage_tail_without_newline_is_dropped() {
        let dir = tempfile::tempdir().unwrap();
        let (mut ledger, _) = JsonlLedger::open(dir.path(), TimeMs::new(0)).unwrap();
        ledger
            .append_all(vec![draft(EventKind::CityInitialized, 1)])
            .unwrap();
        let seg = only_segment(dir.path());
        drop(ledger);
        let mut bytes = fs::read(&seg).unwrap();
        bytes.extend_from_slice(b"{half of a torn write");
        fs::write(&seg, &bytes).unwrap();

        let (reopened, report) = JsonlLedger::open(dir.path(), TimeMs::new(5)).unwrap();
        assert!(report.recovered.is_some());
        let lines = reopened.read_raw_lines().unwrap();
        assert_eq!(lines.len(), 2);
        verify_chain(&lines);
    }

    #[test]
    fn higher_version_fixture_is_refused_with_direction_and_path() {
        let fixture = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("..")
            .join("fixtures")
            .join("ledger-v2");
        let before = fs::read(fixture.join("ledger-00000000000000000000.jsonl")).unwrap();

        let outcome = JsonlLedger::open(&fixture, TimeMs::new(0));
        let err = outcome.err().expect("v2 fixture must refuse to open");
        match &err {
            MemoryError::VersionAhead { path, v } => {
                assert_eq!(*v, 2);
                assert!(path.to_string_lossy().contains("ledger-v2"));
            }
            other => panic!("expected VersionAhead, got {other:?}"),
        }
        let ax = err.into_ax();
        assert_eq!(ax.code(), &AxCode::LogVersionUnsupported);
        assert!(ax.to_string().contains("newer"), "direction must be spoken");

        let after = fs::read(fixture.join("ledger-00000000000000000000.jsonl")).unwrap();
        assert_eq!(before, after, "browsing must never rewrite (A16)");
    }

    fn only_segment(dir: &Path) -> std::path::PathBuf {
        let mut files: Vec<_> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect();
        files.sort();
        assert_eq!(files.len(), 1);
        files.remove(0)
    }

    proptest::proptest! {
        #![proptest_config(proptest::prelude::ProptestConfig::with_cases(24))]

        #[test]
        fn any_tail_damage_recovers_to_a_valid_chain(
            n in 1usize..5,
            cut_back in 1usize..40,
            garbage in proptest::collection::vec(1u8..=255, 0..24),
        ) {
            let dir = tempfile::tempdir().unwrap();
            let (mut ledger, _) = JsonlLedger::open(dir.path(), TimeMs::new(0)).unwrap();
            let drafts: Vec<_> = (0..n)
                .map(|i| draft(EventKind::GateChecked, u64::try_from(i).unwrap()))
                .collect();
            ledger.append_all(drafts).unwrap();
            let baseline = ledger.read_raw_lines().unwrap();
            let seg = only_segment(dir.path());
            drop(ledger);

            let mut bytes = fs::read(&seg).unwrap();
            let keep = bytes.len().saturating_sub(cut_back);
            bytes.truncate(keep);
            bytes.extend_from_slice(&garbage);
            fs::write(&seg, &bytes).unwrap();

            let (reopened, _) = JsonlLedger::open(dir.path(), TimeMs::new(999)).unwrap();
            let lines = reopened.read_raw_lines().unwrap();
            verify_chain(&lines);
            // Every surviving pre-damage line is a byte-exact prefix entry.
            let survivors = lines
                .iter()
                .filter(|l| {
                    EventRecord::parse_line(l).unwrap().kind() != EventKind::LogTruncated
                })
                .count();
            proptest::prop_assert!(survivors <= baseline.len());
            for (mine, original) in lines.iter().take(survivors).zip(baseline.iter()) {
                proptest::prop_assert_eq!(mine, original);
            }
        }
    }
}
