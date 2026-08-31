// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Standing identity. A resident is an address plus the file that says
//! how it works, and that file is the resident segment of every prefix it
//! runs under — so two residents with different URBANITE files solve the
//! same task differently, which is the point rather than a defect.
//!
//! The dossier is a projection, not a second file: what a resident has
//! done is already in the ledger, and a stored summary beside it would be
//! a second account of the same past.

use std::path::{Path, PathBuf};

use kernel::{Address, AxCode, AxError, B3Hash, EventKind, EventRecord, RunId, Seq};

/// The file a resident is described by, at the resident's own address.
pub const URBANITE_FILE: &str = "URBANITE.md";

/// What a prefix's resident segment says when nobody has written an
/// URBANITE.md. Ephemeral workers run under this: no standing identity,
/// and the text says so rather than pretending to a character.
const EPHEMERAL_SEGMENT: &str = "You have no standing identity. Finish the task in JOB.md and report; \
     nothing about you outlives this run.\n";

/// Standing identity, or the absence of one. Exhaustive: a run is either
/// carried out by someone the city knows or by a worker it does not.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Identity {
    Resident(Resident),
    Ephemeral { addr: Address },
}

impl Identity {
    /// Loads the identity at `addr`. An address with an URBANITE.md is a
    /// resident; one without is ephemeral. Reading no file is not a
    /// failure — most rooms hold no standing identity.
    ///
    /// # Errors
    /// Propagates a file that exists and cannot be read, because a
    /// resident whose description is unreadable must not silently become
    /// an ephemeral one with different behaviour.
    pub fn load(city_root: &Path, addr: &Address) -> Result<Identity, AxError> {
        let path = urbanite_path(city_root, addr);
        match std::fs::read(&path) {
            Ok(bytes) => Ok(Identity::Resident(Resident::new(addr.clone(), bytes))),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                Ok(Identity::Ephemeral { addr: addr.clone() })
            }
            Err(err) => Err(AxError::failure(
                AxCode::StorageFatal,
                "read a resident description",
                format!("{}: {err}", path.display()),
            )
            .with_recovery("fix the file's permissions, or remove it to run as ephemeral")),
        }
    }

    /// The bytes this identity contributes as the prefix's resident
    /// segment. Byte-stable for as long as the file is: the same resident
    /// produces the same segment on every run, which is what makes the
    /// prefix cacheable across a resident's whole life.
    #[must_use]
    pub fn segment_bytes(&self) -> Vec<u8> {
        match self {
            Identity::Resident(resident) => resident.urbanite.clone(),
            Identity::Ephemeral { .. } => EPHEMERAL_SEGMENT.as_bytes().to_vec(),
        }
    }

    #[must_use]
    pub fn addr(&self) -> &Address {
        match self {
            Identity::Resident(resident) => &resident.addr,
            Identity::Ephemeral { addr } => addr,
        }
    }

    /// Who the ledger records as the actor.
    #[must_use]
    pub fn who(&self) -> String {
        self.addr().as_str().to_owned()
    }
}

/// A standing identity and the description it is known by.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Resident {
    addr: Address,
    urbanite: Vec<u8>,
    digest: B3Hash,
}

impl Resident {
    fn new(addr: Address, urbanite: Vec<u8>) -> Resident {
        let digest = B3Hash::digest(&urbanite);
        Resident {
            addr,
            urbanite,
            digest,
        }
    }

    #[must_use]
    pub fn addr(&self) -> &Address {
        &self.addr
    }

    /// The description's content hash. Two runs whose resident digests
    /// differ read different instructions, and that difference is visible
    /// without diffing the text.
    #[must_use]
    pub fn digest(&self) -> B3Hash {
        self.digest
    }
}

/// Where a resident's description lives.
#[must_use]
pub fn urbanite_path(city_root: &Path, addr: &Address) -> PathBuf {
    let mut path = city_root.to_path_buf();
    for segment in addr.as_str().split('/') {
        path.push(segment);
    }
    path.push(URBANITE_FILE);
    path
}

/// What a resident has done, folded from the ledger.
///
/// This is a projection: delete it and rebuild from the same events and
/// the answer is identical. Nothing here is stored beside the ledger,
/// because a dossier on disk would be a second account of the same past.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Dossier {
    runs_started: u32,
    runs_frozen: u32,
    last_seq: Option<Seq>,
    last_run: Option<RunId>,
}

impl Dossier {
    #[must_use]
    pub fn new() -> Dossier {
        Dossier::default()
    }

