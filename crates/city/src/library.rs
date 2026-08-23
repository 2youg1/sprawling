// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The city's central stock of settled work, and the reading room each
//! building takes a subset of it into.
//!
//! The division is the only reason resident context does not grow with
//! the disk. A thousand skills may sit in the library and all of them
//! are findable; **not one byte of them enters a run's prefix** unless
//! that building's reading room admits it, and what a reading room
//! admits is a list a person wrote in `BUILDING.md`.
//!
//! The library lives under the reserved prefix, which is outside every
//! write domain. That is deliberate: a resident may read the stock and
//! may not restock it, so an agent cannot quietly grant itself a skill
//! by writing one.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use kernel::{Address, AxCode, AxError};

/// Where the stock sits, under the reserved prefix.
pub const LIBRARY_DIR: &str = "library";

/// One shelved item: what it is called, which section it sits in, and
/// the one line a catalog would show.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Holding {
    pub name: String,
    pub section: String,
    /// The first non-empty line of the document, which is what the
    /// author wrote to describe it. Taken rather than generated: a
    /// summary of a summary is a digest, and digests are suspect.
    pub disclosure: String,
    pub path: PathBuf,
}

/// The city's stock, by section then name.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Library {
    holdings: BTreeMap<(String, String), Holding>,
}

impl Library {
    /// Reads the stock from disk.
    ///
    /// A city with no library is an empty library, not an error: most
    /// cities start with nothing settled.
    ///
    /// # Errors
    /// Propagates a directory that exists and cannot be read — that is
    /// a broken installation rather than an empty one.
    pub fn scan(city_root: &Path) -> Result<Library, AxError> {
        let root = city_root.join(kernel::RESERVED_PREFIX).join(LIBRARY_DIR);
        let mut holdings = BTreeMap::new();
        if !root.exists() {
            return Ok(Library { holdings });
        }
        for section in read_dir(&root)? {
            if !section.is_dir() {
                continue;
            }
            let section_name = file_name(&section);
            for item in read_dir(&section)? {
                if item.is_dir() || item.extension().is_none_or(|ext| ext != "md") {
                    continue;
                }
                let name = item
                    .file_stem()
                    .and_then(|stem| stem.to_str())
                    .unwrap_or_default()
                    .to_owned();
                if name.is_empty() {
                    continue;
                }
                let text = std::fs::read_to_string(&item).map_err(|err| {
                    AxError::failure(
                        AxCode::StorageFatal,
                        "read a library holding",
                        format!("{}: {err}", item.display()),
                    )
                })?;
                holdings.insert(
                    (section_name.clone(), name.clone()),
                    Holding {
                        name,
                        section: section_name.clone(),
                        disclosure: first_line(&text),
                        path: item,
                    },
                );
            }
        }
        Ok(Library { holdings })
    }

    /// Everything on the shelves, in section then name order.
    #[must_use]
    pub fn all(&self) -> Vec<&Holding> {
        self.holdings.values().collect()
    }

    /// The sections, which is the navigation a person browses by.
    #[must_use]
    pub fn sections(&self) -> Vec<&str> {
        let mut seen: Vec<&str> = self
            .holdings
            .values()
            .map(|holding| holding.section.as_str())
            .collect();
        seen.dedup();
        seen
    }

    /// What one building's reading room admits.
    ///
    /// Matched by name alone, so a person writing the list in
    /// `BUILDING.md` does not have to know which section something was
    /// filed under. A name on the list that is not on the shelves is
    /// simply absent from the result: the catalog shows what a run can
    /// actually reach, and a promise of a missing skill is worse than
    /// its absence.
    #[must_use]
    pub fn reading_room(&self, admitted: &[String]) -> Vec<&Holding> {
        self.holdings
            .values()
            .filter(|holding| admitted.iter().any(|name| name == &holding.name))
            .collect()
    }

    /// The names on a list that the shelves do not have. Shown to the
    /// person who wrote the list, since only they can fix it.
    #[must_use]
    pub fn missing(&self, admitted: &[String]) -> Vec<String> {
        admitted
            .iter()
            .filter(|name| !self.holdings.values().any(|holding| holding.name == **name))
            .cloned()
            .collect()
    }
}

/// Where a holding sits, as an address, for the must-read list and for
/// cost attribution by skill.
///
/// # Errors
/// Propagates an address the city cannot spell.
pub fn holding_address(holding: &Holding) -> Result<Address, AxError> {
    Address::parse(&format!(
        "{}/{}/{}/{}.md",
        kernel::RESERVED_PREFIX.trim_end_matches('/'),
        LIBRARY_DIR,
        holding.section,
        holding.name
    ))
}

fn read_dir(path: &Path) -> Result<Vec<PathBuf>, AxError> {
    let mut out = Vec::new();
    let entries = std::fs::read_dir(path).map_err(|err| {
        AxError::failure(
            AxCode::StorageFatal,
            "read the library",
            format!("{}: {err}", path.display()),
        )
    })?;
    for entry in entries {
        let entry = entry.map_err(|err| {
            AxError::failure(
                AxCode::StorageFatal,
                "read the library",
                format!("{}: {err}", path.display()),
            )
        })?;
        out.push(entry.path());
    }
    out.sort();
    Ok(out)
}

