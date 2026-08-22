// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The documents a building keeps its long work in, and the job file a
//! run starts from.
//!
//! Three of the four spine documents are laid out here — `Roadmap.md`,
//! `Memo.md`, `Handoff.md`. The fourth, `BUILDING.md`, is written where
//! its meaning lives (`crate::building`, read by `crate::policy`): one
//! file, one writer.
//!
//! Nothing here overwrites. A building that has been working keeps its
//! plan, its decisions and its handoff, whatever is run against it
//! afterwards. The one file that is rewritten is `JOB.md`, because it
//! holds the task of the current session rather than a record of past
//! ones — and the record of past ones is in the ledger, where a rewrite
//! cannot reach it.

use std::path::{Path, PathBuf};

use kernel::{Address, AxCode, AxError};

use crate::policy::building_path;

/// The plan: the single denominator for progress in a building.
pub const ROADMAP_FILE: &str = "Roadmap.md";
/// Decisions and corrections.
pub(crate) const MEMO_FILE: &str = "Memo.md";
/// What the next agent needs before it starts.
pub(crate) const HANDOFF_FILE: &str = "Handoff.md";
/// The task of one session, in the room it is run from.
pub const JOB_FILE: &str = "JOB.md";
/// The city's own instructions, read into every prefix.
pub const CITY_FILE: &str = "City.md";

const ROADMAP_TEMPLATE: &str = include_str!("../../../docs/templates/Roadmap.md");
const MEMO_TEMPLATE: &str = include_str!("../../../docs/templates/Memo.md");
const HANDOFF_TEMPLATE: &str = include_str!("../../../docs/templates/Handoff.md");
const NAME_PLACEHOLDER: &str = "<building name>";

/// What a dispatch knows about the work when the job file is written.
pub struct JobBrief<'a> {
    /// One line: what to produce.
    pub task: &'a str,
    /// What counts as success, what counts as failure, when to stop.
    pub goal: &'a str,
    /// The ceilings this run works under, in the caller's own words.
    pub budget: &'a str,
}

/// Lays out the spine documents a building starts with.
///
/// # Errors
/// Propagates a directory that cannot be created or written.
pub(crate) fn lay_out(building_root: &Path, addr: &Address) -> Result<(), AxError> {
    std::fs::create_dir_all(building_root).map_err(|err| storage(building_root, &err))?;
    let name = addr.as_str();
    write_new(
        &building_root.join(ROADMAP_FILE),
        &empty_roadmap(&ROADMAP_TEMPLATE.replace(NAME_PLACEHOLDER, name)),
    )?;
    write_new(
        &building_root.join(MEMO_FILE),
        &MEMO_TEMPLATE.replace(NAME_PLACEHOLDER, name),
    )?;
    write_new(
        &building_root.join(HANDOFF_FILE),
        &HANDOFF_TEMPLATE.replace(NAME_PLACEHOLDER, name),
    )?;
    Ok(())
}

/// Where the job file of a run at `addr` lives.
#[must_use]
pub fn job_path(city_root: &Path, addr: &Address) -> PathBuf {
    let mut path = city_root.to_path_buf();
    for segment in addr.as_str().split('/') {
        path.push(segment);
    }
    path.push(JOB_FILE);
    path
}

/// Writes the job file for one run and returns the bytes written, so the
/// caller can record the same text as history without reading the file
/// back and hoping it is unchanged.
///
/// # Errors
/// Propagates a room that cannot be created or written.
pub fn write_job(
    city_root: &Path,
    addr: &Address,
    brief: &JobBrief<'_>,
) -> Result<String, AxError> {
    let path = job_path(city_root, addr);
    if let Some(room) = path.parent() {
        std::fs::create_dir_all(room).map_err(|err| storage(room, &err))?;
    }
    let mut text = format!(
        "# {JOB_FILE} — {}\n\n> The task for this session. Read it in full and leave it \
         unchanged.\n\n## Task\n\n{}\n\n## Goal\n\n{}\n",
        brief.task, brief.task, brief.goal
    );
    if !brief.budget.is_empty() {
        text.push_str(&format!("\n## Budget\n\n{}\n", brief.budget));
    }
    std::fs::write(&path, text.as_bytes()).map_err(|err| storage(&path, &err))?;
    Ok(text)
}

/// The norm documents a run at `addr` is aligned by, in reading order:
/// how the city works, then how this building works.
///
/// The progress documents (`Roadmap.md`, `Memo.md`, the last frozen
/// point) are not here — they are what a run reads to know where the
/// work stands, and they are named per run, not per address.
///
/// # Errors
/// Propagates the reserved-subtree refusal: an address with no building
/// has no building norms.
pub fn norms(city_root: &Path, addr: &Address) -> Result<Vec<PathBuf>, AxError> {
    let building = crate::building::Building::of(addr)?;
    let mut out = vec![city_root.join(CITY_FILE)];
    let rules = building_path(city_root, building.addr());
    if rules.exists() {
        out.push(rules);
    }
    Ok(out)
}

/// The template's example rows are for a person reading the template. A
/// building that starts with them starts with tasks nobody asked for,
/// and they would count in the denominator.
fn empty_roadmap(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for line in text.lines() {
        if !is_placeholder_row(line) {
            out.push_str(line);
            out.push('\n');
        }
    }
    out
}

