// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! Who a run can reach, and who stands there (city-SPEC.md section
//! 8-15).
//!
//! This building has speech and had no address book. `signal` takes the
//! address of whoever you are talking to, the refusal names the boundary
//! rather than the residents inside it, and delivery opens a queue for
//! whatever address it is given - so a guessed name returned
//! `queued: true` and was read by nobody. What is scanned here is the
//! answer to the question that guess was standing in for.
//!
//! **Detail decays with distance.** This building's addresses come with
//! the line each resident wrote about what to bring them; the rest of
//! the city comes as building names and nothing else. That is not a new
//! rule - it is the boundary `signal` already draws, made visible, so
//! that what a run can see and what it can say to are the same set.

use std::path::Path;

use kernel::{Address, AxError};

use crate::resident::Identity;

/// The heading `docs/templates/URBANITE.md` asks a resident to write the
/// kind of work that belongs with them under. A roster's one line is
/// taken from there when it exists, because "why would I go to them" is
/// exactly what that section answers.
const BRING_HEADING: &str = "## Bring them";

/// Who stands at an address. Exhaustive: either somebody with a standing
/// identity lives there, or the place is open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Occupancy {
    /// A standing identity, and the one line it offers whoever is
    /// deciding what to bring it. Empty when the resident's description
    /// says nothing about itself, which is the author's choice rather
    /// than a defect.
    Resident { bring: String },
    /// A place with nobody standing in it: somewhere to send an
    /// ephemeral worker, or to move somebody into.
    Empty,
}

/// One address a run can reach, occupied or not.
///
/// An empty room is a neighbour in the sense that matters here: it is a
/// place this run can name. Leaving it out would read as "this address
/// does not exist", and the address is exactly what `delegate` takes.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Neighbour {
    pub addr: Address,
    /// The last segment of the address, which is the word a person typed
    /// when they made the place.
    pub name: String,
    pub occupancy: Occupancy,
    /// Signals waiting in that room, as they stood when this run was
    /// dispatched.
    ///
    /// Not "is somebody running there": in a city that drives one run at
    /// a time, the answer is always no, and a column that is always the
    /// same word teaches nothing. What a waiting count says is worth
    /// knowing - somebody has already spoken to them and they have not
    /// answered yet, so a second question may be one too many.
    pub waiting: u32,
}

/// What one run can see of the city around it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Neighbourhood {
    building: Address,
    here: Vec<Neighbour>,
    buildings: Vec<Address>,
}

impl Neighbourhood {
    /// Reads the neighbourhood of a run at `me`, inside `building`.
    ///
    /// The building's own address is a place like any other - a resident
    /// may stand at the building root - so it is scanned alongside the
    /// rooms. `me` is left out: a neighbour is somebody else.
    ///
    /// `waiting` answers how deep each room's queue is. It is handed in
    /// rather than read: the queues are the assembly layer's, folded
    /// from the ledger, and this crate cannot see that far. Everything
    /// else here is a function of the disk.
    ///
    /// A snapshot taken at dispatch is as current as anything can be. The
    /// city drives one run at a time, and a signal this run sends is
    /// delivered after it freezes, so no queue moves under it.
    ///
    /// # Errors
    /// Propagates a directory that exists and cannot be read, and a
    /// resident description that exists and cannot be read. The second
    /// one matters: `Identity::load` refuses to downgrade an unreadable
    /// description to "nobody lives here", and a roster that did would
    /// quietly report a resident as an empty room.
    pub fn scan(
        city_root: &Path,
        building: &Address,
        me: &Address,
        waiting: &dyn Fn(&Address) -> u32,
    ) -> Result<Neighbourhood, AxError> {
        let mut here = Vec::new();
        let mut places = vec![building.clone()];
        places.extend(crate::room::all(city_root, building)?);
        for addr in places {
            if &addr == me {
                continue;
            }
            let identity = Identity::load(city_root, &addr)?;
            let occupancy = match identity {
                Identity::Resident(_) => Occupancy::Resident {
                    bring: bring_line(&String::from_utf8_lossy(&identity.segment_bytes())),
                },
                Identity::Ephemeral { .. } => Occupancy::Empty,
            };
            here.push(Neighbour {
                name: name_of(&addr).to_owned(),
                waiting: waiting(&addr),
                addr,
                occupancy,
            });
        }
        Ok(Neighbourhood {
            building: building.clone(),
            here,
            buildings: crate::building::all(city_root)?,
        })
    }

