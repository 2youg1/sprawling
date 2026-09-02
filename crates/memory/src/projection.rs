// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The cold view: the questions too big for memory
//! and too slow for a scan — what is in the Recycle Bin, how far each
//! Run got, where to resume after a restart.
//!
//! It is derived, never authoritative. Delete the file and replay the
//! ledger and you get the same view back; that is the property the
//! tests assert, and it is why `apply` skips anything at or below the
//! last applied seq — a caller that cannot remember where it stopped
//! may simply start over.
//!
//! Determinism is asserted on [`Projection::export_canonical`], not on
//! the database file. redb is free to differ byte-for-byte between
//! runs (allocator state, page reuse); the logical content is not.

use std::path::Path;

use kernel::{EventKind, EventRecord, Seq, TimeMs};
use redb::{ReadableDatabase, ReadableTable, TableDefinition};
use serde_json::Value;

use crate::error::MemoryError;

const META: TableDefinition<&str, u64> = TableDefinition::new("meta");
const RUNS: TableDefinition<&str, &str> = TableDefinition::new("runs");
const RECYCLE: TableDefinition<u64, &str> = TableDefinition::new("recycle");
const LAST_APPLIED: &str = "last_applied";

/// One row of the Recycle Bin: what was discarded, and whether it has
/// since come back.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecycleEntry {
    pub seq: Seq,
    pub t: TimeMs,
    pub paths: Vec<String>,
    pub restoration: String,
    pub restored: bool,
}

/// One row of the progress view.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunRow {
    pub run: String,
    pub started_t: TimeMs,
    pub frozen: Option<String>,
}

pub struct Projection {
    db: redb::Database,
    last_applied: Option<Seq>,
}

/// What open found and repaired.
pub struct ProjectionOpenReport {
    pub rebuilt: Option<ViewRebuilt>,
}

/// The stored view could not be read, so open removed it and started an
/// empty one. `last_applied` is `None` in consequence, and that is the
/// whole instruction to the caller: replay from the start.
pub struct ViewRebuilt {
    /// What the store said before the file went. Removing the file
    /// destroys the only other copy of this sentence, and a view that
    /// resets without saying why teaches nobody anything.
    pub reason: String,
}

fn db_err(op: &'static str) -> impl FnOnce(String) -> MemoryError {
    move |detail| MemoryError::Projection {
        op,
        detail: detail.to_string(),
    }
}

/// One record into the open tables. Shared by the single-record and the
/// batched fold so "what a record means" has exactly one definition.
fn fold_record(
    record: &EventRecord,
    runs: &mut redb::Table<'_, &'static str, &'static str>,
    recycle: &mut redb::Table<'_, u64, &'static str>,
) -> Result<(), MemoryError> {
    let seq = record.seq();
    match record.kind() {
        EventKind::RunStarted => {
            let row = serde_json::json!({
                "started_t": record.t().value(),
                "frozen": Value::Null,
            })
            .to_string();
            runs.insert(record.run().to_string().as_str(), row.as_str())
                .map_err(|e| db_err("insert run row")(e.to_string()))?;
        }
        EventKind::RunFrozen => {
            let key = record.run().to_string();
            let started = runs
                .get(key.as_str())
                .map_err(|e| db_err("read run row")(e.to_string()))?
                .and_then(|guard| {
                    serde_json::from_str::<Value>(guard.value())
                        .ok()?
                        .get("started_t")?
                        .as_u64()
                })
                .unwrap_or_else(|| record.t().value());
            let completion = record
                .data()
                .as_map()
                .get("completion")
                .and_then(Value::as_str)
                .unwrap_or("unspecified")
                .to_owned();
            let row = serde_json::json!({
                "started_t": started,
                "frozen": completion,
            })
            .to_string();
            runs.insert(key.as_str(), row.as_str())
                .map_err(|e| db_err("insert run row")(e.to_string()))?;
        }
        EventKind::FileDiscarded => {
            let data = record.data().as_map();
            let paths: Vec<String> = data
                .get("paths")
                .and_then(Value::as_array)
                .map(|items| {
                    items
                        .iter()
                        .filter_map(|v| v.as_str().map(str::to_owned))
                        .collect()
                })
                .unwrap_or_default();
            let row = serde_json::json!({
                "t": record.t().value(),
                "paths": paths,
                "restoration": restoration_label(data.get("restoration")),
                "restored": false,
            })
            .to_string();
            recycle
                .insert(seq.value(), row.as_str())
                .map_err(|e| db_err("insert recycle row")(e.to_string()))?;
        }
        EventKind::DiscardRestored => {
            // The restore names the discard it undoes; a restore with no
            // matching discard is dropped, not invented.
            if let Some(target) = record
                .data()
                .as_map()
                .get("discard_seq")
                .and_then(Value::as_u64)
            {
                let existing = recycle
                    .get(target)
                    .map_err(|e| db_err("read recycle row")(e.to_string()))?
                    .map(|guard| guard.value().to_owned());
                if let Some(raw) = existing
                    && let Ok(mut parsed) = serde_json::from_str::<Value>(&raw)
                    && let Some(map) = parsed.as_object_mut()
                {
                    map.insert("restored".to_owned(), Value::Bool(true));
                    let row = parsed.to_string();
                    recycle
                        .insert(target, row.as_str())
                        .map_err(|e| db_err("insert recycle row")(e.to_string()))?;
                }
            }
        }
        _ => {}
    }
    Ok(())
}

