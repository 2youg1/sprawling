// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Which room a named session works in, and how a new one comes into
//! being (city-SPEC.md section 8-13).
//!
//! A room is where one session keeps its files, which is what
//! `ARCHITECTURE.md` section 6 has always said of `JOB.md`. What was
//! missing is that nothing opened a room for a session that did not name
//! one, so every dispatch a person typed by hand landed in the same
//! address and the second one wrote over the first one's work.

use std::path::Path;

use kernel::{Address, AxCode, AxError, SessionName};

use crate::archive::ARCHIVE_DIR;

/// How many suffixed names are tried before a person is asked to pick
/// another word. High enough that nobody meets it by working, low
/// enough that a wrong loop stops rather than fills a disk.
const SUFFIX_LIMIT: u32 = 999;

/// The rooms this building has, in address order.
///
/// One authority for what counts as a room, because there were three:
/// the building page walked the directory itself, and so did two
/// city-level readers. A room is a direct subdirectory whose name an
/// address can hold, which is exactly how rooms come into being -
/// [`open`] and delegation both create one level down. Dot directories
/// are not rooms (that is what keeps the reserved subtree out), and
/// neither is the archive, which is where a building keeps what it
/// remembers rather than somebody to talk to.
///
/// A building with no directory yet has no rooms; that is an answer
/// rather than a failure.
///
/// # Errors
/// Propagates a directory that exists and cannot be read. A caller that
/// would rather show what it could read says so at its own call site.
pub fn all(city_root: &Path, building: &Address) -> Result<Vec<Address>, AxError> {
    let root = city_root.join(building.as_str());
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let entries = std::fs::read_dir(&root).map_err(|err| {
        AxError::failure(
            AxCode::StorageFatal,
            "list the rooms of a building",
            format!("{}: {err}", root.display()),
        )
        .with_recovery("check the building directory is readable")
    })?;
    let mut out = Vec::new();
    for entry in entries.flatten() {
        if !entry.path().is_dir() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name == ARCHIVE_DIR {
            continue;
        }
        if let Ok(addr) = Address::parse(&format!("{}/{name}", building.as_str())) {
            out.push(addr);
        }
    }
    // Two machines reading one directory must answer the same thing;
    // `read_dir` order is the filesystem's, not ours.
    out.sort_by(|left: &Address, right: &Address| left.as_str().cmp(right.as_str()));
    Ok(out)
}

/// Opens a room under `building` for a session called `name`.
///
/// A name that is already taken is suffixed rather than reused:
/// `refactor`, then `refactor-2`. Reusing it would put two sessions that
/// share nothing but a common word into one set of files, which is the
/// defect this module exists to remove. Continuing an earlier session is
/// dispatching to the room it already has, and the interface offers that
/// as a choice rather than as a coincidence of spelling.
///
/// # Errors
/// Refuses a name that cannot follow the building as one address
/// segment, and a building whose rooms are all taken up to the suffix
/// limit. Propagates a directory that cannot be created.
pub fn open(city_root: &Path, building: &Address, name: &SessionName) -> Result<Address, AxError> {
    for attempt in 1..=SUFFIX_LIMIT {
        let candidate = if attempt == 1 {
            format!("{}/{}", building.as_str(), name.as_str())
        } else {
            format!("{}/{}-{attempt}", building.as_str(), name.as_str())
        };
        let addr = Address::parse(&candidate)?;
        let dir = city_root.join(addr.as_str());
        // `create_dir` rather than exists-then-create: the question and
        // the answer are one operation, so two dispatches in the same
        // millisecond cannot both be told the name was free.
        match std::fs::create_dir_all(dir.parent().unwrap_or(city_root))
            .and_then(|()| std::fs::create_dir(&dir))
        {
            Ok(()) => return Ok(addr),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => {
                return Err(AxError::failure(
                    AxCode::StorageFatal,
                    "open a room for this session",
                    format!("{}: {err}", dir.display()),
                )
                .with_recovery("check the city directory is writable"));
            }
        }
    }
    Err(AxError::failure(
        AxCode::InvalidArgs,
        "open a room for this session",
        name.as_str().to_owned(),
    )
    .with_recovery(
        "that name and its first 999 suffixes are taken; give this session another word",
    ))
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
    use super::open;
    use kernel::{Address, SessionName};

    fn named(raw: &str) -> SessionName {
        SessionName::parse(raw).unwrap()
    }

    #[test]
    fn a_session_gets_the_room_it_is_named_after() {
        let dir = tempfile::tempdir().unwrap();
        let lab = Address::parse("lab").unwrap();
        let first = open(dir.path(), &lab, &named("refactor")).unwrap();
        assert_eq!(first.as_str(), "lab/refactor");
        assert!(dir.path().join("lab").join("refactor").is_dir());
    }

    /// Two sessions that happen to share a word do not share a room:
    /// that is the overwriting this module was written to stop.
    #[test]
    fn the_same_word_twice_is_two_rooms() {
        let dir = tempfile::tempdir().unwrap();
        let lab = Address::parse("lab").unwrap();
        let first = open(dir.path(), &lab, &named("test")).unwrap();
        let second = open(dir.path(), &lab, &named("test")).unwrap();
        assert_eq!(first.as_str(), "lab/test");
        assert_eq!(second.as_str(), "lab/test-2");
        assert_ne!(first, second);
    }

    /// A room opened beside a directory that is not a room of ours -
    /// the building's own reserved subtree - does not collide with it.
    #[test]
    fn a_buildings_reserved_subtree_is_not_in_the_way() {
        let dir = tempfile::tempdir().unwrap();
        let lab = Address::parse("lab").unwrap();
        std::fs::create_dir_all(dir.path().join("lab").join(kernel::RESERVED_PREFIX)).unwrap();
        let room = open(dir.path(), &lab, &named("notes")).unwrap();
        assert_eq!(room.as_str(), "lab/notes");
        assert!(!room.is_reserved());
    }
}
