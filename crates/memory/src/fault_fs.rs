// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! FaultFs: the second Vfs adapter — a deterministic power-loss model
//!. Compiled for tests and for citysim under the
//! `fault` feature; never in a production build.
//!
//! The model is stricter than any real platform so the write discipline
//! it enforces holds on every platform (memory-SPEC 8-2):
//! - every file has two planes: `durable` (survives power loss) and
//!   `live` (what the running process observes). `sync_data` promotes
//!   live to durable; a power cut drops the unsynced delta, except a
//!   `TornTail` prefix that models a torn write reaching the platter.
//! - a created file's directory entry survives only after `sync_dir` on
//!   its parent — even when its bytes were synced (stricter than POSIX).
//! - removals are instantly durable (simplification: the only remover is
//!   tail recovery, and a resurrected empty segment is harmless — reopen
//!   tolerates it).
//! - the op hitting `cut_at_op` fails with "power lost"; the plan is then
//!   consumed, so the same instance serves the powered reopen. Appends
//!   land on `live` before the cut check so the tear can bite the very
//!   write that died.
//!
//! Everything is explicit in [`FaultPlan`]; there is no randomness.

use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use crate::jsonl::Vfs;

/// The whole fault script; fully explicit, fully deterministic.
#[derive(Debug, Clone, Copy)]
pub struct FaultPlan {
    /// 1-based op number that dies with "power lost"; `None` = never.
    pub cut_at_op: Option<u64>,
    pub torn_tail: TornTail,
}

/// How much of each file's unsynced delta the platter kept.
#[derive(Debug, Clone, Copy)]
pub enum TornTail {
    None,
    KeepBytes(u64),
}

struct FileState {
    durable: Vec<u8>,
    live: Vec<u8>,
    durable_entry: bool,
}

struct State {
    files: BTreeMap<PathBuf, FileState>,
    dirs: BTreeSet<PathBuf>,
    op: u64,
    plan: FaultPlan,
}

/// Shared-state handle: clone it, hand one clone to the ledger, keep one
/// to cut power and to reopen after the crash.
#[derive(Clone)]
pub struct FaultFs(Rc<RefCell<State>>);

impl FaultFs {
    pub fn new(plan: FaultPlan) -> Self {
        FaultFs(Rc::new(RefCell::new(State {
            files: BTreeMap::new(),
            dirs: BTreeSet::new(),
            op: 0,
            plan,
        })))
    }

    /// Power loss now: see the module doc for the exact semantics.
    pub fn power_cut(&self) {
        let mut state = self.0.borrow_mut();
        cut(&mut state);
    }

    pub fn op_count(&self) -> u64 {
        self.0.borrow().op
    }

    /// Counts the op; when it hits the plan, power dies: the cut applies
    /// and the op itself fails.
    fn charge(&self, op_name: &'static str) -> io::Result<()> {
        let mut state = self.0.borrow_mut();
        state.op = state.op.saturating_add(1);
        if state.plan.cut_at_op == Some(state.op) {
            state.plan.cut_at_op = None;
            cut(&mut state);
            return Err(io::Error::other(format!(
                "power lost during {op_name} (op {})",
                state.op
            )));
        }
        Ok(())
    }
}

fn cut(state: &mut State) {
    let torn = state.plan.torn_tail;
    state.files.retain(|_, file| file.durable_entry);
    for file in state.files.values_mut() {
        let delta = file.live.get(file.durable.len()..).unwrap_or(&[]);
        let keep = match torn {
            TornTail::None => 0,
            TornTail::KeepBytes(k) => usize::try_from(k).unwrap_or(usize::MAX).min(delta.len()),
        };
        let mut settled = file.durable.clone();
        settled.extend_from_slice(delta.get(..keep).unwrap_or(&[]));
        file.durable = settled.clone();
        file.live = settled;
    }
}

fn not_found(path: &Path) -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        format!("no such file: {}", path.display()),
    )
}

impl Vfs for FaultFs {
    fn create_dir_all(&mut self, dir: &Path) -> io::Result<()> {
        self.0.borrow_mut().dirs.insert(dir.to_path_buf());
        self.charge("create_dir_all")
    }

    fn list(&self, dir: &Path) -> io::Result<Vec<PathBuf>> {
        // A read op still charges: the process can die mid-read too.
        FaultFs::charge(self, "list")?;
        let state = self.0.borrow();
        let mut files: Vec<PathBuf> = state
            .files
            .keys()
            .filter(|p| p.parent() == Some(dir))
            .cloned()
            .collect();
        files.sort();
        Ok(files)
    }

