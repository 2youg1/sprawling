// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! What a keystroke means, decided away from the browser that delivers it.
//!
//! The readers of this client already work a keyboard all day: it is the
//! interface to Claude Code and to Codex, and both are driven without a
//! pointer. This client had no shortcut at all, so the only way to reach a
//! page was to aim at the nav, and the only way to reach a room was to
//! type its address.
//!
//! Everything here is a pure function of the keystroke and the sequence so
//! far, which is what lets the whole key map be tested on the host. The
//! browser contributes three facts and no judgement: which key, whether
//! the command modifier was down, and whether the reader was inside a
//! text field at the time.
//!
//! **A sequence cannot get stuck.** `g` waits for one more key and any key
//! that is not a destination returns to the resting state, so there is no
//! mode a reader can be in without knowing it - and no timer, which this
//! shape is not allowed to hold anyway.

/// Where a two-key sequence has got to.
///
/// Two states rather than a `bool` because the set is closed by the key
/// map: `g` is the only sequence leader this client has, and a third
/// leader would be a third variant rather than a second flag.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Chord {
    /// No sequence is open; the next key is read on its own.
    #[default]
    Idle,
    /// `g` was pressed; the next key names a destination or cancels.
    Leading,
}

/// One keystroke, as the three facts a judgement needs.
///
/// `command` is the platform's own accelerator - `Ctrl` where this client
/// mostly runs, `Meta` on a Mac - resolved by the caller, because which
/// physical key that is belongs to the browser and not to this decision.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Stroke<'a> {
    /// `KeyboardEvent.key`, verbatim.
    pub key: &'a str,
    /// Whether the accelerator was held.
    pub command: bool,
    /// Whether focus was inside something the reader types into.
    ///
    /// Load-bearing: without it, writing the word "goal" into the task box
    /// would navigate away on its `g`.
    pub in_text: bool,
}

/// A destination the `g` sequence can reach.
///
/// Five, and they are the five a person returns to while work is running.
/// The remaining pages are a nav click away and are not places anybody
/// goes to repeatedly, so binding them would spend letters to no end.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Place {
    Overview,
    City,
    Sessions,
    Approvals,
    Ledger,
}

/// What the client should do about a keystroke.
///
/// Exhaustive, and `Ignore` is a real answer rather than an absence: the
/// caller decides whether to let the browser have the key, and it can only
/// decide that if every stroke is answered.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Act {
    /// Nothing here claims this key; the browser keeps it.
    Ignore,
    /// Open the command palette.
    OpenPalette,
    /// Close whatever is open, in one step.
    Dismiss,
    /// Put the cursor where work is started, or send what is already
    /// written there.
    Compose,
    /// Show the key map itself.
    ShowKeys,
    /// Go somewhere.
    Go(Place),
}

/// Reads one keystroke against the sequence so far.
///
/// Returns the sequence state to hold next, so the caller stores one value
/// and never reasons about modes.
#[must_use]
pub fn press(chord: Chord, stroke: &Stroke) -> (Chord, Act) {
    // Two keys outrank both the sequence and the text field. A palette
    // that a reader has to leave the box to open is a palette nobody
    // opens, and Escape must always mean "let me out" or it means nothing.
    if stroke.command && key_is(stroke.key, "k") {
        return (Chord::Idle, Act::OpenPalette);
    }
    if stroke.command && stroke.key == "Enter" {
        return (Chord::Idle, Act::Compose);
    }
    if stroke.key == "Escape" {
        return (Chord::Idle, Act::Dismiss);
    }
    // Past this point a bare letter is a letter somebody is writing.
    if stroke.in_text {
        return (Chord::Idle, Act::Ignore);
    }
    match chord {
        Chord::Leading => (Chord::Idle, destination(stroke.key)),
        Chord::Idle => match stroke.key {
            "?" => (Chord::Idle, Act::ShowKeys),
            key if key_is(key, "g") => (Chord::Leading, Act::Ignore),
            _ => (Chord::Idle, Act::Ignore),
        },
    }
}

