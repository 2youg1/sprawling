// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! The bench harness the performance register calls for: the three
//! wall-clock budgets (`xtask/budgets.toml`) measured on this machine,
//! reported with their machine, never gated.
//!
//! This is a measuring Main, so it is the second sanctioned sampling
//! point besides `bin::assembly`: every `Instant::now` here carries the
//! same `#[expect]` the first one carries.

use std::path::PathBuf;
use std::time::Instant;

use kernel::{Address, EventDraft, EventKind, Ledger as _, Payload, RunId, TimeMs};

fn main() -> std::process::ExitCode {
    let machine = format!(
        "{}-{}, {} core(s)",
        std::env::consts::OS,
        std::env::consts::ARCH,
        std::thread::available_parallelism().map_or(0, std::num::NonZero::get)
    );
    println!("bench on {machine}");
    println!("readings are for this machine; budgets.toml states the budgets\n");
    let scratch = scratch_dir();
    let outcome = ledger_append(&scratch)
        .and_then(|()| prefix_assembly())
        .and_then(|()| projection_rebuild(&scratch));
    std::fs::remove_dir_all(&scratch).ok();
    match outcome {
        Ok(()) => std::process::ExitCode::SUCCESS,
        Err(why) => {
            eprintln!("bench failed: {why}");
            std::process::ExitCode::FAILURE
        }
    }
}

#[expect(
    clippy::disallowed_methods,
    reason = "a bench harness is a measuring Main; time is its subject, not its input"
)]
fn stamp() -> Instant {
    Instant::now()
}

fn scratch_dir() -> PathBuf {
    let dir = std::env::temp_dir().join(format!("sprawl-bench-{}", std::process::id()));
    std::fs::create_dir_all(&dir).ok();
    dir
}

fn draft(n: u64) -> Result<EventDraft, String> {
    let mut map = serde_json::Map::new();
    map.insert("n".to_owned(), serde_json::Value::from(n));
    map.insert(
        "note".to_owned(),
        serde_json::Value::String("a line about the shape of ordinary work".to_owned()),
    );
    Ok(EventDraft {
        run: RunId::CITY,
        t: TimeMs::new(1_700_000_000_000_u64.saturating_add(n)),
        who: "bench".to_owned(),
        addr: None,
        kind: EventKind::SignalEnqueued,
        data: Payload::new(map).map_err(|e| e.to_string())?,
        ig: false,
    })
}

/// Budget row `ledger_append`: append plus fsync, p50 and p99.
fn ledger_append(scratch: &std::path::Path) -> Result<(), String> {
    let dir = scratch.join("ledger");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let (mut ledger, _report) = memory::JsonlLedger::open(&dir, TimeMs::new(1_700_000_000_000))
        .map_err(|e| {
            let ax = e.into_ax();
            format!("{ax}")
        })?;
    const SINGLES: u64 = 1_000;
    let mut times = Vec::with_capacity(usize::try_from(SINGLES).unwrap_or(1_000));
    for n in 0..SINGLES {
        let d = draft(n)?;
        let t0 = stamp();
        ledger.append(d).map_err(|e| format!("{e}"))?;
        times.push(t0.elapsed());
    }
    times.sort();
    let p50 = times.get(times.len() / 2).copied().unwrap_or_default();
    let p99 = times
        .get(times.len().saturating_mul(99) / 100)
        .copied()
        .unwrap_or_default();
    println!(
        "ledger_append      p50 {:>8.3} ms   p99 {:>8.3} ms   (budget 5 / 20 ms; {SINGLES} single appends, one fsync each)",
        p50.as_secs_f64() * 1_000.0,
        p99.as_secs_f64() * 1_000.0
    );

    // The wave shape production actually uses: one barrier per batch.
    const BATCH: u64 = 1_000;
    let drafts: Vec<EventDraft> = (0..BATCH)
        .map(|n| draft(n.saturating_add(SINGLES)))
        .collect::<Result<_, _>>()?;
    let t0 = stamp();
    ledger.append_all(drafts).map_err(|e| format!("{e}"))?;
    let took = t0.elapsed();
    println!(
        "ledger_append_all  {BATCH} records in {:>8.3} ms   ({:.0} records/s group-committed)",
        took.as_secs_f64() * 1_000.0,
        f64::from(u32::try_from(BATCH).unwrap_or(u32::MAX)) / took.as_secs_f64()
    );
    Ok(())
}

