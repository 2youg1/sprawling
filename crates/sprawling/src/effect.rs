// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! What a run's desks left behind: the lines the history takes, and the
//! change the city may not make before them.
//!
//! `docs/glossary.md` defines the Ledger as the only history, and says
//! every effect becomes an EventRecord first. ARCHITECTURE.md section 5
//! calls that ordering the design's load-bearing rule rather than a
//! logging preference. It used to be spelled out once per desk inside
//! `dispatch_in`, and two of the six spelled it backwards - the shared
//! plan was written and the shelf was filed before the lines that
//! announce them.
//!
//! Here the order is a property of the types instead. [`Then`] - the
//! change - is reachable only out of [`Landing::record`], and that
//! method appends every line before it hands the change over. Writing
//! the other order means obtaining a value that has no other source.

use std::path::{Path, PathBuf};

use kernel::{Address, AxCode, AxError, EventKind, Payload, TimeMs};

/// One line of the history, attributed to whoever caused it.
///
/// The attribution travels with the line rather than with the batch: a
/// signal is recorded against the room it is going to, and consuming one
/// is recorded against the resident that read it.
pub(crate) struct Line {
    pub(crate) who: String,
    pub(crate) addr: Address,
    pub(crate) kind: EventKind,
    pub(crate) data: Payload,
}

/// What the city does once a desk's lines are on the ledger.
///
/// Every arm is a change somebody can observe without reading the
/// history: a room's queue, the goal register, the shared plan, the
/// building's shelf. That is exactly why none of them may happen first.
pub(crate) enum Then {
    /// The line was the whole of it.
    Nothing,
    /// Put each signal in the room it names, and knock on that door.
    Deliver(Vec<collab::Signal>),
    /// Hold this ground in the city's goal register.
    Hold(Vec<kernel::GoalEntry>),
    /// Write the shared plan back, as the desk left it.
    Roadmap { path: PathBuf, text: String },
    /// Put these on the building's shelf.
    Shelf(Vec<Filing>),
}

/// One entry and the body it carries.
///
/// The entry already names its own place: `city::archive_entry` decides
/// that from the kind, the instant and the subject, none of which needs
/// a disk. That is what lets the line be written first.
pub(crate) struct Filing {
    pub(crate) entry: city::ArchiveEntry,
    pub(crate) body: String,
}

/// Everything one desk left behind, resolved.
///
/// Both fields are private, and `then` leaves only through
/// [`Landing::record`].
pub(crate) struct Landing {
    lines: Vec<Line>,
    then: Then,
}

impl Landing {
    /// Appends every line, then hands back the change that follows them.
    ///
    /// # Errors
    /// Propagates the first line the ledger refuses, and makes no change
    /// at all in that case: the change is the return value, so a caller
    /// that never receives it cannot apply it.
    pub(crate) fn record(
        self,
        append: &mut impl FnMut(Line) -> Result<(), AxError>,
    ) -> Result<Then, AxError> {
        for line in self.lines {
            append(line)?;
        }
        Ok(self.then)
    }

    /// What a run said, and what it read out of its own queue.
    ///
    /// # Errors
    /// Propagates a signal whose payload cannot be built.
    pub(crate) fn signals(
        effects: Vec<collab::SignalEffect>,
        room: &Address,
        who: &str,
    ) -> Result<Landing, AxError> {
        let mut lines = Vec::new();
        let mut deliver = Vec::new();
        for effect in effects {
            match effect {
                collab::SignalEffect::Enqueued(signal) => {
                    lines.push(Line {
                        who: who.to_owned(),
                        addr: signal.room().clone(),
                        kind: EventKind::SignalEnqueued,
                        data: signal.enqueued_payload()?,
                    });
                    deliver.push(signal);
                }
                collab::SignalEffect::Consumed { signal, by } => {
                    let data = signal.consumed_payload(&by)?;
                    lines.push(Line {
                        who: by,
                        addr: room.clone(),
                        kind: EventKind::SignalConsumed,
                        data,
                    });
                }
            }
        }
        Ok(Landing {
            lines,
            then: Then::Deliver(deliver),
        })
    }

    /// What ground a run claimed, and where two claims met.
    ///
    /// # Errors
    /// Propagates an entry or a conflict whose payload cannot be built.
    pub(crate) fn goals(
        effects: Vec<collab::GoalEffect>,
        room: &Address,
        who: &str,
    ) -> Result<Landing, AxError> {
        let mut lines = Vec::new();
        let mut hold = Vec::new();
        for effect in effects {
            match effect {
                collab::GoalEffect::Registered(entry) => {
                    lines.push(Line {
                        who: who.to_owned(),
                        addr: room.clone(),
                        kind: EventKind::GoalRegistered,
                        data: goal_payload(&entry)?,
                    });
                    hold.push(entry);
                }
                collab::GoalEffect::Conflicted { entry, level } => {
                    lines.push(Line {
                        who: who.to_owned(),
                        addr: room.clone(),
                        kind: EventKind::GoalConflict,
                        data: collab::conflict_payload(&entry, &level)?,
                    });
                }
            }
        }
        Ok(Landing {
            lines,
            then: Then::Hold(hold),
        })
    }

