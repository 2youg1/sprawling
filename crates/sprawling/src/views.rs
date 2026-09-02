// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The fold every query is answered from, and the lines a page reads off
//! it.
//!
//! **Why it is a projection and not part of the assembly point.** Nothing
//! here decides anything or reaches a provider: it folds records into the
//! answers a client asks for, and `rebuild_views` throws the whole thing
//! away and folds the ledger again to get the same bytes. That is
//! ARCHITECTURE.md section 9 shape 7, while `bin::assembly` is an
//! adapter - and a file holding two shapes is what section 9 says a split
//! looks like.
//!
//! **What it deliberately does not hold.** The plans are
//! `crate::plan_view`'s and are read through it; a second parse here
//! would be a second answer to "what is stuck and why", and only one of
//! them would be folding the records that say why. What waits in a room
//! is folded from signal records rather than read off a queue, because a
//! queue answers by being consumed and a view that consumed what it
//! showed would change the thing it reports on.

use std::path::{Path, PathBuf};

use kernel::{Address, AxCode, AxError, EventKind, EventRecord, RunId};

// Where a city keeps its ledger and how a building reads off disk are
// `bin::assembly`'s: it forms the city that laid them out. Borrowed
// rather than copied, so "where the ledger lives" keeps one answer.
use crate::assembly::{city_address, ledger_dir, read_building};

/// The derived views a query reads. They are rebuilt from the ledger at
/// startup and folded forward by the write observer, so deleting them
/// costs nothing but the rebuild — the ledger remains the only history.
pub(crate) struct Views {
    city_root: PathBuf,
    hot: memory::HotView,
    attribution: memory::Attribution,
    approvals: std::collections::BTreeMap<String, kernel::ApprovalItem>,
    book: gateway::EndpointBook,
    /// The city's own name, as its first record states it. Handed to a
    /// client at the handshake: the event stream only carries what
    /// happens next, and a browser opened today would otherwise have no
    /// way to learn the name of a city initialised last month.
    city: Option<Address>,
    /// What waits in each room, folded from the signal records. Held
    /// here rather than read off a queue: a queue answers by being
    /// consumed, and a view that consumed what it showed would change
    /// the thing it reports on.
    waiting: std::collections::BTreeMap<Address, Vec<channels::SignalLine>>,
    /// Discarded files, keyed by path so a restoration closes the row
    /// it opened rather than adding a second one.
    discards: std::collections::BTreeMap<String, channels::DiscardLine>,
    /// What the city archived, newest last.
    assets: Vec<channels::RegistryLine>,
    /// How many records this view has folded. The one number a page
    /// cannot derive from any other answer.
    events: u64,
    /// seq to byte offset, held rather than rebuilt.
    ///
    /// Rebuilding it read the whole side cache and allocated a `String`
    /// per line, and that was charged to every history question a page
    /// asked - 14.4 ms of it on a fifty thousand record ledger. Held, the
    /// same question costs one directory listing and the bytes that are
    /// actually new.
    index: memory::LedgerIndex,
    /// Every building's plan, parsed once and re-parsed only when a
    /// record says it may have moved.
    plans: crate::plan_view::PlanView,
    /// What each building is working towards, folded from the records
    /// that said so. The goal text and its state, not the value itself:
    /// declaring a pursuit takes the depth-zero position, and a view
    /// that could mint one would be a second door onto the guard.
    pursuits: std::collections::BTreeMap<Address, (String, kernel::PursuitState)>,
}