    #[must_use]
    pub fn building(&self) -> &Address {
        &self.building
    }

    /// Every address in this building except this run's own, in address
    /// order.
    #[must_use]
    pub fn here(&self) -> &[Neighbour] {
        &self.here
    }

    /// The city's buildings by name, this one included: a run that could
    /// not see the building it is in would have no way to read the list
    /// as a map.
    #[must_use]
    pub fn buildings(&self) -> &[Address] {
        &self.buildings
    }

    /// How many of the addresses here have somebody standing at them.
    ///
    /// People rather than places, because this is the number `status`
    /// reports and an empty room has no reader. Saturating is the
    /// documented answer for a building with more residents than a `u32`
    /// can count, which no disk holds.
    #[must_use]
    pub fn residents(&self) -> u32 {
        let count = self
            .here
            .iter()
            .filter(|neighbour| matches!(neighbour.occupancy, Occupancy::Resident { .. }))
            .count();
        u32::try_from(count).unwrap_or(u32::MAX)
    }
}

/// What a place is called: the last segment of its address, which is
/// the word a person typed when they made it. The whole address reads
/// like a path; the last segment reads like somebody.
fn name_of(addr: &Address) -> &str {
    addr.as_str().rsplit('/').next().unwrap_or(addr.as_str())
}

/// The one line a roster shows for a resident.
///
/// The `## Bring them` section first, and the first line of prose only
/// when that section is absent or empty. This is deliberately not
/// `library::first_line`, which takes a holding's *title*: a shelf entry
/// is named by its heading and a resident is described by their prose,
/// so the two documents are read by two rules that live in two places.
fn bring_line(text: &str) -> String {
    if let Some((_, tail)) = text.split_once(BRING_HEADING)
        && let Some(line) = section_line(tail)
    {
        return line;
    }
    first_prose(text).unwrap_or_default()
}

/// The first line of a section, or nothing when the section is empty.
/// A heading ends it: reading past one would report the next section's
/// content as this one's.
fn section_line(tail: &str) -> Option<String> {
    for line in tail.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }
        if trimmed.starts_with('#') {
            return None;
        }
        return Some(trimmed.to_owned());
    }
    None
}

