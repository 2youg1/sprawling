// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! What the city is listening to, and which building answers.
//!
//! The shape is `schedule`'s: a table on disk, a pure question over it,
//! and dispatches as the answer. The difference is where the trigger
//! comes from. A schedule fires because time passed, which the city can
//! observe on its own; a watch fires because something happened
//! elsewhere, which it cannot.
//!
//! So the city does not ask. Polling would be a timer nobody reads,
//! running whether or not anything happened, and it would be a second
//! authority on what arrived first. The service that holds the
//! connection pushes, and this module only decides where a push lands.
//!
//! A building that no longer exists is no longer listening. A building
//! is one line of business; when the line ends the building goes, and
//! its subscriptions go with it rather than needing a second list
//! somebody has to remember to prune.

use std::path::{Path, PathBuf};

use kernel::{Address, AxCode, AxError, TimeMs};
use serde::Deserialize;

/// The city's watch table, at the city root.
pub const WATCH_FILE: &str = "WATCH.toml";

/// One thing the city listens to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Source {
    name: String,
    matches: String,
    addr: Address,
    /// Whether an arrival from here may start work, or only be noticed.
    /// Default is to notice: something arriving from outside is not by
    /// itself a reason to spend a model.
    starts_work: bool,
}

impl Source {
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// The substring that routes an arrival here. Not a pattern
    /// language, for `triage`'s reason: a routing table is read far more
    /// often than it is written, and a regular expression adds one more
    /// thing to debug on the day something goes wrong.
    #[must_use]
    pub fn matches(&self) -> &str {
        &self.matches
    }

    #[must_use]
    pub fn addr(&self) -> &Address {
        &self.addr
    }

    #[must_use]
    pub fn starts_work(&self) -> bool {
        self.starts_work
    }

    /// Which building owns this source — the first path segment.
    #[must_use]
    pub fn building(&self) -> &str {
        self.addr
            .as_str()
            .split('/')
            .next()
            .unwrap_or(self.addr.as_str())
    }
}

/// Whether the city is currently hearing a source, and since when.
///
/// Recorded rather than repaired. A machine or a router that stops
/// working is something a person notices; what the city owes is not a
/// backfill but an honest answer to "when was this building not
/// listening", which needs both edges written down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Link {
    Live { since: TimeMs },
    Down { since: TimeMs },
}

impl Link {
    #[must_use]
    pub fn is_live(&self) -> bool {
        matches!(self, Link::Live { .. })
    }

    #[must_use]
    pub fn since(&self) -> TimeMs {
        match self {
            Link::Live { since } | Link::Down { since } => *since,
        }
    }
}

/// The whole table.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Watch {
    sources: Vec<Source>,
}

impl Watch {
    /// Reads a watch table.
    ///
    /// # Errors
    /// Refuses a file that does not parse, a key this version does not
    /// read, an address that is not one, and an empty match string —
    /// an empty substring matches everything, which is a routing table
    /// with one row written as if it had several.
    pub fn parse(text: &str) -> Result<Watch, AxError> {
        let file: WatchFile = toml::from_str(text).map_err(|err| refuse(err.to_string()))?;
        let mut sources = Vec::new();
        for row in file.source {
            if row.matches.is_empty() {
                return Err(refuse(format!(
                    "{}: an empty match takes everything; say what it takes",
                    row.name
                )));
            }
            let addr = Address::parse(&row.addr)?;
            if addr.is_reserved() {
                return Err(refuse(format!(
                    "{}: the city's own subtree does not answer arrivals",
                    row.name
                )));
            }
            sources.push(Source {
                name: row.name,
                matches: row.matches,
                addr,
                starts_work: row.starts_work,
            });
        }
        Ok(Watch { sources })
    }

    /// Reads the city's watch table. A city that listens to nothing has
    /// no file, which is the ordinary case.
    ///
    /// # Errors
    /// Propagates an unreadable file and a file that does not parse.
    pub fn load(city_root: &Path) -> Result<Watch, AxError> {
        let path = watch_path(city_root);
        match std::fs::read_to_string(&path) {
            Ok(text) => Watch::parse(&text),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(Watch::default()),
            Err(err) => Err(AxError::failure(
                AxCode::StorageFatal,
                "read the city watch table",
                format!("{}: {err}", path.display()),
            )
            .with_recovery("fix the file's permissions; a watch table that exists is read")),
        }
    }