impl Views {
    pub(crate) fn new(city_root: &Path) -> Views {
        Views {
            city_root: city_root.to_path_buf(),
            hot: memory::HotView::new(),
            attribution: memory::Attribution::new(),
            approvals: std::collections::BTreeMap::new(),
            book: gateway::EndpointBook::new(),
            city: None,
            waiting: std::collections::BTreeMap::new(),
            discards: std::collections::BTreeMap::new(),
            assets: Vec::new(),
            events: 0,
            // An unreadable ledger directory is not a reason to refuse to
            // start: the index is disposable, every refresh tries again,
            // and a city with no ledger yet is the ordinary first run.
            index: memory::LedgerIndex::load_or_rebuild(&ledger_dir(city_root))
                .unwrap_or_else(|_| memory::LedgerIndex::empty()),
            plans: crate::plan_view::PlanView::default(),
            pursuits: std::collections::BTreeMap::new(),
        }
    }

    /// One entry per building, with its plan as the projection last read
    /// it.
    fn spine(&mut self) -> Vec<channels::BuildingProgress> {
        let root = self.city_root.clone();
        buildings_of(&root)
            .into_iter()
            .map(|addr| {
                let reading = self.plans.of(&root, &addr);
                channels::BuildingProgress {
                    addr,
                    progress: reading.progress,
                    problems: reading.problems,
                    blocked: reading.blocked,
                    ready: u32::try_from(reading.ready.len()).unwrap_or(u32::MAX),
                }
            })
            .collect()
    }

    /// What each pursuit is doing, as the city reads it.
    ///
    /// The verdict is computed here rather than on the page, so the stop
    /// condition has one authority: a client that worked out for itself
    /// whether a city had finished would be the second.
    fn pursuit_lines(&mut self) -> Vec<channels::PursuitLine> {
        let root = self.city_root.clone();
        let held: Vec<(Address, String, kernel::PursuitState)> = self
            .pursuits
            .iter()
            .map(|(addr, (goal, state))| (addr.clone(), goal.clone(), *state))
            .collect();
        let in_flight = u32::try_from(self.hot.active_count()).unwrap_or(u32::MAX);
        let mut out = Vec::new();
        for (addr, goal, state) in held {
            let ready = self.plans.of(&root, &addr).ready;
            out.push(channels::PursuitLine {
                goal,
                state,
                verdict: verdict_line(kernel::observe_pursuit(state, &ready, in_flight)),
                addr,
            });
        }
        out
    }

    /// What this city is called: what its first record says, and for a
    /// city made before that record carried a name, the directory it
    /// lives in. One place decides, so two readers cannot disagree.
    pub(crate) fn city(&self) -> Option<Address> {
        self.city.clone().or_else(|| city_address(&self.city_root))
    }
}

/// The settings page's read of the endpoint book.
fn endpoints_answer(book: &gateway::EndpointBook) -> channels::EndpointsAnswer {
    let endpoints = book
        .endpoints()
        .map(|endpoint| channels::EndpointSummary {
            name: endpoint.name.clone(),
            base_url: endpoint.base_url.clone(),
            dialect: endpoint.dialect,
            models: endpoint.models.clone(),
            local: endpoint.is_local(),
            has_credential: endpoint.has_credential(),
        })
        .collect();
    let chosen = book
        .choices()
        .map(|(tag, endpoint, entry)| channels::ChosenSummary {
            tag,
            endpoint: endpoint.to_owned(),
            model: entry.id.clone(),
            max_output_tokens: entry.max_output_tokens,
        })
        .collect();
    channels::EndpointsAnswer { endpoints, chosen }
}

/// One clause a person reads: what the city is doing about its pursuit.
///
/// The one wording, so the page, the console and a log line all say the
/// same thing about the same verdict.
fn verdict_line(verdict: kernel::PursuitVerdict) -> String {
    match verdict {
        kernel::PursuitVerdict::Work { next } => format!("working on {next}"),
        kernel::PursuitVerdict::Waiting { in_flight } => {
            format!("waiting for {in_flight} run(s) still going")
        }
        kernel::PursuitVerdict::Paused => "paused".to_owned(),
        kernel::PursuitVerdict::Finished => {
            "finished: nothing is ready and nobody is working".to_owned()
        }
    }
}