/// Budget row `prefix_assembly`: one frozen prefix from realistic docs.
fn prefix_assembly() -> Result<(), String> {
    let addr = |raw: &str| Address::parse(raw).map_err(|e| format!("{e}"));
    let doc = |raw: &str, bytes: usize| -> Result<runtime::SourceDoc, String> {
        Ok(runtime::SourceDoc {
            addr: addr(raw)?,
            bytes: Some(
                "A paragraph of instructions that reads like a real document.\n"
                    .bytes()
                    .cycle()
                    .take(bytes)
                    .collect(),
            ),
        })
    };
    let plan = runtime::PrefixPlan {
        city: vec![doc("City.md", 6_000)?],
        building: vec![doc("lab/BUILDING.md", 2_000)?, doc("lab/Memo.md", 4_000)?],
        resident: vec![doc("lab/URBANITE.md", 3_000)?],
        run: vec![doc("lab/room1/JOB.md", 1_500)?],
        caps: runtime::SegmentCaps::startup_default(),
    };
    const ROUNDS: usize = 1_000;
    let mut times = Vec::with_capacity(ROUNDS);
    for _ in 0..ROUNDS {
        let plan = plan.clone();
        let t0 = stamp();
        let prefix = runtime::build_prefix(plan).map_err(|e| format!("{e}"))?;
        times.push(t0.elapsed());
        std::hint::black_box(prefix);
    }
    times.sort();
    let p50 = times.get(times.len() / 2).copied().unwrap_or_default();
    println!(
        "prefix_assembly    p50 {:>8.3} ms                    (budget 1 ms; 16.5 KB over four slots, {ROUNDS} rounds)",
        p50.as_secs_f64() * 1_000.0
    );
    Ok(())
}

/// Budget row `projection_rebuild`: records folded per second when the
/// disk view is rebuilt from the ledger.
fn projection_rebuild(scratch: &std::path::Path) -> Result<(), String> {
    let dir = scratch.join("rebuild");
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let (mut ledger, _report) = memory::JsonlLedger::open(&dir, TimeMs::new(1_700_000_000_000))
        .map_err(|e| format!("{}", e.into_ax()))?;
    // Every record folds into a table: half start a run, half freeze it,
    // so the reading measures the fold, not the skip arm.
    const RECORDS: u64 = 20_000;
    let drafts: Vec<EventDraft> = (0..RECORDS)
        .map(|n| {
            let mut d = draft(n)?;
            let mut run_bytes = [0u8; 16];
            run_bytes[..8].copy_from_slice(&(n / 2).to_le_bytes());
            d.run = RunId::from_bytes(run_bytes);
            if n % 2 == 0 {
                d.kind = EventKind::RunStarted;
            } else {
                d.kind = EventKind::RunFrozen;
                let mut map = serde_json::Map::new();
                map.insert(
                    "completion".to_owned(),
                    serde_json::Value::String("done".to_owned()),
                );
                d.data = Payload::new(map).map_err(|e| e.to_string())?;
            }
            Ok::<EventDraft, String>(d)
        })
        .collect::<Result<_, _>>()?;
    ledger.append_all(drafts).map_err(|e| format!("{e}"))?;
    let lines = memory::read_raw_lines_at(&dir).map_err(|e| format!("{}", e.into_ax()))?;
    let records: Vec<kernel::EventRecord> = lines
        .iter()
        .map(|line| serde_json::from_slice(line).map_err(|e| e.to_string()))
        .collect::<Result<_, _>>()?;

    let store = scratch.join("projection.redb");
    let (mut projection, _) =
        memory::Projection::open(&store).map_err(|e| format!("{}", e.into_ax()))?;
    let t0 = stamp();
    projection
        .apply_all(records.iter())
        .map_err(|e| format!("{}", e.into_ax()))?;
    let took = t0.elapsed();
    let count = u32::try_from(records.len()).unwrap_or(u32::MAX);
    println!(
        "projection_rebuild {} records in {:>8.3} ms   ({:.0} records/s; budget 50,000/s)",
        records.len(),
        took.as_secs_f64() * 1_000.0,
        f64::from(count) / took.as_secs_f64()
    );
    Ok(())
}