    #[must_use]
    pub fn sources(&self) -> &[Source] {
        &self.sources
    }

    /// The sources whose building still stands.
    ///
    /// `standing` is the list of buildings the city currently has. A
    /// source whose building was demolished is dropped here rather than
    /// deleted from the file: the file is the person's, and a city that
    /// edited it would be answering a question nobody asked.
    #[must_use]
    pub fn listening(&self, standing: &[Address]) -> Vec<&Source> {
        self.sources
            .iter()
            .filter(|source| {
                standing
                    .iter()
                    .any(|building| building.as_str() == source.building())
            })
            .collect()
    }
}

/// Where the watch table lives.
#[must_use]
pub fn watch_path(city_root: &Path) -> PathBuf {
    city_root.join(WATCH_FILE)
}

fn refuse(subject: String) -> AxError {
    AxError::failure(AxCode::ConfigInvalid, "read the city watch table", subject).with_recovery(
        "each `[[source]]` needs `name`, `matches` and `addr`; `starts_work` defaults to false",
    )
}

#[derive(Debug, Default, Deserialize)]
#[serde(deny_unknown_fields)]
struct WatchFile {
    #[serde(default)]
    source: Vec<SourceRow>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct SourceRow {
    name: String,
    matches: String,
    addr: String,
    #[serde(default)]
    starts_work: bool,
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

    const TABLE: &str = "\
[[source]]
name = \"github\"
matches = \"pull request\"
addr = \"lab/room1\"
starts_work = true

[[source]]
name = \"mail\"
matches = \"invoice\"
addr = \"mill/room1\"
";

    #[test]
    fn a_table_says_where_each_arrival_lands_and_whether_it_may_work() {
        let watch = Watch::parse(TABLE).unwrap();
        assert_eq!(watch.sources().len(), 2);
        assert_eq!(watch.sources()[0].addr().as_str(), "lab/room1");
        assert!(watch.sources()[0].starts_work());
        assert!(
            !watch.sources()[1].starts_work(),
            "arriving from outside is not by itself a reason to spend a model"
        );
    }

    #[test]
    fn a_demolished_building_stops_listening_without_a_second_list() {
        let watch = Watch::parse(TABLE).unwrap();
        let standing = vec![Address::parse("lab").unwrap()];
        let live = watch.listening(&standing);
        assert_eq!(live.len(), 1);
        assert_eq!(live[0].name(), "github");
        assert_eq!(
            watch.listening(&[]).len(),
            0,
            "a city with no buildings hears nothing"
        );
    }

    #[test]
    fn an_empty_match_is_refused_where_it_is_written() {
        let err = Watch::parse("[[source]]\nname = \"x\"\nmatches = \"\"\naddr = \"lab/room1\"\n")
            .unwrap_err();
        assert_eq!(err.code(), &AxCode::ConfigInvalid);
        assert!(err.subject().contains("takes everything"));
    }

    #[test]
    fn the_citys_own_subtree_does_not_answer_arrivals() {
        let err = Watch::parse(
            "[[source]]\nname = \"x\"\nmatches = \"y\"\naddr = \".sprawling/ledger\"\n",
        )
        .unwrap_err();
        assert_eq!(err.code(), &AxCode::ConfigInvalid);
    }

    #[test]
    fn a_key_this_version_does_not_read_is_refused_rather_than_ignored() {
        let err = Watch::parse(
            "[[source]]\nname = \"x\"\nmatches = \"y\"\naddr = \"lab/a\"\npoll = 30\n",
        )
        .unwrap_err();
        assert_eq!(err.code(), &AxCode::ConfigInvalid);
        assert!(
            err.recovery().contains("starts_work"),
            "the refusal lists what this version does read"
        );
    }

    #[test]
    fn a_city_that_listens_to_nothing_has_no_file() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(Watch::load(dir.path()).unwrap(), Watch::default());
        std::fs::write(watch_path(dir.path()), TABLE).unwrap();
        assert_eq!(Watch::load(dir.path()).unwrap().sources().len(), 2);
    }

    #[test]
    fn both_edges_of_a_connection_are_facts_with_a_time_on_them() {
        let up = Link::Live {
            since: TimeMs::new(10),
        };
        let down = Link::Down {
            since: TimeMs::new(20),
        };
        assert!(up.is_live() && !down.is_live());
        assert_eq!(down.since(), TimeMs::new(20));
    }
}