impl Projection {
    /// Always hands back a usable view.
    ///
    /// A stored view that cannot be read is removed and started again,
    /// because this view is derived and its recorded recovery is to
    /// delete the file and replay. The caller needs no new branch for
    /// that case: the fresh view reports `last_applied() == None`, which
    /// it must already handle as an ordinary first run.
    ///
    /// The removal is attempted once. A file that is merely corrupt heals
    /// on the second open; a directory that cannot be written fails the
    /// second open too and reports, so no error variant has to be told
    /// apart from another.
    pub fn open(path: &Path) -> Result<(Projection, ProjectionOpenReport), MemoryError> {
        let unreadable = match Self::open_once(path) {
            Ok(projection) => {
                return Ok((projection, ProjectionOpenReport { rebuilt: None }));
            }
            Err(failure) => failure.to_string(),
        };
        match std::fs::remove_file(path) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
            Err(err) => {
                return Err(db_err("remove unreadable projection")(err.to_string()));
            }
        }
        let projection = Self::open_once(path)?;
        Ok((
            projection,
            ProjectionOpenReport {
                rebuilt: Some(ViewRebuilt { reason: unreadable }),
            },
        ))
    }

    fn open_once(path: &Path) -> Result<Projection, MemoryError> {
        let db =
            redb::Database::create(path).map_err(|e| db_err("open projection")(e.to_string()))?;
        // Creating the tables at open time means every later read finds
        // them, so "empty" and "absent" never need distinguishing.
        let txn = db
            .begin_write()
            .map_err(|e| db_err("begin projection write")(e.to_string()))?;
        {
            txn.open_table(META)
                .map_err(|e| db_err("open meta table")(e.to_string()))?;
            txn.open_table(RUNS)
                .map_err(|e| db_err("open runs table")(e.to_string()))?;
            txn.open_table(RECYCLE)
                .map_err(|e| db_err("open recycle table")(e.to_string()))?;
        }
        txn.commit()
            .map_err(|e| db_err("commit projection open")(e.to_string()))?;
        let last_applied = read_last_applied(&db)?;
        Ok(Projection { db, last_applied })
    }

    /// Folds one record in, inside one transaction. Idempotent by seq:
    /// a record at or below `last_applied` is a no-op, so replaying a
    /// segment cannot double-count.
    pub fn apply(&mut self, record: &EventRecord) -> Result<(), MemoryError> {
        self.apply_all(std::iter::once(record))
    }

    /// Folds a batch inside one transaction - one durability barrier for
    /// the whole fold, mirroring the ledger's own group commit. This is
    /// the rebuild path: per-record transactions put a disk barrier under
    /// every event and rebuilt at about a thousand records a second; one
    /// transaction per batch rebuilds at the budgeted rate. Same fold,
    /// same idempotence; only the barrier moves.
    pub fn apply_all<'a>(
        &mut self,
        records: impl IntoIterator<Item = &'a EventRecord>,
    ) -> Result<(), MemoryError> {
        let mut last = self.last_applied;
        let txn = self
            .db
            .begin_write()
            .map_err(|e| db_err("begin projection write")(e.to_string()))?;
        {
            let mut runs = txn
                .open_table(RUNS)
                .map_err(|e| db_err("open runs table")(e.to_string()))?;
            let mut recycle = txn
                .open_table(RECYCLE)
                .map_err(|e| db_err("open recycle table")(e.to_string()))?;
            let mut folded_any = false;
            for record in records {
                let seq = record.seq();
                if last.is_some_and(|l| seq <= l) {
                    continue;
                }
                fold_record(record, &mut runs, &mut recycle)?;
                last = Some(seq);
                folded_any = true;
            }
            if !folded_any {
                // Nothing new: leave the store byte-identical rather than
                // committing an empty transaction.
                drop(runs);
                drop(recycle);
                txn.abort()
                    .map_err(|e| db_err("abort empty projection write")(e.to_string()))?;
                return Ok(());
            }
            let mut meta = txn
                .open_table(META)
                .map_err(|e| db_err("open meta table")(e.to_string()))?;
            if let Some(seq) = last {
                meta.insert(LAST_APPLIED, seq.value())
                    .map_err(|e| db_err("write last_applied")(e.to_string()))?;
            }
        }
        txn.commit()
            .map_err(|e| db_err("commit projection apply")(e.to_string()))?;
        self.last_applied = last;
        Ok(())
    }

    /// The Recycle Bin, seq-ordered. Restored entries stay listed with
    /// `restored: true` — the bin is a history, not a pending queue.
    pub fn recycle_bin(&self) -> Result<Vec<RecycleEntry>, MemoryError> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| db_err("begin projection read")(e.to_string()))?;
        let table = txn
            .open_table(RECYCLE)
            .map_err(|e| db_err("open recycle table")(e.to_string()))?;
        let mut out = Vec::new();
        let iter = table
            .iter()
            .map_err(|e| db_err("scan recycle table")(e.to_string()))?;
        for row in iter {
            let (key, value) = row.map_err(|e| db_err("read recycle row")(e.to_string()))?;
            let parsed: Value = serde_json::from_str(value.value())
                .map_err(|e| db_err("parse recycle row")(e.to_string()))?;
            out.push(RecycleEntry {
                seq: Seq::new(key.value()),
                t: TimeMs::new(parsed.get("t").and_then(Value::as_u64).unwrap_or(0)),
                paths: parsed
                    .get("paths")
                    .and_then(Value::as_array)
                    .map(|items| {
                        items
                            .iter()
                            .filter_map(|v| v.as_str().map(str::to_owned))
                            .collect()
                    })
                    .unwrap_or_default(),
                restoration: parsed
                    .get("restoration")
                    .and_then(Value::as_str)
                    .unwrap_or("unknown")
                    .to_owned(),
                restored: parsed
                    .get("restored")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            });
        }
        Ok(out)
    }

    /// The progress view, RunId-ordered.
    pub fn run_rows(&self) -> Result<Vec<RunRow>, MemoryError> {
        let txn = self
            .db
            .begin_read()
            .map_err(|e| db_err("begin projection read")(e.to_string()))?;
        let table = txn
            .open_table(RUNS)
            .map_err(|e| db_err("open runs table")(e.to_string()))?;
        let mut out = Vec::new();
        let iter = table
            .iter()
            .map_err(|e| db_err("scan runs table")(e.to_string()))?;
        for row in iter {
            let (key, value) = row.map_err(|e| db_err("read run row")(e.to_string()))?;
            let parsed: Value = serde_json::from_str(value.value())
                .map_err(|e| db_err("parse run row")(e.to_string()))?;
            out.push(RunRow {
                run: key.value().to_owned(),
                started_t: TimeMs::new(
                    parsed.get("started_t").and_then(Value::as_u64).unwrap_or(0),
                ),
                frozen: parsed
                    .get("frozen")
                    .and_then(Value::as_str)
                    .map(str::to_owned),
            });
        }
        Ok(out)
    }

    pub fn last_applied(&self) -> Option<Seq> {
        self.last_applied
    }

    /// The logical content, table by table, key-ordered, one line per
    /// row. Two projections built from the same ledger export the same
    /// bytes — that is the rebuild guarantee, stated where it holds.
    pub fn export_canonical(&self) -> Result<Vec<u8>, MemoryError> {
        let mut out = String::new();
        out.push_str(&format!(
            "meta {LAST_APPLIED} {}\n",
            self.last_applied.map(|s| s.value()).unwrap_or(0)
        ));
        for row in self.run_rows()? {
            out.push_str(&format!(
                "runs {} {} {}\n",
                row.run,
                row.started_t.value(),
                row.frozen.unwrap_or_else(|| "-".to_owned())
            ));
        }
        for entry in self.recycle_bin()? {
            out.push_str(&format!(
                "recycle {} {} {} {} {}\n",
                entry.seq.value(),
                entry.t.value(),
                entry.paths.join(","),
                entry.restoration,
                entry.restored
            ));
        }
        Ok(out.into_bytes())
    }
}