/// What one `pursuit_changed` record says.
///
/// `None` for a record this build cannot read as one, which a view skips
/// rather than inventing a goal for.
pub(crate) fn pursuit_from(
    record: &EventRecord,
) -> Option<(Address, Option<(String, kernel::PursuitState)>)> {
    let map = record.data().as_map();
    let addr = record.addr()?.clone();
    let step = map.get("step")?.as_str()?;
    let goal = map
        .get("goal")
        .and_then(serde_json::Value::as_str)
        .unwrap_or_default()
        .to_owned();
    let held = match step {
        "set" => Some((goal, kernel::PursuitState::Running)),
        "pause" => Some((goal, kernel::PursuitState::Paused)),
        "resume" => Some((goal, kernel::PursuitState::Running)),
        "clear" => None,
        _ => return None,
    };
    Some((addr, held))
}

/// Every building the city has, in reading order.
///
/// The plans themselves are `crate::plan_view`'s: reading them here as
/// well would be a second parse of the same file, and the two would
/// disagree the first time one of them was invalidated and the other was
/// not.
fn buildings_of(city_root: &Path) -> Vec<Address> {
    let mut found = city::buildings(city_root).unwrap_or_default();
    found.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    found
}

impl Views {
    /// Folds one record into every view that cares about it.
    ///
    /// # Errors
    /// Propagates a view's own refusal to fold a malformed record.
    pub(crate) fn apply(&mut self, record: &EventRecord) -> Result<(), AxError> {
        self.hot
            .apply(record)
            .map_err(memory::MemoryError::into_ax)?;
        self.attribution
            .apply(record)
            .map_err(memory::MemoryError::into_ax)?;
        self.book.apply(record)?;
        self.plans.apply(record);
        self.events = self.events.saturating_add(1);
        match record.kind() {
            EventKind::CityInitialized => {
                self.city = record.addr().cloned();
            }
            EventKind::SignalEnqueued => {
                if let Some((room, line)) = signal_line(record) {
                    self.waiting.entry(room).or_default().push(line);
                }
            }
            EventKind::SignalConsumed => {
                if let Some((room, line)) = signal_line(record)
                    && let Some(queue) = self.waiting.get_mut(&room)
                {
                    queue.retain(|held| held.id != line.id);
                }
            }
            EventKind::PursuitChanged => {
                if let Some((addr, held)) = pursuit_from(record) {
                    match held {
                        Some(entry) => {
                            self.pursuits.insert(addr, entry);
                        }
                        None => {
                            self.pursuits.remove(&addr);
                        }
                    }
                }
            }
            EventKind::FileDiscarded => {
                for line in discard_lines(record) {
                    self.discards.insert(line.path.clone(), line);
                }
            }
            EventKind::DiscardRestored => {
                for line in discard_lines(record) {
                    if let Some(held) = self.discards.get_mut(&line.path) {
                        held.restored = true;
                    }
                }
            }
            EventKind::AssetArchived => {
                if let Some(line) = registry_line(record) {
                    self.assets.push(line);
                }
            }
            EventKind::ApprovalRequested => {
                // The payload *is* the item: it was written by serialising
                // one, so it reads back as one. Rebuilding a lesser shape
                // out of hand-picked fields is how this view came to show
                // every waiting item as "(no summary recorded)" - the field
                // it read had never been written by anybody.
                let value = serde_json::Value::Object(record.data().as_map().clone());
                let item: kernel::ApprovalItem = serde_json::from_value(value).map_err(|err| {
                    AxError::failure(
                        AxCode::WireMismatch,
                        "fold an approval into the queue",
                        format!("seq {}: {err}", record.seq().value()),
                    )
                    .with_recovery(
                        "the record stands; this view skips it and the observer reports it",
                    )
                })?;
                self.approvals.insert(item.id.as_str().to_owned(), item);
            }
            EventKind::ApprovalResolved => {
                if let Some(id) = record
                    .data()
                    .as_map()
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                {
                    self.approvals.remove(id);
                }
            }
            _ => {}
        }
        Ok(())
    }