    /// What a wave deleted, as the sweep found it. The payloads carry
    /// their own way back, so there is nothing left for the city to do
    /// once they are on the ledger.
    pub(crate) fn discards(payloads: Vec<Payload>, room: &Address, who: &str) -> Landing {
        Landing {
            lines: payloads
                .into_iter()
                .map(|data| Line {
                    who: who.to_owned(),
                    addr: room.clone(),
                    kind: EventKind::FileDiscarded,
                    data,
                })
                .collect(),
            then: Then::Nothing,
        }
    }

    /// What a run asked its building to remember.
    ///
    /// `at` is one stamp for the whole batch rather than one per entry:
    /// a settlement is one moment, and two entries filed by one run
    /// should not be able to land on two different days.
    ///
    /// # Errors
    /// Propagates a kind outside the four, an entry with no subject, and
    /// a payload that cannot be built.
    pub(crate) fn shelf(
        effects: Vec<collab::ArchiveEffect>,
        write_root: &Path,
        building: &Address,
        at: TimeMs,
        room: &Address,
        who: &str,
    ) -> Result<Landing, AxError> {
        let mut lines = Vec::new();
        let mut filings = Vec::new();
        for collab::ArchiveEffect::Recorded { kind, text } in effects {
            let entry = city::archive_entry(
                write_root,
                building,
                city::ArchiveKind::parse(&kind)?,
                at,
                &text,
            )?;
            let mut data = serde_json::Map::new();
            data.insert(
                "kind".to_owned(),
                serde_json::Value::String(entry.kind.as_str().to_owned()),
            );
            data.insert(
                "day".to_owned(),
                serde_json::Value::Number(entry.day.into()),
            );
            data.insert(
                "subject".to_owned(),
                serde_json::Value::String(entry.subject.clone()),
            );
            lines.push(Line {
                who: who.to_owned(),
                addr: room.clone(),
                kind: EventKind::AssetArchived,
                data: Payload::new(data)?,
            });
            filings.push(Filing { entry, body: text });
        }
        Ok(Landing {
            lines,
            then: Then::Shelf(filings),
        })
    }
}

/// What a run's claims on the shared plan came to. Two answers and no
/// third, because the plan is written whole: either every effect still
/// matches the file as it stands and the lines take the rows, or one of
/// them does not and nothing at all is written.
pub(crate) enum Claims {
    Landed(Box<Landing>),
    /// The rows that moved, so a person can be told which.
    Stale(Vec<u64>),
}

impl Claims {
    /// Checks each effect against the file as it stands now rather than
    /// as it stood when the run was dispatched. Today one run is driven
    /// at a time, so the two agree; when they stop agreeing the losing
    /// claim is dropped rather than written over somebody's row.
    ///
    /// # Errors
    /// Propagates a claim whose payload cannot be built.
    pub(crate) fn of(
        effects: &[collab::ClaimEffect],
        on_disk: &str,
        text: String,
        path: PathBuf,
        room: &Address,
        who: &str,
    ) -> Result<Claims, AxError> {
        let stale: Vec<u64> = effects
            .iter()
            .filter(|effect| !collab::still_true(on_disk, effect))
            .map(collab::ClaimEffect::index)
            .collect();
        if !stale.is_empty() {
            return Ok(Claims::Stale(stale));
        }
        let mut lines = Vec::new();
        for effect in effects {
            lines.push(Line {
                who: who.to_owned(),
                addr: room.clone(),
                kind: match effect {
                    collab::ClaimEffect::Claimed { .. } => EventKind::RoadmapClaimed,
                    collab::ClaimEffect::Finished { .. } => EventKind::RoadmapFinished,
                    collab::ClaimEffect::Released { .. } => EventKind::RoadmapReleased,
                },
                data: effect.payload(who)?,
            });
        }
        Ok(Claims::Landed(Box::new(Landing {
            lines,
            then: Then::Roadmap { path, text },
        })))
    }
}

/// The `goal_registered` payload is the entry itself. One shape, written
/// and read here, so a claim reads back as the claim that was made.
///
/// # Errors
/// Refuses an entry that does not serialise to an object.
pub(crate) fn goal_payload(entry: &kernel::GoalEntry) -> Result<Payload, AxError> {
    let value = serde_json::to_value(entry)
        .map_err(|err| AxError::failure(AxCode::InvalidArgs, "record a goal", err.to_string()))?;
    let map = value.as_object().cloned().ok_or_else(|| {
        AxError::failure(AxCode::InvalidArgs, "record a goal", "a goal is an object")
    })?;
    Payload::new(map)
}

/// The other direction of the same shape, which is why it lives beside
/// it: a restarted worker rebuilds its goal register by reading back
/// exactly what this module wrote.
///
/// # Errors
/// Refuses a payload this build's `GoalEntry` cannot be read out of.
pub(crate) fn goal_from_payload(data: &Payload) -> Result<kernel::GoalEntry, AxError> {
    serde_json::from_value(serde_json::Value::Object(data.as_map().clone())).map_err(|err| {
        AxError::failure(
            AxCode::InvalidArgs,
            "read a registered goal",
            err.to_string(),
        )
        .with_recovery("this shape is written by the same binary that reads it; report it")
    })
}
