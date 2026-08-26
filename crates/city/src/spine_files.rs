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
pub const HANDOFF_FILE: &str = "Handoff.md";
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

/// What one session was given to work from.
///
/// Exhaustive, and the two arms are different situations rather than a
/// present and an absent value: a session either carries out a task
/// somebody wrote down, or works with the person directly and takes the
/// work from the conversation. The prefix says which, because an agent
/// told to read a job file that nobody wrote spends its first turn
/// looking for it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RunBrief {
    /// The job file's own text, as it was written to the room.
    Job { text: String },
    /// Nobody assigned this session a task: the person is here.
    Principal,
}

/// What the prefix says when there is no job file.
///
/// It states the situation rather than the absence, because "there is no
/// JOB.md" describes the disk and what the agent needs is what to do
/// instead.
const PRINCIPAL_BRIEF: &str = "No task file was written for this session. You are working with the \
     person directly: what to do arrives in the conversation, and you \
     answer there.\n";

impl RunBrief {
    /// The bytes this brief contributes to the prefix's run segment.
    #[must_use]
    pub fn segment_text(&self) -> &str {
        match self {
            RunBrief::Job { text } => text,
            RunBrief::Principal => PRINCIPAL_BRIEF,
        }
    }
}

/// Lays down this session's brief and answers which of the two it is.
///
/// A stated goal is what makes a task a job: the file's one irreplaceable
/// section says when to stop, and a form with that field blank teaches an
/// agent that stopping is undefined. So a dispatch that states a goal
/// gets a job file, and one that does not is a conversation.
///
/// **The brief is this dispatch's, never what an earlier session left in
/// the room.** A job file from last week is still on disk and can still
/// be read; what it may not do is present itself as the task of a session
/// nobody assigned one to.
///
/// # Errors
/// Propagates a room that cannot be created or written.
pub fn write_brief(
    city_root: &Path,
    addr: &Address,
    brief: &JobBrief<'_>,
) -> Result<RunBrief, AxError> {
    if brief.goal.trim().is_empty() {
        return Ok(RunBrief::Principal);
    }
    let text = write_job(city_root, addr, brief)?;
    Ok(RunBrief::Job { text })
}

/// Where a building's handoff lives.
///
/// The one place this path is spelled, for the same reason `job_path`
/// and `roadmap_path` are.
#[must_use]
pub fn handoff_path(city_root: &Path, building_addr: &Address) -> PathBuf {
    let mut path = city_root.to_path_buf();
    for segment in building_addr.as_str().split('/') {
        path.push(segment);
    }
    path.push(HANDOFF_FILE);
    path
}

/// What the last session in this building left for the next one.
///
/// `None` says one thing only: there is nothing here worth carrying -
/// either no file, or a form still holding the template's own
/// parenthetical guidance, which in the prefix costs the same bytes as a
/// filled one and carries nothing. A file that exists and cannot be read
/// is a third fact and is reported, because a prefix that quietly omits
/// it tells the next session there was no handoff.
///
/// # Errors
/// `E_STORAGE_FATAL` naming the path, for every failure except a file
/// that is not there.
pub fn handoff(city_root: &Path, building_addr: &Address) -> Result<Option<String>, AxError> {
    let path = handoff_path(city_root, building_addr);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => {
            return Err(AxError::failure(
                AxCode::StorageFatal,
                "read a building's handoff",
                format!("{}: {err}", path.display()),
            )
            .with_recovery(
                "the next session is assembled from this file; \
                 make it readable, then dispatch again",
            ));
        }
    };
    if is_blank_form(&text) {
        return Ok(None);
    }
    Ok(Some(text))
}