    /// A bounded slice of the one history, ending just before `before`
    /// or at the tail.
    ///
    /// Read from the ledger rather than held: a view that kept the
    /// records would be a second copy of the only history, and the index
    /// already maps a sequence to a byte offset. An unreadable line ends
    /// the slice rather than emptying it - what was read is still true.
    fn history(&mut self, before: Option<kernel::Seq>, limit: u32) -> channels::HistoryAnswer {
        let empty = channels::HistoryAnswer {
            records: Vec::new(),
            earlier: None,
        };
        let dir = ledger_dir(&self.city_root);
        if self.index.refresh(&dir).is_err() {
            return empty;
        }
        let Some(tail) = self.index.tail_seq() else {
            return empty;
        };
        let end = match before {
            None => tail,
            // The record just before the oldest one the caller holds.
            // `Seq::FIRST` is zero and it is the genesis line, so a
            // `before` of it has nothing behind it - and the arithmetic
            // that says so must not also make the genesis unreachable,
            // which is what a floor of one did.
            Some(seq) => match seq.value().checked_sub(1) {
                None => return empty,
                Some(value) => kernel::Seq::new(value),
            },
        };
        let want = u64::from(limit.clamp(1, channels::HISTORY_MAX));
        let start = end.value().saturating_sub(want.saturating_sub(1));
        let mut records = Vec::new();
        let mut reader = self.index.reader(&dir);
        for value in start..=end.value() {
            let Ok(line) = reader.line_at(kernel::Seq::new(value)) else {
                break;
            };
            let Ok(record) = EventRecord::parse_line(&line) else {
                break;
            };
            records.push(record);
        }
        channels::HistoryAnswer {
            records,
            earlier: (start > kernel::Seq::FIRST.value()).then(|| kernel::Seq::new(start)),
        }
    }

    /// One session's slice of the history, ending just before `before`.
    ///
    /// One bound, not two. The index knows which sequences this run
    /// wrote, so the newest `limit` of them are named before a single
    /// line is read and the work is proportional to the answer rather
    /// than to the ledger. What used to need a second bound - a client
    /// naming a session that ended a month ago and making the server
    /// walk the whole history to find out - is no longer reachable, so
    /// the second bound is gone rather than merely unused.
    ///
    /// The lines are then read oldest first, which is both the order the
    /// answer is delivered in and the order the cursor walks without a
    /// seek. A line that will not read ends the slice rather than
    /// emptying it - what was read is still true.
    fn run_history(
        &mut self,
        run: kernel::RunId,
        before: Option<kernel::Seq>,
        limit: u32,
    ) -> channels::HistoryAnswer {
        let empty = channels::HistoryAnswer {
            records: Vec::new(),
            earlier: None,
        };
        let dir = ledger_dir(&self.city_root);
        if self.index.refresh(&dir).is_err() {
            return empty;
        }
        let want = usize::try_from(limit.clamp(1, channels::HISTORY_MAX)).unwrap_or(1);
        // One more than was asked for: whether this session wrote
        // anything older is exactly what `earlier` reports, and taking
        // one extra sequence answers it without a second question.
        let mut newest: Vec<kernel::Seq> = self
            .index
            .run_seqs_before(run, before)
            .take(want.saturating_add(1))
            .collect();
        let has_older = newest.len() > want;
        newest.truncate(want);
        // The oldest record handed back, so the next question asks for
        // what is strictly before it and the two pages meet exactly.
        let earlier = has_older.then(|| newest.last().copied()).flatten();
        newest.reverse();
        let mut records = Vec::with_capacity(newest.len());
        let mut reader = self.index.reader(&dir);
        for seq in newest {
            let Ok(line) = reader.line_at(seq) else {
                break;
            };
            let Ok(record) = EventRecord::parse_line(&line) else {
                break;
            };
            records.push(record);
        }
        channels::HistoryAnswer { records, earlier }
    }

