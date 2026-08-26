// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! What a building remembers between runs.
//!
//! Four kinds, and the list is closed: a preference somebody stated, a
//! decision somebody made, a correction somebody had to make twice, and
//! a fact about the world that was expensive to find out. Anything that
//! does not fit one of those four is a note, and notes belong in the
//! documents a person already reads.
//!
//! **Recall is structural, not semantic.** An entry is filed by kind and
//! date, the index says where it is, and reading it means reading the
//! original. No vector store, no embedding, no similarity: the recall
//! this needs is "what did we decide about X", and an index answers that
//! without a second copy of the text to drift from the first.
//!
//! The index is a projection. Delete it and it rebuilds from the
//! entries, which is why it may be stored as plainly as it is.

use std::path::{Path, PathBuf};

use kernel::{AxCode, AxError, TimeMs};

/// Where a building's archive sits, relative to the building.
pub const ARCHIVE_DIR: &str = "Archive";

/// The four kinds. Closed on purpose: a fifth would need a reason, and
/// "it did not fit" is the reason a category list rots.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Kind {
    /// How the person wants things done.
    Preference,
    /// What was chosen, and what was chosen against.
    Decision,
    /// Something that went wrong and the way it was put right.
    Correction,
    /// Something true about the world that cost work to establish.
    Fact,
}

impl Kind {
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Kind::Preference => "preference",
            Kind::Decision => "decision",
            Kind::Correction => "correction",
            Kind::Fact => "fact",
        }
    }

    /// # Errors
    /// Refuses a kind outside the four.
    pub fn parse(raw: &str) -> Result<Kind, AxError> {
        match raw {
            "preference" => Ok(Kind::Preference),
            "decision" => Ok(Kind::Decision),
            "correction" => Ok(Kind::Correction),
            "fact" => Ok(Kind::Fact),
            other => Err(AxError::failure(
                AxCode::InvalidArgs,
                "read an archive kind",
                other.to_owned(),
            )
            .with_recovery("preference, decision, correction or fact")),
        }
    }
}

/// One filed entry.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub kind: Kind,
    /// Whole days since the epoch. Days rather than milliseconds because
    /// the index is browsed by a person, and because a stamp with more
    /// precision than the question needs invites comparisons nobody
    /// meant to make.
    pub day: u64,
    /// One line, which is what the index shows.
    pub subject: String,
    /// The file the entry lives in. Reading the entry means reading it.
    pub at: PathBuf,
}

/// Whole days from an injected instant. Time only ever arrives as a
/// parameter here, as everywhere.
#[must_use]
pub fn day_of(at: TimeMs) -> u64 {
    at.value().saturating_div(86_400_000)
}

/// What one entry is and where it goes, decided before anything is
/// written.
///
/// The path is `<building>/Archive/<kind>/<day>-<slug>.md`, which sorts
/// by date inside each kind without an index having to exist yet.
///
/// Separate from [`file`] because the ledger line an entry becomes is
/// built out of `kind`, `day` and `subject` alone: all three are
/// functions of what the caller passed in, so the line can be written
/// before the shelf is touched. That order is the Ledger's own rule,
/// and it is only available to a caller who can name the entry without
/// filing it.
///
/// # Errors
/// Refuses an empty subject: the index line is the only thing most
/// entries are ever read by.
pub fn entry(
    city_root: &Path,
    building: &kernel::Address,
    kind: Kind,
    at: TimeMs,
    subject: &str,
) -> Result<Entry, AxError> {
    if subject.trim().is_empty() {
        return Err(AxError::failure(
            AxCode::InvalidArgs,
            "file an archive entry",
            "an entry with no subject".to_owned(),
        )
        .with_recovery("give it the line you would want to see months from now"));
    }
    let day = day_of(at);
    Ok(Entry {
        kind,
        day,
        subject: subject.trim().to_owned(),
        at: city_root
            .join(building.as_str())
            .join(ARCHIVE_DIR)
            .join(kind.as_str())
            .join(format!("{day}-{}.md", slug(subject))),
    })
}

/// Puts one entry on its building's shelf, at the place [`entry`] chose.
///
/// # Errors
/// Propagates whatever creating the directory or writing the file
/// reports, and refuses an entry whose path has no directory to sit in.
pub fn file(entry: &Entry, body: &str) -> Result<(), AxError> {
    let dir = entry.at.parent().ok_or_else(|| {
        AxError::failure(
            AxCode::InvalidArgs,
            "file an archive entry",
            format!("{} has no directory to sit in", entry.at.display()),
        )
    })?;
    std::fs::create_dir_all(dir).map_err(|err| {
        AxError::failure(
            AxCode::StorageFatal,
            "file an archive entry",
            format!("{}: {err}", dir.display()),
        )
    })?;
    let text = format!("# {}\n\n{body}\n", entry.subject);
    std::fs::write(&entry.at, text.as_bytes()).map_err(|err| {
        AxError::failure(
            AxCode::StorageFatal,
            "file an archive entry",
            format!("{}: {err}", entry.at.display()),
        )
    })
}

