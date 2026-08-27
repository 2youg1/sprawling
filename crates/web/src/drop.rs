// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

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

use channels::{Address, AxCode, AxError, RunId};
use dioxus::html::HasFileData;
use dioxus::prelude::*;

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
    /// The bar work is started from. It already knows where the work
    /// goes, because a person put it there.
    Composer,
    /// A session already running. Dropping here means "look at these",
    /// which is a Steer - it lands in the box and waits for the button,
    /// exactly as every other arm does.
    Run(RunId),
    /// Something this page cannot address. A room name the city will not
    /// parse is not a place, and it is not a session either: saying it
    /// was one is the interface stating a fact that did not happen.
    Nowhere,
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
    /// Write `task` and leave the address alone.
    ///
    /// A separate arm from [`Meaning::Aim`] because the two differ in the
    /// one way that matters: dropping on a building says **where**, and
    /// dropping on the bar says **what**. Folding them together would
    /// make a drop overwrite a destination the person had already
    /// chosen.
    Task {
        task: String,
    },
    /// Write `said` into the session's own box. Not sent: a run already
    /// spending money is exactly where an unrecallable gesture would
    /// cost the most.
    Say {
        run: RunId,
        said: String,
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

/// What a browser will say about a drop, without reading a byte.
///
/// The one thin edge of this module, and it lives here rather than beside
/// each drop zone for the reason the rest of the file exists: a second
/// copy of it would be a second answer to "what was dropped". The names
/// and the text are all this takes, because a file dropped on a city
/// formed around somebody's own folder is already inside that city.
#[must_use]
pub fn from_event(event: &Event<DragData>) -> Dropped {
    let names: Vec<String> = event
        .files()
        .iter()
        .map(dioxus::html::FileData::name)
        .collect();
    if names.is_empty() {
        return Dropped::Unreadable;
    }
    Dropped::Files(names)
}

/// Reads one gesture.
///
/// Pure, so what a drag means is decided once and asserted without a
/// browser — which is the only way this is testable at all, since no
/// gate in this repository drives a real one.
#[must_use]
pub fn read(target: &Target, dropped: &Dropped) -> Meaning {
    // What was dropped is read once, before where it landed is
    // considered: an unreadable drop is unreadable everywhere, and
    // deciding that per target is how four arms become four copies of
    // one rule.
    let Some(written) = written(dropped) else {
        return Meaning::Refused {
            because: Msg::DropUnreadable,
        };
    };
    match target {
        Target::Place(addr) => Meaning::Aim {
            addr: addr.clone(),
            task: written,
        },
        Target::Composer => Meaning::Task { task: written },
        Target::Run(run) => Meaning::Say {
            run: *run,
            said: written,
        },
        Target::Nowhere => Meaning::Refused {
            because: Msg::DropNotAPlace,
        },
    }
}

/// The line a drop writes, or `None` when this build cannot read it.
///
/// One reading for every target, so the four arms above differ only in
/// where the line goes.
fn written(dropped: &Dropped) -> Option<String> {
    match dropped {
        Dropped::Unreadable => None,
        Dropped::Text(text) if text.trim().is_empty() => None,
        Dropped::Text(text) => Some(text.trim().to_owned()),
        Dropped::Files(names) if names.is_empty() => None,
        Dropped::Files(names) => Some(task_line(names)),
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

    /// The gesture that has no meaning: a place this city will not
    /// address is not somewhere work can be put.
    #[test]
    fn a_drop_on_something_unaddressable_is_refused_and_says_why() {
        let meaning = read(&Target::Nowhere, &Dropped::Text("anything".to_owned()));
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

    /// Dropping on the bar says what the work is about. It must not say
    /// where the work goes: the person already answered that, and a
    /// gesture that silently redirected their dispatch would be worse
    /// than one that did nothing.
    #[test]
    fn a_drop_on_the_composer_writes_the_task_and_leaves_the_address_alone() {
        let meaning = read(
            &Target::Composer,
            &Dropped::Files(vec!["meter.csv".to_owned()]),
        );
        assert_eq!(
            meaning,
            Meaning::Task {
                task: "1 file was dropped here: meter.csv".to_owned()
            }
        );
    }

    /// The ruling section 8-38 recorded said a run had no meaning this
    /// build could carry out. The steer box is that meaning, and the
    /// principle it was protecting is untouched: this writes the line and
    /// sends nothing.
    #[test]
    fn a_drop_on_a_session_writes_into_its_box_and_does_not_send() {
        let run = RunId::from_bytes([9u8; 16]);
        let meaning = read(
            &Target::Run(run),
            &Dropped::Files(vec!["lex.rs".to_owned(), "mod.rs".to_owned()]),
        );
        let Meaning::Say { run: said_to, said } = meaning else {
            panic!("a drop on a session says something into it");
        };
        assert_eq!(said_to, run);
        assert_eq!(said, "2 files were dropped here: lex.rs, mod.rs");
    }

    /// One reading of what was dropped, four places it can go. A drop
    /// this build cannot read is refused wherever it lands, rather than
    /// each target deciding that for itself.
    #[test]
    fn an_unreadable_drop_is_refused_at_every_target() {
        let run = RunId::from_bytes([1u8; 16]);
        for target in [
            place("lab"),
            Target::Composer,
            Target::Run(run),
            Target::Nowhere,
        ] {
            assert!(
                matches!(read(&target, &Dropped::Unreadable), Meaning::Refused { .. }),
                "{target:?} read an unreadable drop as something"
            );
        }
    }

    #[test]
    fn a_drop_this_build_cannot_read_is_refused_rather_than_read_as_empty() {
        for dropped in [
            Dropped::Unreadable,
            Dropped::Files(Vec::new()),
            Dropped::Text("   ".to_owned()),
        ]
        .into_iter()
        {
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