    /// Answers one query. Every arm either answers or names itself
    /// unavailable; none of them returns an empty result that a reader
    /// would mistake for an empty city.
    pub(crate) fn answer(&mut self, query: &channels::Query) -> channels::Answer {
        match query {
            channels::Query::CityView => {
                let runs: Vec<channels::RunSummary> = self
                    .hot
                    .runs()
                    .map(|(run, hot)| summarize(*run, hot))
                    .collect();
                let active = self.hot.active_count();
                let frozen = self.hot.frozen_count();
                channels::Answer::City(channels::CityAnswer {
                    runs,
                    active,
                    frozen,
                    buildings: self.spine(),
                    pursuits: self.pursuit_lines(),
                })
            }
            channels::Query::RunView { run } => {
                channels::Answer::Run(self.hot.get(run).map(|hot| summarize(*run, hot)))
            }
            channels::Query::ApprovalQueue => {
                channels::Answer::Approvals(channels::ApprovalsAnswer {
                    items: self.approvals.values().cloned().collect(),
                })
            }
            channels::Query::CostView => {
                let report = self.attribution.report();
                channels::Answer::Cost(Box::new(channels::CostAnswer {
                    total: report.total,
                    by_run: report.by_run,
                    by_actor: report.by_actor,
                    by_segment: report.by_segment,
                    by_tool: report.by_tool,
                    by_skill: report.by_skill,
                }))
            }
            channels::Query::History { before, limit } => {
                channels::Answer::History(Box::new(self.history(*before, *limit)))
            }
            channels::Query::RunHistory { run, before, limit } => {
                channels::Answer::History(Box::new(self.run_history(*run, *before, *limit)))
            }
            // A checkpoint this city does not hold is `Unavailable`, not
            // an empty change list: "nothing moved" and "I could not
            // look" are different answers and a reader acts differently
            // on each.
            channels::Query::Changes { base, head } => {
                let far = match head {
                    Some(oid) => memory::Head::Commit(*oid),
                    None => memory::Head::WorkingTree,
                };
                match memory::between(&self.city_root, *base, far) {
                    Ok(files) => channels::Answer::Changes(channels::ChangesAnswer {
                        base: *base,
                        head: *head,
                        files,
                    }),
                    Err(_) => channels::Answer::Unavailable {
                        query: format!("Changes({base})"),
                    },
                }
            }
            channels::Query::EndpointView => {
                channels::Answer::Endpoints(endpoints_answer(&self.book))
            }
            channels::Query::BuildingView { addr } => {
                let root = self.city_root.clone();
                let plan = self.plans.of(&root, addr);
                match read_building(&root, addr, plan) {
                    Some(answer) => channels::Answer::Building(Box::new(answer)),
                    // A building nobody raised is not an empty building. The
                    // page needs to be able to tell those apart.
                    None => channels::Answer::Unavailable {
                        query: format!("BuildingView({})", addr.as_str()),
                    },
                }
            }
            channels::Query::InboxView { addr } => channels::Answer::Inbox(channels::InboxAnswer {
                addr: addr.clone(),
                waiting: self.waiting.get(addr).cloned().unwrap_or_default(),
            }),
            channels::Query::DiscardView => channels::Answer::Discards(channels::DiscardAnswer {
                rows: self.discards.values().cloned().collect(),
            }),
            channels::Query::RegistryView => channels::Answer::Registry(channels::RegistryAnswer {
                assets: self.assets.clone(),
            }),
            channels::Query::ArchiveSearch { needle } => {
                channels::Answer::Archive(self.search_archives(needle))
            }
            channels::Query::Metrics => {
                channels::Answer::Metrics(Box::new(channels::MetricsAnswer {
                    events: self.events,
                    runs_active: self.hot.active_count(),
                    runs_frozen: self.hot.frozen_count(),
                    buildings: u64::try_from(buildings_of(&self.city_root).len())
                        .unwrap_or(u64::MAX),
                    approvals_waiting: u64::try_from(self.approvals.len()).unwrap_or(u64::MAX),
                    signals_waiting: self
                        .waiting
                        .values()
                        .map(|queue| u64::try_from(queue.len()).unwrap_or(u64::MAX))
                        .sum(),
                    discards_outstanding: self
                        .discards
                        .values()
                        .filter(|row| !row.restored)
                        .count()
                        .try_into()
                        .unwrap_or(u64::MAX),
                }))
            }
        }
    }

