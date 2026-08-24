// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! What a drop means, and what it is refused for.
//!
//! **A drop aims work; it does not start it.** Everything a gesture can
//! mean here ends at the control surface with the address filled in and
//! the task written — the person still presses the button, because a
//! drag that spent money would be a gesture nobody could take back. That
//! is the whole rule, and it is what lets the meanings below be so few.
//!
//! **Bytes are not copied.** A file dropped on a building is named, not
//! staged: in a city formed around a folder somebody already works in,
//! that file is already inside the city, and staging a copy would make
//! two authorities for one file. What the drop contributes is the thing
//! a person would otherwise type — where the work goes, and what it is
//! about.
//!
//! A gesture this build cannot name is refused with the reason. There is
//! no arm that guesses.

use channels::{Address, AxCode, AxError};

use crate::lang::{Lang, Msg, say};

/// What was dropped, as a browser can describe it without reading a
/// byte.
///
/// Exhaustive: a drop carries files, or text, or nothing this build can
/// read. The third is a real case — an image dragged between two tabs
/// arrives as neither — and calling it "empty" would be the interface
/// inventing a fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dropped {
    Files(Vec<String>),
    Text(String),
    Unreadable,
}

/// Where it landed.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Target {
    /// A building or a room: both are addresses, and the difference is
    /// the address's own.
    Place(Address),
    /// A run. Not a place: a run is something that happened at an
    /// address, and dropping work onto it has no meaning this build can
    /// carry out.
    Run,
}

/// What the drop means. Exhaustive, and the refusal carries its own
/// reason rather than leaving the interface to compose one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Meaning {
    /// Point the control surface at `addr` and write `task` into it. The
    /// person presses the button.
    Aim {
        addr: Address,
        task: String,
    },
    Refused {
        because: Msg,
    },
}

/// The most file names one dropped task line carries.
///
/// A folder dragged in can hold thousands, and a task line that listed
/// them all would be a task nobody reads. The count is always stated, so
/// what is not listed is visible rather than lost.
pub const NAMED_FILES: usize = 8;

/// Reads one gesture.
///
/// Pure, so what a drag means is decided once and asserted without a
/// browser — which is the only way this is testable at all, since no
/// gate in this repository drives a real one.
#[must_use]
pub fn read(target: &Target, dropped: &Dropped) -> Meaning {
    let Target::Place(addr) = target else {
        return Meaning::Refused {
            because: Msg::DropNotAPlace,
        };
    };
    match dropped {
        Dropped::Unreadable => Meaning::Refused {
            because: Msg::DropUnreadable,
        },
        Dropped::Text(text) if text.trim().is_empty() => Meaning::Refused {
            because: Msg::DropUnreadable,
        },
        Dropped::Text(text) => Meaning::Aim {
            addr: addr.clone(),
            task: text.trim().to_owned(),
        },
        Dropped::Files(names) if names.is_empty() => Meaning::Refused {
            because: Msg::DropUnreadable,
        },
        Dropped::Files(names) => Meaning::Aim {
            addr: addr.clone(),
            task: task_line(names),
        },
    }
}

/// The task a dropped set of files writes.
///
/// It states the count first and then names what fits, so a person
/// reading it knows immediately whether the list is the whole of it.
fn task_line(names: &[String]) -> String {
    let mut listed: Vec<&str> = names.iter().take(NAMED_FILES).map(String::as_str).collect();
    listed.sort_unstable();
    let head = if names.len() == 1 {
        "1 file was dropped here".to_owned()
    } else {
        format!("{} files were dropped here", names.len())
    };
    let rest = names.len().saturating_sub(listed.len());
    let tail = if rest > 0 {
        format!(" and {rest} more")
    } else {
        String::new()
    };
    format!("{head}: {}{tail}", listed.join(", "))
}

/// The refusal a person reads, in the three parts every other refusal in
/// this city has.
///
/// Built here rather than at the drop site so one gesture has one
/// wording wherever it is made.
#[must_use]
pub fn refusal(lang: Lang, because: Msg) -> AxError {
    AxError::failure(
        AxCode::InvalidArgs,
        say(lang, Msg::DropAction).to_owned(),
        say(lang, because).to_owned(),
    )
    .with_recovery(say(lang, Msg::DropRecovery).to_owned())
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

    fn place(addr: &str) -> Target {
        Target::Place(Address::parse(addr).unwrap())
    }

    #[test]
    fn a_file_dropped_on_a_room_aims_the_work_at_that_room() {
        let meaning = read(
            &place("lab/room1"),
            &Dropped::Files(vec!["meter.csv".to_owned()]),
        );
        let Meaning::Aim { addr, task } = meaning else {
            panic!("a file dropped on a room aims work there");
        };
        assert_eq!(addr.as_str(), "lab/room1");
        assert_eq!(task, "1 file was dropped here: meter.csv");
    }

    /// A folder dragged in can hold thousands. What is not listed has to
    /// be visible rather than lost.
    #[test]
    fn a_long_list_says_how_much_of_it_is_not_listed() {
        let names: Vec<String> = (0..20).map(|n| format!("file-{n:02}.txt")).collect();
        let Meaning::Aim { task, .. } = read(&place("lab"), &Dropped::Files(names)) else {
            panic!("files dropped on a building aim work there");
        };
        assert!(task.starts_with("20 files were dropped here: file-00.txt"));
        assert!(task.ends_with("and 12 more"), "{task}");
    }

    #[test]
    fn dropped_text_becomes_the_task_and_nothing_else() {
        let Meaning::Aim { addr, task } = read(
            &place("lab"),
            &Dropped::Text("  measure the thing  ".to_owned()),
        ) else {
            panic!("text dropped on a building aims work there");
        };
        assert_eq!(addr.as_str(), "lab");
        assert_eq!(task, "measure the thing");
    }

    /// The gesture that has no meaning: a run is something that
    /// happened, not a place work can be put.
    #[test]
    fn a_drop_on_a_run_is_refused_and_says_why() {
        let meaning = read(&Target::Run, &Dropped::Text("anything".to_owned()));
        assert_eq!(
            meaning,
            Meaning::Refused {
                because: Msg::DropNotAPlace
            }
        );
        let said = refusal(Lang::En, Msg::DropNotAPlace);
        assert!(
            !said.recovery().is_empty(),
            "a refusal teaches the way round"
        );
    }

    #[test]
    fn a_drop_this_build_cannot_read_is_refused_rather_than_read_as_empty() {
        for dropped in [
            Dropped::Unreadable,
            Dropped::Files(Vec::new()),
            Dropped::Text("   ".to_owned()),
        ] {
            assert_eq!(
                read(&place("lab"), &dropped),
                Meaning::Refused {
                    because: Msg::DropUnreadable
                },
                "{dropped:?} is not an empty task"
            );
        }
    }
}