/// The second key of the `g` sequence.
fn destination(key: &str) -> Act {
    if key_is(key, "o") {
        return Act::Go(Place::Overview);
    }
    if key_is(key, "c") {
        return Act::Go(Place::City);
    }
    if key_is(key, "s") {
        return Act::Go(Place::Sessions);
    }
    if key_is(key, "a") {
        return Act::Go(Place::Approvals);
    }
    if key_is(key, "l") {
        return Act::Go(Place::Ledger);
    }
    Act::Ignore
}

/// Whether a reported key is the given letter, in either case.
///
/// A reader with caps lock on is still asking for the same thing, and the
/// browser reports `"K"` for it.
fn key_is(key: &str, letter: &str) -> bool {
    key.eq_ignore_ascii_case(letter)
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
    use super::{Act, Chord, Place, Stroke, press};

    fn bare(key: &str) -> Stroke<'_> {
        Stroke {
            key,
            command: false,
            in_text: false,
        }
    }

    fn typing(key: &str) -> Stroke<'_> {
        Stroke {
            key,
            command: false,
            in_text: true,
        }
    }

    fn held(key: &str) -> Stroke<'_> {
        Stroke {
            key,
            command: true,
            in_text: false,
        }
    }

    #[test]
    fn the_palette_opens_from_inside_a_text_box() {
        // The case that matters: somebody halfway through a task line
        // wants to jump somewhere. A palette they must first click out of
        // is one they will not use.
        let inside = Stroke {
            key: "k",
            command: true,
            in_text: true,
        };
        assert_eq!(press(Chord::Idle, &inside), (Chord::Idle, Act::OpenPalette));
    }

    #[test]
    fn escape_always_means_let_me_out() {
        for stroke in [bare("Escape"), typing("Escape"), held("Escape")] {
            assert_eq!(press(Chord::Idle, &stroke), (Chord::Idle, Act::Dismiss));
        }
        // Including halfway through a sequence, which is the state a
        // reader is most likely to want out of.
        assert_eq!(
            press(Chord::Leading, &bare("Escape")),
            (Chord::Idle, Act::Dismiss)
        );
    }

    #[test]
    fn a_letter_being_typed_is_never_a_command() {
        // Writing "goal" into the task box must not navigate on its `g`.
        for key in ["g", "o", "c", "a", "l", "s", "?"] {
            assert_eq!(press(Chord::Idle, &typing(key)), (Chord::Idle, Act::Ignore));
        }
    }

    #[test]
    fn the_sequence_reaches_all_five_and_leaves_no_mode_behind() {
        let wanted = [
            ("o", Place::Overview),
            ("c", Place::City),
            ("s", Place::Sessions),
            ("a", Place::Approvals),
            ("l", Place::Ledger),
        ];
        for (key, place) in wanted {
            let (chord, act) = press(Chord::Idle, &bare("g"));
            assert_eq!((chord, act), (Chord::Leading, Act::Ignore));
            assert_eq!(press(chord, &bare(key)), (Chord::Idle, Act::Go(place)));
        }
    }

    #[test]
    fn a_sequence_nobody_finished_cannot_strand_the_reader() {
        // Any key that is not a destination ends the sequence rather than
        // waiting, so there is no invisible mode and no timer - which this
        // shape may not hold anyway.
        let (chord, act) = press(Chord::Leading, &bare("z"));
        assert_eq!((chord, act), (Chord::Idle, Act::Ignore));
    }

    #[test]
    fn caps_lock_asks_for_the_same_thing() {
        assert_eq!(
            press(Chord::Idle, &held("K")),
            (Chord::Idle, Act::OpenPalette)
        );
        let (chord, _) = press(Chord::Idle, &bare("G"));
        assert_eq!(
            press(chord, &bare("O")),
            (Chord::Idle, Act::Go(Place::Overview))
        );
    }

    #[test]
    fn the_two_keys_that_start_work_are_told_apart() {
        assert_eq!(
            press(Chord::Idle, &held("Enter")),
            (Chord::Idle, Act::Compose)
        );
        // A bare Enter belongs to whatever is focused - a form, a button,
        // a link - and this module does not take it.
        assert_eq!(
            press(Chord::Idle, &bare("Enter")),
            (Chord::Idle, Act::Ignore)
        );
    }

    #[test]
    fn the_key_map_is_reachable_without_knowing_the_key_map() {
        assert_eq!(press(Chord::Idle, &bare("?")), (Chord::Idle, Act::ShowKeys));
    }
}