/// The first line of prose in a document: not a heading, and not a
/// blockquote. The template's guidance to the author is a blockquote,
/// and showing it would give every unedited resident the same sentence
/// about themselves.
fn first_prose(text: &str) -> Option<String> {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty() && !line.starts_with('#') && !line.starts_with('>'))
        .map(str::to_owned)
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
    use std::path::PathBuf;

    fn city() -> tempfile::TempDir {
        tempfile::tempdir().unwrap()
    }

    fn addr(raw: &str) -> Address {
        Address::parse(raw).unwrap()
    }

    fn resident_at(root: &Path, at: &str, urbanite: &str) {
        let dir: PathBuf = root.join(at);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(crate::resident::URBANITE_FILE), urbanite).unwrap();
    }

    fn empty_room(root: &Path, at: &str) {
        std::fs::create_dir_all(root.join(at)).unwrap();
    }

    #[test]
    fn a_run_sees_the_others_in_its_building_and_not_itself() {
        let dir = city();
        let root = dir.path();
        resident_at(
            root,
            "lab/mason",
            "# URBANITE.md\n\n## Bring them\n\nA kiln that will not fire.\n",
        );
        resident_at(
            root,
            "lab/scribe",
            "# URBANITE.md\n\nReads twice before writing once.\n",
        );
        empty_room(root, "lab/store");
        resident_at(root, "market/broker", "# URBANITE.md\n\nPrices things.\n");

        let seen = Neighbourhood::scan(root, &addr("lab"), &addr("lab/mason"), &|_| 0).unwrap();
        let names: Vec<&str> = seen
            .here()
            .iter()
            .map(|neighbour| neighbour.addr.as_str())
            .collect();
        assert_eq!(
            names,
            vec!["lab", "lab/scribe", "lab/store"],
            "the building root is a place too, and a run is not its own neighbour"
        );
        assert_eq!(seen.residents(), 1, "only scribe stands anywhere here");
        assert_eq!(
            seen.here()[1].occupancy,
            Occupancy::Resident {
                bring: "Reads twice before writing once.".to_owned()
            },
            "with no `Bring them` section the first line of prose stands in"
        );
        assert_eq!(seen.here()[2].occupancy, Occupancy::Empty);
        assert_eq!(seen.here()[2].name, "store");
    }

    #[test]
    fn the_rest_of_the_city_is_names_and_nothing_else() {
        let dir = city();
        let root = dir.path();
        resident_at(root, "lab/mason", "# URBANITE.md\n\nFires kilns.\n");
        resident_at(root, "market/broker", "# URBANITE.md\n\nPrices things.\n");
        std::fs::create_dir_all(root.join(kernel::RESERVED_PREFIX)).unwrap();

        let seen = Neighbourhood::scan(root, &addr("lab"), &addr("lab/mason"), &|_| 0).unwrap();
        let buildings: Vec<&str> = seen
            .buildings()
            .iter()
            .map(kernel::Address::as_str)
            .collect();
        assert_eq!(
            buildings,
            vec!["lab", "market"],
            "the city's own account is not a building"
        );
        assert!(
            !seen
                .here()
                .iter()
                .any(|n| n.addr.as_str().starts_with("market")),
            "another building's residents are not in reach and so are not shown"
        );
    }

    /// The reserved subtree and the archive are directories inside a
    /// building that nobody stands in; listing them as places to talk to
    /// would send signals into the building's own filing.
    #[test]
    fn a_buildings_own_filing_is_not_a_neighbour() {
        let dir = city();
        let root = dir.path();
        empty_room(root, "lab/notes");
        std::fs::create_dir_all(root.join("lab").join(kernel::RESERVED_PREFIX)).unwrap();
        std::fs::create_dir_all(root.join("lab").join(crate::archive::ARCHIVE_DIR)).unwrap();

        let seen = Neighbourhood::scan(root, &addr("lab"), &addr("lab/notes"), &|_| 0).unwrap();
        let names: Vec<&str> = seen
            .here()
            .iter()
            .map(|neighbour| neighbour.addr.as_str())
            .collect();
        assert_eq!(names, vec!["lab"], "one place, and it is the building root");
    }

    #[test]
    fn the_line_comes_from_bring_them_when_the_resident_wrote_one() {
        let filled = "# URBANITE.md — mason\n\n> guidance the author left in place\n\n## Who\n\nA potter.\n\n## Bring them\n\nAnything that has to survive a firing.\n";
        assert_eq!(bring_line(filled), "Anything that has to survive a firing.");

        let unsectioned = "# URBANITE.md\n\n> guidance\n\nAsks rather than guesses.\n";
        assert_eq!(
            bring_line(unsectioned),
            "Asks rather than guesses.",
            "the template's guidance is not a description of anybody"
        );

        let sectionless_but_headed = "# URBANITE.md\n\n## Bring them\n\n## Who\n\nA potter.\n";
        assert_eq!(
            bring_line(sectionless_but_headed),
            "A potter.",
            "an empty section falls back rather than borrowing the next one's line"
        );

        assert_eq!(
            bring_line(""),
            "",
            "an empty description is a choice, not a defect"
        );
    }

    /// The heading this module reads is the template's. If the template
    /// renames it and this file does not move, every roster silently
    /// falls back to the first line of prose.
    #[test]
    fn the_shipped_template_still_carries_the_section_this_reads() {
        let template = include_str!("../../../docs/templates/URBANITE.md");
        assert!(
            template.contains(BRING_HEADING),
            "docs/templates/URBANITE.md no longer has {BRING_HEADING}"
        );
    }
}