/// Everything filed in one building, kind then day then subject.
///
/// This is the index, and it is computed rather than stored: a stored
/// one would be a second account of what is on the disk, and the disk is
/// the one that is true.
///
/// # Errors
/// Propagates a directory that exists and cannot be read.
pub fn index(city_root: &Path, building: &kernel::Address) -> Result<Vec<Entry>, AxError> {
    let root = city_root.join(building.as_str()).join(ARCHIVE_DIR);
    let mut entries = Vec::new();
    if !root.exists() {
        return Ok(entries);
    }
    for kind in [
        Kind::Preference,
        Kind::Decision,
        Kind::Correction,
        Kind::Fact,
    ] {
        let dir = root.join(kind.as_str());
        if !dir.exists() {
            continue;
        }
        let listed = std::fs::read_dir(&dir).map_err(|err| {
            AxError::failure(
                AxCode::StorageFatal,
                "read an archive",
                format!("{}: {err}", dir.display()),
            )
        })?;
        for item in listed {
            let path = item
                .map_err(|err| {
                    AxError::failure(
                        AxCode::StorageFatal,
                        "read an archive",
                        format!("{}: {err}", dir.display()),
                    )
                })?
                .path();
            if path.extension().is_none_or(|ext| ext != "md") {
                continue;
            }
            let text = std::fs::read_to_string(&path).map_err(|err| {
                AxError::failure(
                    AxCode::StorageFatal,
                    "read an archive entry",
                    format!("{}: {err}", path.display()),
                )
            })?;
            let stem = path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default();
            let day = stem
                .split_once('-')
                .and_then(|(day, _)| day.parse::<u64>().ok())
                .unwrap_or_default();
            entries.push(Entry {
                kind,
                day,
                subject: subject_of(&text),
                at: path,
            });
        }
    }
    entries.sort_by(|left, right| {
        left.kind
            .cmp(&right.kind)
            .then(left.day.cmp(&right.day))
            .then(left.subject.cmp(&right.subject))
    });
    Ok(entries)
}

fn subject_of(text: &str) -> String {
    text.lines()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .unwrap_or_default()
        .trim_start_matches(['#', ' '])
        .to_owned()
}

/// A file name from a subject: lowercase, letters and digits, dashes
/// between. Long subjects are cut rather than hashed, because a person
/// looking at the directory should recognise the file.
fn slug(subject: &str) -> String {
    let mut out = String::new();
    for ch in subject.trim().chars() {
        if out.chars().count() >= 48 {
            break;
        }
        if ch.is_ascii_alphanumeric() {
            out.extend(ch.to_lowercase());
        } else if !out.ends_with('-') && !out.is_empty() {
            out.push('-');
        }
    }
    let trimmed = out.trim_end_matches('-').to_owned();
    if trimmed.is_empty() {
        "entry".to_owned()
    } else {
        trimmed
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
    use kernel::Address;

    fn lab() -> Address {
        Address::parse("lab").unwrap()
    }

    #[test]
    fn what_was_filed_is_what_comes_back() {
        let dir = tempfile::tempdir().unwrap();
        let entry = entry(
            dir.path(),
            &lab(),
            Kind::Decision,
            TimeMs::new(1_700_000_000_000),
            "fast-forward only, and why",
        )
        .unwrap();
        file(
            &entry,
            "Because the party who knows whether the work still applies is the one who did it.",
        )
        .unwrap();
        let found = index(dir.path(), &lab()).unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].subject, entry.subject);
        assert_eq!(found[0].kind, Kind::Decision);
        assert_eq!(found[0].day, entry.day);
        let body = std::fs::read_to_string(&found[0].at).unwrap();
        assert!(
            body.contains("the one who did it"),
            "recall is reading the original, not reading a summary of it"
        );
    }

    #[test]
    fn the_index_rebuilds_from_the_entries_and_sorts_the_same_way_twice() {
        let dir = tempfile::tempdir().unwrap();
        for (kind, day, subject) in [
            (Kind::Fact, 3, "the kiln takes six hours"),
            (Kind::Preference, 1, "metric units"),
            (Kind::Fact, 1, "the clay comes from the east pit"),
        ] {
            let entry = entry(
                dir.path(),
                &lab(),
                kind,
                TimeMs::new(day * 86_400_000),
                subject,
            )
            .unwrap();
            file(&entry, "body").unwrap();
        }
        let first = index(dir.path(), &lab()).unwrap();
        let again = index(dir.path(), &lab()).unwrap();
        assert_eq!(first, again);
        let order: Vec<&str> = first.iter().map(|entry| entry.subject.as_str()).collect();
        assert_eq!(
            order,
            vec![
                "metric units",
                "the clay comes from the east pit",
                "the kiln takes six hours"
            ],
            "kind first, then date: preference before fact, and older fact before newer"
        );
    }

    #[test]
    fn a_building_with_no_archive_has_an_empty_index() {
        let dir = tempfile::tempdir().unwrap();
        assert!(index(dir.path(), &lab()).unwrap().is_empty());
    }

    #[test]
    fn the_four_kinds_are_the_four_kinds() {
        for name in ["preference", "decision", "correction", "fact"] {
            assert_eq!(Kind::parse(name).unwrap().as_str(), name);
        }
        let refused = Kind::parse("idea").unwrap_err();
        assert_eq!(refused.code(), &AxCode::InvalidArgs);
        assert!(refused.recovery().contains("correction"));
    }

    #[test]
    fn an_entry_without_a_line_worth_reading_is_refused() {
        let dir = tempfile::tempdir().unwrap();
        assert!(entry(dir.path(), &lab(), Kind::Fact, TimeMs::new(0), "   ").is_err());
    }
}