    fn list_dirs(&self, dir: &Path) -> io::Result<Vec<PathBuf>> {
        FaultFs::charge(self, "list_dirs")?;
        let state = self.0.borrow();
        let mut dirs: Vec<PathBuf> = state
            .files
            .keys()
            .filter_map(|path| path.parent())
            .filter(|parent| parent.parent() == Some(dir))
            .map(Path::to_path_buf)
            .collect();
        dirs.sort();
        dirs.dedup();
        Ok(dirs)
    }

    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        FaultFs::charge(self, "read")?;
        let state = self.0.borrow();
        state
            .files
            .get(path)
            .map(|f| f.live.clone())
            .ok_or_else(|| not_found(path))
    }

    fn append(&mut self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        {
            let mut state = self.0.borrow_mut();
            let file = state.files.entry(path.to_path_buf()).or_insert(FileState {
                durable: Vec::new(),
                live: Vec::new(),
                durable_entry: false,
            });
            file.live.extend_from_slice(bytes);
        }
        // Bytes are on the live plane before the charge: the tear model
        // can bite exactly this write.
        self.charge("append")
    }

    fn truncate(&mut self, path: &Path, len: u64) -> io::Result<()> {
        {
            let mut state = self.0.borrow_mut();
            let file = state.files.get_mut(path).ok_or_else(|| not_found(path))?;
            let len = usize::try_from(len).unwrap_or(usize::MAX);
            file.live.truncate(len);
        }
        self.charge("truncate")
    }

    fn rename(&mut self, from: &Path, to: &Path) -> io::Result<()> {
        {
            let mut state = self.0.borrow_mut();
            let Some(mut file) = state.files.remove(from) else {
                return Err(not_found(from));
            };
            // The target's dir entry is new: it survives only after a
            // sync_dir. Stricter than reality — a cut here loses the
            // object entirely, but its put never returned Ok, so no
            // acknowledged effect is lost (memory-SPEC 8-3).
            file.durable_entry = false;
            state.files.insert(to.to_path_buf(), file);
        }
        self.charge("rename")
    }

    fn sync_data(&mut self, path: &Path) -> io::Result<()> {
        // Charge first: when power dies during the barrier, the barrier
        // never happened.
        self.charge("sync_data")?;
        let mut state = self.0.borrow_mut();
        let file = state.files.get_mut(path).ok_or_else(|| not_found(path))?;
        file.durable = file.live.clone();
        Ok(())
    }

    fn sync_dir(&mut self, dir: &Path) -> io::Result<()> {
        self.charge("sync_dir")?;
        let mut state = self.0.borrow_mut();
        for (path, file) in state.files.iter_mut() {
            if path.parent() == Some(dir) {
                file.durable_entry = true;
            }
        }
        Ok(())
    }

    fn remove_file(&mut self, path: &Path) -> io::Result<()> {
        {
            let mut state = self.0.borrow_mut();
            if state.files.remove(path).is_none() {
                return Err(not_found(path));
            }
        }
        self.charge("remove_file")
    }

    fn exists(&self, path: &Path) -> bool {
        self.0.borrow().files.contains_key(path)
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
    use crate::jsonl::{JsonlLedger, Vfs};
    use kernel::{
        EventDraft, EventKind, EventRecord, GENESIS_PREV, Payload, RunId, Seq, TimeMs, chain_hash,
    };
    use std::path::{Path, PathBuf};

    fn plain() -> FaultPlan {
        FaultPlan {
            cut_at_op: None,
            torn_tail: TornTail::None,
        }
    }

    fn cut_at(op: u64, torn: TornTail) -> FaultPlan {
        FaultPlan {
            cut_at_op: Some(op),
            torn_tail: torn,
        }
    }

    #[test]
    fn unsynced_bytes_vanish_and_synced_bytes_survive() {
        let fs = FaultFs::new(plain());
        let mut v: Box<dyn Vfs> = Box::new(fs.clone());
        let dir = PathBuf::from("d");
        let file = dir.join("f");
        v.create_dir_all(&dir).unwrap();
        v.append(&file, b"durable").unwrap();
        v.sync_data(&file).unwrap();
        v.sync_dir(&dir).unwrap();
        v.append(&file, b"+lost").unwrap();
        fs.power_cut();
        assert_eq!(v.read(&file).unwrap(), b"durable");
    }

    #[test]
    fn torn_tail_keeps_a_prefix_of_the_unsynced_delta() {
        let fs = FaultFs::new(FaultPlan {
            cut_at_op: None,
            torn_tail: TornTail::KeepBytes(3),
        });
        let mut v: Box<dyn Vfs> = Box::new(fs.clone());
        let dir = PathBuf::from("d");
        let file = dir.join("f");
        v.create_dir_all(&dir).unwrap();
        v.append(&file, b"ok").unwrap();
        v.sync_data(&file).unwrap();
        v.sync_dir(&dir).unwrap();
        v.append(&file, b"abcdef").unwrap();
        fs.power_cut();
        assert_eq!(v.read(&file).unwrap(), b"okabc");
    }

    #[test]
    fn a_file_without_a_synced_dir_entry_vanishes_entirely() {
        let fs = FaultFs::new(plain());
        let mut v: Box<dyn Vfs> = Box::new(fs.clone());
        let dir = PathBuf::from("d");
        let file = dir.join("f");
        v.create_dir_all(&dir).unwrap();
        v.append(&file, b"synced but entry is not").unwrap();
        v.sync_data(&file).unwrap();
        // no sync_dir: stricter than POSIX on purpose (memory-SPEC 8-2).
        fs.power_cut();
        assert!(!v.exists(&file));
        assert!(v.read(&file).is_err());
    }

    #[test]
    fn the_cut_op_fails_and_later_ops_proceed() {
        let fs = FaultFs::new(cut_at(3, TornTail::None));
        let mut v: Box<dyn Vfs> = Box::new(fs.clone());
        let dir = PathBuf::from("d");
        v.create_dir_all(&dir).unwrap(); // op 1
        v.append(&dir.join("f"), b"x").unwrap(); // op 2
        let denied = v.sync_data(&dir.join("f")); // op 3: power dies here
        assert!(denied.is_err());
        assert!(v.list(&dir).is_ok(), "after the cut the plan is consumed");
        assert_eq!(fs.op_count(), 4);
    }

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

    fn waves() -> Vec<Vec<EventDraft>> {
        vec![
            vec![
                draft(EventKind::CityInitialized, 1),
                draft(EventKind::BuildingCreated, 2),
            ],
            vec![draft(EventKind::RunStarted, 3)],
            vec![
                draft(EventKind::GateChecked, 4),
                draft(EventKind::RunFrozen, 5),
            ],
        ]
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

    /// A3 point 1 (power loss at EventRecord append), whole-matrix form:
    /// cut at every single vfs op; after reopen the chain verifies and
    /// every wave that returned Ok is still there, byte-exact.
    #[test]
    fn power_cut_matrix_over_every_op_keeps_acknowledged_waves() {
        let dir = Path::new("city/ledger");

        // Baseline: full run, no cut. Also measures total op count.
        let fs = FaultFs::new(plain());
        let (mut ledger, _) =
            JsonlLedger::open_with(Box::new(fs.clone()), dir, TimeMs::new(0)).expect("clean open");
        let mut cumulative: Vec<usize> = Vec::new();
        for wave in waves() {
            ledger.append_all(wave).expect("clean append");
            cumulative.push(ledger.read_raw_lines().unwrap().len());
        }
        let baseline = ledger.read_raw_lines().unwrap();
        let total_ops = fs.op_count();
        drop(ledger);

        for torn in [TornTail::None, TornTail::KeepBytes(7)] {
            for cut in 1..=total_ops {
                let fs = FaultFs::new(cut_at(cut, torn));
                let mut acknowledged = 0usize;
                // Err from open = power died during it; nothing ran.
                if let Ok((mut ledger, _)) =
                    JsonlLedger::open_with(Box::new(fs.clone()), dir, TimeMs::new(0))
                {
                    for wave in waves() {
                        match ledger.append_all(wave) {
                            Ok(_) => {
                                acknowledged = ledger
                                    .read_raw_lines()
                                    .map(|l| l.len())
                                    .unwrap_or(acknowledged)
                            }
                            Err(_) => break, // crash-only: the process dies here
                        }
                    }
                }

                // When the cut lands during the reopen itself (a second
                // power loss, possibly mid-recovery), the next open must
                // succeed — recovery is idempotent and the plan is spent.
                let reopened =
                    match JsonlLedger::open_with(Box::new(fs.clone()), dir, TimeMs::new(99)) {
                        Ok((ledger, _)) => ledger,
                        Err(_) => {
                            let (ledger, _) =
                                JsonlLedger::open_with(Box::new(fs.clone()), dir, TimeMs::new(99))
                                    .unwrap_or_else(|e| {
                                        panic!("second reopen after cut {cut} ({torn:?}): {e}")
                                    });
                            ledger
                        }
                    };
                let lines = reopened.read_raw_lines().unwrap();
                verify_chain(&lines);
                let survivors: Vec<&Vec<u8>> = lines
                    .iter()
                    .filter(|l| {
                        EventRecord::parse_line(l).unwrap().kind() != EventKind::LogTruncated
                    })
                    .collect();
                assert!(
                    survivors.len() >= acknowledged,
                    "cut {cut} ({torn:?}): acknowledged waves must survive"
                );
                for (mine, original) in survivors.iter().zip(baseline.iter()) {
                    assert_eq!(
                        *mine, original,
                        "cut {cut} ({torn:?}): survivors must be a byte-exact prefix"
                    );
                }
            }
        }
    }
}