fn file_name(path: &Path) -> String {
    path.file_name()
        .and_then(|name| name.to_str())
        .unwrap_or_default()
        .to_owned()
}

fn first_line(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .trim_start_matches(['#', ' '])
        .to_owned()
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

    fn stocked(root: &Path) {
        let shelves = root.join(kernel::RESERVED_PREFIX).join(LIBRARY_DIR);
        for (section, name, text) in [
            (
                "utilities",
                "unit-tests",
                "# Writing a test that earns its place\n\nbody",
            ),
            ("utilities", "diffing", "How to read a diff\n"),
            ("domain", "kiln-firing", "# Firing schedules\n"),
        ] {
            let dir = shelves.join(section);
            std::fs::create_dir_all(&dir).unwrap();
            std::fs::write(dir.join(format!("{name}.md")), text).unwrap();
        }
    }

    #[test]
    fn a_city_with_no_library_has_an_empty_one_rather_than_an_error() {
        let dir = tempfile::tempdir().unwrap();
        let library = Library::scan(dir.path()).unwrap();
        assert!(library.all().is_empty());
        assert!(library.reading_room(&["anything".to_owned()]).is_empty());
    }

    #[test]
    fn a_building_takes_only_what_its_list_admits() {
        let dir = tempfile::tempdir().unwrap();
        stocked(dir.path());
        let library = Library::scan(dir.path()).unwrap();
        assert_eq!(library.all().len(), 3, "the shelves hold everything");
        let admitted = library.reading_room(&["unit-tests".to_owned(), "diffing".to_owned()]);
        let names: Vec<&str> = admitted.iter().map(|h| h.name.as_str()).collect();
        assert_eq!(
            names,
            vec!["diffing", "unit-tests"],
            "a thousand may sit on the shelf; this building reads two"
        );
    }

    #[test]
    fn the_one_line_is_the_authors_line() {
        let dir = tempfile::tempdir().unwrap();
        stocked(dir.path());
        let library = Library::scan(dir.path()).unwrap();
        let holding = library
            .all()
            .into_iter()
            .find(|h| h.name == "unit-tests")
            .unwrap();
        assert_eq!(holding.disclosure, "Writing a test that earns its place");
        assert_eq!(holding.section, "utilities");
    }

    #[test]
    fn a_name_on_the_list_that_is_not_on_the_shelf_is_reported_to_whoever_wrote_it() {
        let dir = tempfile::tempdir().unwrap();
        stocked(dir.path());
        let library = Library::scan(dir.path()).unwrap();
        let asked = vec!["diffing".to_owned(), "imagined".to_owned()];
        assert_eq!(library.reading_room(&asked).len(), 1);
        assert_eq!(library.missing(&asked), vec!["imagined".to_owned()]);
    }

    #[test]
    fn a_holding_lives_where_no_run_may_write() {
        let dir = tempfile::tempdir().unwrap();
        stocked(dir.path());
        let library = Library::scan(dir.path(), None).unwrap();
        let holding = library.all()[0];
        assert!(
            holding.addr.is_reserved(),
            "a resident may read the stock and may not restock it"
        );
    }

    /// A building keeps the skills only it uses on its own shelf, inside
    /// its own directory, and still cannot restock them.
    #[test]
    fn a_building_keeps_its_own_shelf_and_the_nearer_shelf_wins() {
        let dir = tempfile::tempdir().unwrap();
        stocked(dir.path());
        let lab = Address::parse("lab").unwrap();
        let own = dir
            .path()
            .join("lab")
            .join(kernel::RESERVED_PREFIX)
            .join(BUILDING_SHELF)
            .join("utilities");
        std::fs::create_dir_all(&own).unwrap();
        std::fs::write(own.join("kiln.md"), "Firing a kiln in this lab\n").unwrap();
        std::fs::write(
            own.join("unit-tests.md"),
            "This lab's own rule for tests\n",
        )
        .unwrap();

        let library = Library::scan(dir.path(), Some(&lab)).unwrap();
        let names: Vec<&str> = library.all().iter().map(|h| h.name.as_str()).collect();
        assert!(names.contains(&"kiln"), "{names:?}");
        assert!(names.contains(&"diffing"), "the city's shelf is still read");

        let mine = library
            .all()
            .into_iter()
            .find(|h| h.name == "unit-tests")
            .unwrap();
        assert_eq!(
            mine.disclosure, "This lab's own rule for tests",
            "the nearer shelf wins, as the configuration ladder already does"
        );
        assert!(
            mine.addr.is_reserved(),
            "a building's own shelf is still outside its write domain: {}",
            mine.addr.as_str()
        );

        // Another building sees only the city's shelf.
        let other = Library::scan(dir.path(), Some(&Address::parse("vault").unwrap())).unwrap();
        assert!(!other.all().iter().any(|h| h.name == "kiln"));
    }
}