    /// Folds one record in, keeping only records this resident acted in.
    pub fn apply(&mut self, who: &str, record: &EventRecord) {
        if record.who() != who {
            return;
        }
        match record.kind() {
            EventKind::RunStarted => self.runs_started = self.runs_started.saturating_add(1),
            EventKind::RunFrozen => self.runs_frozen = self.runs_frozen.saturating_add(1),
            _ => {}
        }
        self.last_seq = Some(record.seq());
        self.last_run = Some(record.run());
    }

    #[must_use]
    pub fn runs_started(&self) -> u32 {
        self.runs_started
    }

    #[must_use]
    pub fn runs_frozen(&self) -> u32 {
        self.runs_frozen
    }

    #[must_use]
    pub fn last_seq(&self) -> Option<Seq> {
        self.last_seq
    }

    #[must_use]
    pub fn last_run(&self) -> Option<RunId> {
        self.last_run
    }

    /// A resident is live when a run of theirs started and has not been
    /// frozen. Counted rather than flagged: a flag would need someone to
    /// clear it, and nobody clears a flag after a crash.
    #[must_use]
    pub fn is_live(&self) -> bool {
        self.runs_started > self.runs_frozen
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
    use kernel::{EventDraft, Payload, TimeMs};

    fn record(who: &str, kind: EventKind, seq: u64) -> EventRecord {
        EventRecord::from_draft(
            EventDraft {
                run: RunId::from_bytes([5; 16]),
                t: TimeMs::new(seq),
                who: who.to_owned(),
                addr: None,
                kind,
                data: Payload::empty(),
                ig: false,
            },
            Seq::new(seq),
            B3Hash::digest(b"prev"),
        )
    }

    #[test]
    fn an_address_with_a_description_is_a_resident_and_one_without_is_ephemeral() {
        let dir = tempfile::tempdir().unwrap();
        let addr = Address::parse("lab/room1").unwrap();
        std::fs::create_dir_all(dir.path().join("lab").join("room1")).unwrap();
        std::fs::write(
            urbanite_path(dir.path(), &addr),
            "# URBANITE.md\n\nWrites the failing test first.\n",
        )
        .unwrap();

        let identity = Identity::load(dir.path(), &addr).unwrap();
        let Identity::Resident(resident) = &identity else {
            panic!("an address with a description holds a resident");
        };
        assert_eq!(resident.addr(), &addr);
        assert!(identity.segment_bytes().starts_with(b"# URBANITE.md"));

        let empty = Address::parse("lab/room2").unwrap();
        let ephemeral = Identity::load(dir.path(), &empty).unwrap();
        assert!(matches!(ephemeral, Identity::Ephemeral { .. }));
        assert!(
            ephemeral
                .segment_bytes()
                .starts_with(b"You have no standing identity"),
            "an ephemeral worker is told so, rather than given a character"
        );
    }

    #[test]
    fn a_resident_segment_is_byte_stable_across_runs() {
        let dir = tempfile::tempdir().unwrap();
        let addr = Address::parse("lab/room1").unwrap();
        std::fs::create_dir_all(dir.path().join("lab").join("room1")).unwrap();
        std::fs::write(urbanite_path(dir.path(), &addr), "same every time\n").unwrap();

        let first = Identity::load(dir.path(), &addr).unwrap();
        let second = Identity::load(dir.path(), &addr).unwrap();
        assert_eq!(first.segment_bytes(), second.segment_bytes());
        assert_eq!(first, second);

        // Editing the description changes the digest, which is what makes
        // "this run read different instructions" visible without a diff.
        std::fs::write(urbanite_path(dir.path(), &addr), "now different\n").unwrap();
        let third = Identity::load(dir.path(), &addr).unwrap();
        assert_ne!(first.segment_bytes(), third.segment_bytes());
    }

    #[test]
    fn a_dossier_counts_only_this_residents_runs_and_survives_the_run_that_made_it() {
        let mut dossier = Dossier::new();
        dossier.apply("lab/room1", &record("lab/room1", EventKind::RunStarted, 1));
        dossier.apply("lab/room1", &record("lab/room2", EventKind::RunStarted, 2));
        assert_eq!(dossier.runs_started(), 1, "another room's run is not mine");
        assert!(dossier.is_live());

        dossier.apply("lab/room1", &record("lab/room1", EventKind::RunFrozen, 3));
        assert!(!dossier.is_live(), "the run ended; the identity did not");
        assert_eq!(dossier.runs_frozen(), 1);

        dossier.apply("lab/room1", &record("lab/room1", EventKind::RunStarted, 4));
        assert_eq!(
            dossier.runs_started(),
            2,
            "a resident crosses runs; that is what makes it standing"
        );
        assert_eq!(dossier.last_seq(), Some(Seq::new(4)));
    }
}