/// Whether a handoff is still the form it was laid out as.
///
/// The test is the form's own parenthetical guidance: every section of
/// the template carries one, and a session that wrote the file replaced
/// them with what it found.
fn is_blank_form(text: &str) -> bool {
    let filled = text
        .lines()
        .filter(|line| {
            let trimmed = line.trim();
            !trimmed.is_empty()
                && !trimmed.starts_with('#')
                && !trimmed.starts_with('>')
                && !(trimmed.starts_with('(') && trimmed.ends_with(')'))
        })
        .count();
    filled == 0
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

/// Where a building's plan lives.
///
/// The one place this path is spelled. A caller that joins
/// `city_root/<addr>/Roadmap.md` for itself becomes a second authority
/// for where the plan is, and it keeps working after the real one moves.
#[must_use]
pub fn roadmap_path(city_root: &Path, building_addr: &Address) -> PathBuf {
    let mut path = city_root.to_path_buf();
    for segment in building_addr.as_str().split('/') {
        path.push(segment);
    }
    path.push(ROADMAP_FILE);
    path
}

/// A building's plan as it stands, or an empty document when the
/// building has not been given one yet.
///
/// "Not laid out yet" and "could not be read" are different facts, and
/// only the first one means an empty plan. Everything else - a directory
/// where the file belongs, a permission this process does not have, a
/// device that stopped answering - is reported, because a caller that
/// reads those as an empty plan goes on to tell somebody their claim
/// lost a race that never ran.
///
/// # Errors
/// `E_STORAGE_FATAL` naming the path, for every failure except a file
/// that is not there.
pub fn roadmap(city_root: &Path, building_addr: &Address) -> Result<String, AxError> {
    let path = roadmap_path(city_root, building_addr);
    match std::fs::read_to_string(&path) {
        Ok(text) => Ok(text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(String::new()),
        Err(err) => Err(AxError::failure(
            AxCode::StorageFatal,
            "read a building's plan",
            format!("{}: {err}", path.display()),
        )
        .with_recovery(
            "the plan is shared ground and a run may not claim on it unread; \
             make the file readable, then dispatch again",
        )),
    }
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
    fn a_stated_goal_is_what_makes_a_task_a_job() {
        let dir = tempfile::tempdir().unwrap();
        let room = addr("lab/room1");
        let brief = write_brief(
            dir.path(),
            &room,
            &JobBrief {
                task: "measure the thing",
                goal: "a number with a unit, then stop",
                budget: "24 turns",
            },
        )
        .unwrap();

        let RunBrief::Job { text } = &brief else {
            panic!("a dispatch that says when to stop is a job");
        };
        assert!(text.contains("a number with a unit, then stop"));
        assert_eq!(
            std::fs::read_to_string(job_path(dir.path(), &room)).unwrap(),
            *text,
            "the brief in the prefix and the file in the room are the same bytes"
        );
    }

    /// A session nobody assigned a goal to is a conversation, and no job
    /// file is written for it: a form whose one irreplaceable field is
    /// blank teaches an agent that stopping is undefined.
    #[test]
    fn a_session_with_no_goal_is_the_person_and_leaves_no_job_file() {
        let dir = tempfile::tempdir().unwrap();
        let room = addr("lab/room1");
        for empty in ["", "   ", "\n"] {
            let brief = write_brief(
                dir.path(),
                &room,
                &JobBrief {
                    task: "what do you make of this",
                    goal: empty,
                    budget: "24 turns",
                },
            )
            .unwrap();
            assert_eq!(brief, RunBrief::Principal);
            assert!(
                !job_path(dir.path(), &room).exists(),
                "{empty:?} wrote a job file anyway"
            );
        }
        assert!(
            RunBrief::Principal.segment_text().contains("person"),
            "the brief says what to do instead, not that a file is missing"
        );
    }

    /// The prefix pays for every byte it carries, and a handoff nobody
    /// filled in carries nothing.
    #[test]
    fn an_unfilled_handoff_is_not_worth_prefix_bytes() {
        let dir = tempfile::tempdir().unwrap();
        let root = dir.path().join("lab");
        let lab = addr("lab");
        assert_eq!(
            handoff(dir.path(), &lab).unwrap(),
            None,
            "no building, no handoff"
        );

        lay_out(&root, &lab).unwrap();
        assert_eq!(
            handoff(dir.path(), &lab).unwrap(),
            None,
            "the blank form is the absence of a handoff, not a handoff"
        );

        let written = "# Handoff — lab\n\n## 1 Must-read list\n\nRead the wire spec first.\n";
        std::fs::write(root.join(HANDOFF_FILE), written).unwrap();
        assert_eq!(handoff(dir.path(), &lab).unwrap().as_deref(), Some(written));

        // A third fact, and it is neither of the two above: the file is
        // there and cannot be read.
        let unreadable = dir.path().join("unreadable");
        std::fs::create_dir_all(unreadable.join("lab").join(HANDOFF_FILE)).unwrap();
        let err = handoff(&unreadable, &lab)
            .expect_err("a handoff that cannot be read is not an absent handoff");
        assert!(err.to_string().contains(HANDOFF_FILE), "{err}");
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