fn is_placeholder_row(line: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.starts_with('|') {
        return false;
    }
    let cells: Vec<&str> = trimmed
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .collect();
    match (cells.len(), cells.first(), cells.get(1)) {
        (4, Some(index), Some(item)) => index.parse::<u64>().is_ok() && item.is_empty(),
        _ => false,
    }
}

fn write_new(path: &Path, text: &str) -> Result<(), AxError> {
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
    {
        Ok(mut handle) => std::io::Write::write_all(&mut handle, text.as_bytes())
            .map_err(|err| storage(path, &err)),
        // A document that is already there is the building's own work.
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Ok(()),
        Err(err) => Err(storage(path, &err)),
    }
}

fn storage(path: &Path, err: &std::io::Error) -> AxError {
    AxError::failure(
        AxCode::StorageFatal,
        "lay out a building's documents",
        format!("{}: {err}", path.display()),
    )
    .with_recovery("fix the path's permissions, then run this again")
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
    use crate::policy::BUILDING_FILE;
    use kernel::{Progress, RoadmapShape, check_roadmap_shape, tally};

    fn addr(raw: &str) -> Address {
        Address::parse(raw).unwrap()
    }

    fn roadmap_of(root: &Path) -> String {
        std::fs::read_to_string(root.join(ROADMAP_FILE)).unwrap()
    }

    #[test]
    fn a_new_building_has_a_denominator_and_it_is_zero() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("lab");
        lay_out(&root, &addr("lab")).unwrap();

        let RoadmapShape::WellFormed { rows } = check_roadmap_shape(&roadmap_of(&root)) else {
            panic!("a laid-out roadmap parses");
        };
        assert!(
            rows.is_empty(),
            "the template's example rows are not this building's tasks"
        );
        let Progress::Planned(planned) = tally(&rows) else {
            panic!("a building with a roadmap has a denominator");
        };
        assert_eq!(planned.ratio(), (0, 0));
    }

    #[test]
    fn the_documents_name_the_building_they_belong_to() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("lab");
        lay_out(&root, &addr("lab")).unwrap();
        for file in [ROADMAP_FILE, MEMO_FILE, HANDOFF_FILE] {
            let text = std::fs::read_to_string(root.join(file)).unwrap();
            assert!(text.contains("lab"), "{file} names its building");
            assert!(
                !text.contains(NAME_PLACEHOLDER),
                "{file} has no placeholder"
            );
        }
    }

    #[test]
    fn a_building_that_has_been_working_keeps_its_plan() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("lab");
        lay_out(&root, &addr("lab")).unwrap();
        let worked = "| # | Item | Status | Evidence |\n|---|---|---|---|\n\
                      | 1 | ship it | Done | cas:b3-x |\n";
        std::fs::write(root.join(ROADMAP_FILE), worked).unwrap();

        lay_out(&root, &addr("lab")).unwrap();
        assert_eq!(roadmap_of(&root), worked);
    }

    #[test]
    fn the_job_file_lands_in_the_room_and_says_what_the_run_was_asked_for() {
        let dir = tempfile::tempdir().unwrap();
        let room = addr("lab/room1");
        let text = write_job(
            dir.path(),
            &room,
            &JobBrief {
                task: "measure the thing",
                goal: "a number with a unit, then stop",
                budget: "24 turns",
            },
        )
        .unwrap();

        let path = job_path(dir.path(), &room);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), text);
        assert!(path.ends_with(JOB_FILE));
        assert!(text.contains("measure the thing"));
        assert!(text.contains("a number with a unit, then stop"));
        assert!(text.contains("24 turns"));
    }

    #[test]
    fn a_second_job_replaces_the_first_because_it_is_this_sessions_task() {
        let dir = tempfile::tempdir().unwrap();
        let room = addr("lab/room1");
        let brief = |task| JobBrief {
            task,
            goal: "stop when done",
            budget: "",
        };
        write_job(dir.path(), &room, &brief("first")).unwrap();
        let second = write_job(dir.path(), &room, &brief("second")).unwrap();

        let on_disk = std::fs::read_to_string(job_path(dir.path(), &room)).unwrap();
        assert_eq!(on_disk, second);
        assert!(!on_disk.contains("first"));
        assert!(
            !on_disk.contains("## Budget"),
            "a section with no fact behind it is not written"
        );
    }

    #[test]
    fn the_norms_are_the_citys_and_the_buildings_in_that_order() {
        let dir = tempfile::tempdir().unwrap();
        let room = addr("lab/room1");
        std::fs::write(dir.path().join(CITY_FILE), "# City.md\n").unwrap();

        let before = norms(dir.path(), &room).unwrap();
        assert_eq!(before.len(), 1, "a building with no rules contributes none");

        crate::building::create(dir.path(), &addr("lab"), crate::BuildingTemplate::Minimal)
            .unwrap();
        let after = norms(dir.path(), &room).unwrap();
        assert_eq!(after.len(), 2);
        assert!(after[0].ends_with(CITY_FILE));
        assert!(after[1].ends_with(BUILDING_FILE));
        assert!(after[1].starts_with(dir.path().join("lab")));
    }

    #[test]
    fn the_reserved_subtree_has_no_norms_to_read() {
        let dir = tempfile::tempdir().unwrap();
        let err = norms(dir.path(), &addr(".sprawling/ledger")).unwrap_err();
        assert_eq!(err.code(), &AxCode::InvalidArgs);
    }
}