/// The plan's variant name. The full Locator lives in the ledger; the
/// cold view keeps only what a list needs to render.
fn restoration_label(value: Option<&Value>) -> String {
    match value {
        Some(Value::Object(map)) => map
            .keys()
            .next()
            .cloned()
            .unwrap_or_else(|| "unknown".to_owned()),
        Some(Value::String(s)) => s.clone(),
        _ => "unknown".to_owned(),
    }
}

fn read_last_applied(db: &redb::Database) -> Result<Option<Seq>, MemoryError> {
    let txn = db
        .begin_read()
        .map_err(|e| db_err("begin projection read")(e.to_string()))?;
    let table = txn
        .open_table(META)
        .map_err(|e| db_err("open meta table")(e.to_string()))?;
    let found = table
        .get(LAST_APPLIED)
        .map_err(|e| db_err("read last_applied")(e.to_string()))?
        .map(|guard| Seq::new(guard.value()));
    Ok(found)
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
    use kernel::{B3Hash, EventDraft, Payload, RunId};

    fn record(run: RunId, seq: u64, kind: EventKind, data: Value) -> EventRecord {
        let map = data.as_object().cloned().unwrap_or_default();
        let draft = EventDraft {
            run,
            t: TimeMs::new(seq.saturating_mul(10)),
            who: "resident".to_owned(),
            addr: None,
            kind,
            data: Payload::new(map).unwrap(),
            ig: false,
        };
        EventRecord::from_draft(draft, Seq::new(seq), B3Hash::digest(b""))
    }

    fn script(run: RunId) -> Vec<EventRecord> {
        vec![
            record(run, 0, EventKind::RunStarted, serde_json::json!({})),
            record(
                run,
                1,
                EventKind::FileDiscarded,
                serde_json::json!({
                    "paths": ["file:build/out.o", "file:build/tmp.o"],
                    "restoration": { "rebuildable": { "reason": "cargo build" } },
                }),
            ),
            record(run, 2, EventKind::ToolCalled, serde_json::json!({})),
            record(
                run,
                3,
                EventKind::DiscardRestored,
                serde_json::json!({ "discard_seq": 1 }),
            ),
            record(
                run,
                4,
                EventKind::RunFrozen,
                serde_json::json!({ "completion": "done" }),
            ),
        ]
    }

    #[test]
    fn two_rebuilds_from_one_ledger_export_the_same_bytes() {
        let tmp = tempfile::tempdir().unwrap();
        let run = RunId::from_bytes([5u8; 16]);
        let records = script(run);

        let (mut first, _) = Projection::open(&tmp.path().join("a.redb")).unwrap();
        for r in &records {
            first.apply(r).unwrap();
        }
        let first_bytes = first.export_canonical().unwrap();

        // A second projection, built from the same ledger in the same
        // order, is indistinguishable at the logical level.
        let (mut second, _) = Projection::open(&tmp.path().join("b.redb")).unwrap();
        for r in &records {
            second.apply(r).unwrap();
        }
        assert_eq!(first_bytes, second.export_canonical().unwrap());

        // And a discarded file that came back reads as restored.
        let bin = first.recycle_bin().unwrap();
        assert_eq!(bin.len(), 1);
        assert_eq!(bin[0].seq, Seq::new(1));
        assert_eq!(bin[0].paths.len(), 2);
        assert_eq!(bin[0].restoration, "rebuildable");
        assert!(bin[0].restored);

        let rows = first.run_rows().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].started_t, TimeMs::new(0));
        assert_eq!(rows[0].frozen.as_deref(), Some("done"));
    }

    #[test]
    fn replaying_the_ledger_twice_changes_nothing() {
        let tmp = tempfile::tempdir().unwrap();
        let run = RunId::from_bytes([6u8; 16]);
        let records = script(run);
        let (mut projection, _) = Projection::open(&tmp.path().join("p.redb")).unwrap();
        for r in &records {
            projection.apply(r).unwrap();
        }
        let once = projection.export_canonical().unwrap();
        for r in &records {
            projection.apply(r).unwrap();
        }
        assert_eq!(once, projection.export_canonical().unwrap());
        assert_eq!(projection.last_applied(), Some(Seq::new(4)));
    }

    #[test]
    fn a_reopened_projection_resumes_where_it_stopped() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("resume.redb");
        let run = RunId::from_bytes([7u8; 16]);
        let records = script(run);
        {
            let (mut projection, _) = Projection::open(&path).unwrap();
            for r in records.iter().take(3) {
                projection.apply(r).unwrap();
            }
            assert_eq!(projection.last_applied(), Some(Seq::new(2)));
        }
        let (mut reopened, _) = Projection::open(&path).unwrap();
        assert_eq!(
            reopened.last_applied(),
            Some(Seq::new(2)),
            "the resume point survives the restart"
        );
        for r in &records {
            reopened.apply(r).unwrap();
        }
        assert_eq!(reopened.last_applied(), Some(Seq::new(4)));

        // Deleting the file and replaying everything gives the same view.
        let fresh_path = tmp.path().join("fresh.redb");
        let (mut fresh, _) = Projection::open(&fresh_path).unwrap();
        for r in &records {
            fresh.apply(r).unwrap();
        }
        assert_eq!(
            reopened.export_canonical().unwrap(),
            fresh.export_canonical().unwrap(),
            "the projection is derived, so it is disposable"
        );
    }

    /// The SPEC states this view's recovery as "delete the file and
    /// replay", and the whole argument for keeping a young store here is
    /// that its failure costs a rebuild rather than data. That argument
    /// is worth nothing while no code performs the rebuild: the first
    /// caller to meet a file it cannot read would have met an error
    /// instead.
    #[test]
    fn an_unreadable_view_is_rebuilt_rather_than_reported() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("garbled.redb");
        std::fs::write(&path, b"this is not a redb file, and never was").unwrap();

        let (mut projection, report) =
            Projection::open(&path).expect("a derived view opens even when its file does not");
        let rebuilt = report
            .rebuilt
            .expect("open must say that it reset the view, not reset it in silence");
        assert!(
            !rebuilt.reason.is_empty(),
            "removing the file destroys the only other copy of this sentence"
        );
        assert_eq!(
            projection.last_applied(),
            None,
            "an empty view is the instruction to replay from the start"
        );

        let run = RunId::from_bytes([9u8; 16]);
        let records = script(run);
        for r in &records {
            projection.apply(r).unwrap();
        }
        let fresh_path = tmp.path().join("fresh-after-garble.redb");
        let (mut fresh, fresh_report) = Projection::open(&fresh_path).unwrap();
        assert!(
            fresh_report.rebuilt.is_none(),
            "a path with no file is the ordinary first run, not a repair"
        );
        for r in &records {
            fresh.apply(r).unwrap();
        }
        assert_eq!(
            projection.export_canonical().unwrap(),
            fresh.export_canonical().unwrap(),
            "the rebuilt view is the view"
        );
    }

    #[test]
    fn a_restore_naming_no_discard_is_dropped_not_invented() {
        let tmp = tempfile::tempdir().unwrap();
        let run = RunId::from_bytes([8u8; 16]);
        let (mut projection, _) = Projection::open(&tmp.path().join("q.redb")).unwrap();
        projection
            .apply(&record(
                run,
                0,
                EventKind::RunStarted,
                serde_json::json!({}),
            ))
            .unwrap();
        projection
            .apply(&record(
                run,
                1,
                EventKind::DiscardRestored,
                serde_json::json!({ "discard_seq": 4242 }),
            ))
            .unwrap();
        assert!(projection.recycle_bin().unwrap().is_empty());
        assert_eq!(projection.last_applied(), Some(Seq::new(1)));
    }
}