    /// Every archive entry whose subject contains `needle`, across every
    /// building, read from the shelves at the moment of asking.
    fn search_archives(&self, needle: &str) -> channels::ArchiveAnswer {
        let mut hits = Vec::new();
        let wanted = needle.to_lowercase();
        for building in buildings_of(&self.city_root) {
            let Ok(entries) = city::archive_index(&self.city_root, &building) else {
                continue;
            };
            for entry in entries {
                if !entry.subject.to_lowercase().contains(&wanted) {
                    continue;
                }
                hits.push(channels::ArchiveHit {
                    building: building.clone(),
                    kind: entry.kind.as_str().to_owned(),
                    day: entry.day,
                    subject: entry.subject,
                });
            }
        }
        channels::ArchiveAnswer {
            needle: needle.to_owned(),
            hits,
        }
    }
}

/// One signal, as a room's queue would show it. `None` for a record
/// this version cannot read as a signal: a view skips what it cannot
/// read rather than inventing a row for it.
fn signal_line(record: &EventRecord) -> Option<(Address, channels::SignalLine)> {
    let map = record.data().as_map();
    let text = |key: &str| {
        map.get(key)
            .and_then(serde_json::Value::as_str)
            .map(str::to_owned)
    };
    let room = Address::parse(&text("room")?).ok()?;
    Some((
        room,
        channels::SignalLine {
            id: text("id")?,
            kind: text("kind").unwrap_or_else(|| "signal".to_owned()),
            from: text("from").unwrap_or_default(),
            at: record.t(),
        },
    ))
}

/// The rows one discard record states. A record carries the paths it
/// discarded and one restoration per path.
fn discard_lines(record: &EventRecord) -> Vec<channels::DiscardLine> {
    let map = record.data().as_map();
    // The plan travels as itself. It was written by serialising a
    // `Restoration`, so it reads back as one; a scheme this build cannot
    // name comes through as `None` and the row still appears.
    let restoration = map
        .get("restoration")
        .cloned()
        .and_then(|way| serde_json::from_value::<channels::Restoration>(way).ok());
    map.get("paths")
        .and_then(serde_json::Value::as_array)
        .map(|paths| {
            paths
                .iter()
                .filter_map(|path| {
                    Some(channels::DiscardLine {
                        path: path.as_str()?.to_owned(),
                        restoration: restoration.clone(),
                        at: record.t(),
                        restored: false,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn registry_line(record: &EventRecord) -> Option<channels::RegistryLine> {
    let map = record.data().as_map();
    let text = |key: &str| map.get(key).and_then(serde_json::Value::as_str);
    Some(channels::RegistryLine {
        addr: record.addr().cloned()?,
        kind: text("kind").unwrap_or("fact").to_owned(),
        subject: text("subject").unwrap_or_default().to_owned(),
        at: record.t(),
    })
}

fn summarize(run: RunId, hot: &memory::RunHot) -> channels::RunSummary {
    channels::RunSummary {
        run,
        who: hot.who.clone(),
        frozen: matches!(hot.phase, memory::RunPhase::Frozen),
        last_seq: hot.last_seq,
        last_kind: hot.last_kind,
    }
}
