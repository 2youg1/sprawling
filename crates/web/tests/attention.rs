// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The attention-cost acceptance, as assertions rather than as a review.
//!
//! Four promises are made about this interface: it does not keep a count
//! of things you have not looked at, it does not scroll forever, its
//! progress bar does not move on its own, and a tab in the background
//! stops. Each of them is checkable, and a promise that is only checked
//! by somebody remembering to look is a promise that lasts until the
//! first busy week.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

use std::collections::BTreeSet;

/// Files that render something a person looks at. The rule is about what
/// reaches a screen, so the modules that only decide are out of scope.
const RENDERING: [&str; 8] = [
    "src/alert.rs",
    "src/approval.rs",
    "src/city_view.rs",
    "src/dashboard.rs",
    "src/ledger_view.rs",
    "src/live.rs",
    "src/progress.rs",
    "src/settings.rs",
];

fn source(name: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|err| panic!("{}: {err}", path.display()))
}

/// Lines that are code rather than prose. The modules explain their own
/// refusals in comments, so a scan that counted those would find the
/// words it is looking for in exactly the places that promise not to do
/// the thing.
fn code_lines(text: &str) -> Vec<&str> {
    text.lines()
        .map(str::trim)
        .filter(|line| {
            !line.starts_with("//") && !line.starts_with("///") && !line.starts_with("//!")
        })
        .collect()
}

#[test]
fn nothing_on_screen_counts_what_you_have_not_looked_at() {
    // An unread counter turns an interface into something that owes you
    // a number, and the number only ever goes up while you are away.
    // Matched as a word rather than a substring: `unreadable_rows` is a
    // diagnostic about a plan nobody can parse, and a scan that could
    // not tell the two apart would be a gate people learn to route
    // around.
    let banned = ["unread", "badge", "unseen"];
    let mut found = Vec::new();
    for file in RENDERING {
        let text = source(file);
        for (n, line) in code_lines(&text).iter().enumerate() {
            let lowered = line.to_lowercase();
            for needle in banned {
                let mut from = 0usize;
                while let Some(hit) = lowered.get(from..).and_then(|rest| rest.find(needle)) {
                    let start = from.saturating_add(hit);
                    let end = start.saturating_add(needle.len());
                    let next = lowered.as_bytes().get(end).copied();
                    if !next.is_some_and(|byte| byte.is_ascii_alphabetic()) {
                        found.push(format!("{file}:{n}: {line}"));
                    }
                    from = end;
                }
            }
        }
    }
    assert!(
        found.is_empty(),
        "the interface refuses unread counts:\n{}",
        found.join("\n")
    );
}

#[test]
fn the_live_view_has_an_end_and_says_what_it_dropped() {
    // A feed with no bottom is a feed you cannot finish reading. This
    // one has a window, and what fell out of it is reported rather than
    // quietly discarded.
    let text = source("src/live.rs");
    assert!(
        text.contains("pub const WINDOW"),
        "the window is a named constant, not a number somewhere in a loop"
    );
    assert!(
        text.contains("pub fn dropped"),
        "what left the window is readable; a feed that forgets silently is a feed that lies"
    );
}

#[test]
fn the_progress_bar_does_not_move_on_its_own() {
    // A stripe that flows suggests work is happening. When the work has
    // stopped, that suggestion is false, and it is exactly the moment
    // somebody needs the truth.
    let text = source("src/progress.rs").to_lowercase();
    for banned in ["animation", "keyframes", "@-webkit-keyframes", "spinner"] {
        for line in code_lines(&text) {
            assert!(
                !line.contains(banned),
                "the progress bar carries no {banned}: {line}"
            );
        }
    }
}

#[test]
fn a_backgrounded_tab_stops_rather_than_slowing_down() {
    // Stopping and slowing look the same for a minute and then diverge:
    // a slowed tab still holds a socket, still wakes a laptop, and still
    // costs the person something they did not ask to spend.
    let text = source("src/socket.rs");
    assert!(
        text.contains("Backgrounded") && text.contains("CloseSocket"),
        "going out of view closes the link rather than slowing it"
    );
    assert!(
        text.contains("Suspended"),
        "and a suspended link is its own state, so nothing reaches it by accident"
    );
}

#[test]
fn colour_is_never_the_only_thing_that_carries_a_meaning() {
    // The single-hue language exists so that a city read without colour
    // is the same city. Every rendering module therefore states its
    // states in words somewhere, and the colour gate proves the other
    // half - that no module outside `theme` writes a colour at all.
    let mut silent = BTreeSet::new();
    for file in RENDERING {
        let text = source(file);
        let says_something = code_lines(&text)
            .iter()
            .any(|line| line.contains('"') && line.contains('{'));
        if !says_something {
            silent.insert(file);
        }
    }
    assert!(
        silent.is_empty(),
        "a module that renders has to render words too: {silent:?}"
    );
}
