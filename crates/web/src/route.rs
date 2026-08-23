// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The one translation between a [`View`] and the address bar
//! (web-SPEC.md section 8-14).
//!
//! Without it there is no deep link, no browser back, no bookmark, and
//! no way to photograph any page but the first — which is also why the
//! front end could not be regression-tested.
//!
//! **A fragment, not a path.** A path-based route would need the asset
//! route to answer `index.html` for anything it does not recognise, and
//! that route's refusal of an unknown path is a security judgement
//! (`ClientAssets::lookup` closes over a fixed table and rejects
//! traversal). A fragment is never sent to the server at all, so
//! bookmarks, history and deep links all work without weakening the one
//! judgement standing between a URL and this machine's disk.

use channels::{Address, RunId};

use crate::app::View;

/// The address-bar form of a view, fragment marker included.
///
/// Always begins `#/`, so a fragment written by hand and one written
/// here are the same string.
#[must_use]
pub fn to_fragment(view: &View) -> String {
    match view {
        View::Overview => "#/".to_owned(),
        View::City => "#/city".to_owned(),
        View::Live(None) => "#/live".to_owned(),
        View::Live(Some(run)) => format!("#/live/{run}"),
        View::Approvals => "#/approvals".to_owned(),
        View::RecycleBin => "#/recycle-bin".to_owned(),
        View::Archive => "#/archive".to_owned(),
        View::Dashboard => "#/cost".to_owned(),
        View::Ledger => "#/ledger".to_owned(),
        View::Building(addr) => format!("#/building/{}", addr.as_str()),
        View::Settings => "#/settings".to_owned(),
    }
}

/// The view a fragment names, or `None` when it names nothing.
///
/// `None` rather than a silent fall back to the first page: a link that
/// does not resolve is a fact the caller may want to say something
/// about, and a router that quietly lands somewhere else teaches people
/// their bookmarks are unreliable without ever admitting it.
#[must_use]
pub fn from_fragment(raw: &str) -> Option<View> {
    let path = raw.trim_start_matches('#').trim_start_matches('/');
    let (head, tail) = path.split_once('/').unwrap_or((path, ""));
    match (head, tail) {
        ("", "") => Some(View::Overview),
        ("overview", "") => Some(View::Overview),
        ("city", "") => Some(View::City),
        ("live", "") => Some(View::Live(None)),
        ("live", run) => RunId::parse(run).ok().map(Some).map(View::Live),
        ("approvals", "") => Some(View::Approvals),
        ("recycle-bin", "") => Some(View::RecycleBin),
        ("archive", "") => Some(View::Archive),
        // `cost` is what the nav calls it and what this page writes; the
        // older spelling still resolves, because a link somebody kept is
        // a promise this build did not get to withdraw.
        ("cost", "") | ("dashboard", "") => Some(View::Dashboard),
        ("ledger", "") => Some(View::Ledger),
        ("settings", "") => Some(View::Settings),
        ("building", addr) => Address::parse(addr).ok().map(View::Building),
        _ => None,
    }
}

/// What the address bar says, when this build cannot resolve it.
///
/// `None` for an empty fragment, which is the first page rather than a
/// broken link. Separate from [`current`] because the two answers are
/// different questions: one asks where to go, the other asks what to say
/// about not going anywhere.
#[cfg(target_arch = "wasm32")]
#[must_use]
pub fn unresolved() -> Option<String> {
    let hash = web_sys::window()?.location().hash().ok()?;
    let named = hash.trim_start_matches('#').trim_start_matches('/');
    if named.is_empty() || from_fragment(&hash).is_some() {
        return None;
    }
    Some(format!("#/{named}"))
}

/// Reads the address bar. `None` when there is no browser to read.
#[cfg(target_arch = "wasm32")]
#[must_use]
pub fn current() -> Option<View> {
    let hash = web_sys::window()?.location().hash().ok()?;
    from_fragment(&hash)
}

/// Puts a view in the address bar without reloading the page.
///
/// Writing the fragment is the only way a view changes: the listener on
/// `hashchange` is what moves the signal, so a click and the browser's
/// own back button take the same path and cannot disagree.
#[cfg(target_arch = "wasm32")]
pub fn go(view: &View) {
    let Some(window) = web_sys::window() else {
        return;
    };
    // Assigning the hash is what pushes a history entry; `replace` would
    // make the back button skip the page a person just left.
    let _ = window.location().set_hash(&to_fragment(view));
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
    use super::{View, from_fragment, to_fragment};
    use channels::{Address, RunId};

    /// Every view this client has, so the round trip is exhaustive by
    /// construction: a variant added without a fragment fails to compile
    /// here rather than shipping as a page nobody can link to.
    fn every_view() -> Vec<View> {
        let all = [
            View::Overview,
            View::City,
            View::Live(None),
            View::Live(Some(RunId::from_bytes([7u8; 16]))),
            View::Approvals,
            View::RecycleBin,
            View::Archive,
            View::Dashboard,
            View::Ledger,
            View::Building(Address::parse("lab/room1").unwrap()),
            View::Settings,
        ];
        // The match is what makes the list exhaustive: adding a variant
        // stops this compiling until the list names it.
        for view in &all {
            match view {
                View::Overview
                | View::City
                | View::Live(_)
                | View::Approvals
                | View::RecycleBin
                | View::Archive
                | View::Dashboard
                | View::Ledger
                | View::Building(_)
                | View::Settings => {}
            }
        }
        all.to_vec()
    }

    #[test]
    fn every_view_survives_the_address_bar_unchanged() {
        for view in every_view() {
            let written = to_fragment(&view);
            assert_eq!(
                from_fragment(&written).as_ref(),
                Some(&view),
                "{written} did not come back as the view that wrote it"
            );
        }
    }

    #[test]
    fn every_fragment_is_absolute_so_a_hand_written_one_matches() {
        for view in every_view() {
            assert!(to_fragment(&view).starts_with("#/"));
        }
    }

    /// An empty address bar is the city, which is what a person gets by
    /// opening the URL the terminal printed.
    #[test]
    fn nothing_in_the_address_bar_is_the_first_page() {
        for empty in ["", "#", "#/"] {
            assert_eq!(from_fragment(empty), Some(View::Overview));
        }
        // The city keeps a fragment of its own, so a link to it made
        // before the overview existed still lands on the city.
        assert_eq!(from_fragment("#/city"), Some(View::City));
    }

    /// A link that does not resolve says so rather than landing
    /// somewhere else quietly.
    #[test]
    fn the_nav_label_and_the_address_agree() {
        // They did not: the nav said "cost" and the address bar said
        // "#/dashboard", so a person who typed what they were shown landed
        // on a fragment this build could not resolve.
        assert_eq!(to_fragment(&View::Dashboard), "#/cost");
        assert_eq!(from_fragment("#/cost"), Some(View::Dashboard));
        assert_eq!(
            from_fragment("#/dashboard"),
            Some(View::Dashboard),
            "and a link somebody already kept still lands"
        );
    }

    #[test]
    fn a_fragment_that_names_nothing_answers_nothing() {
        for wrong in [
            "#/nowhere",
            "#/building/",
            "#/live/not-a-run",
            "#/city/extra",
        ] {
            assert_eq!(from_fragment(wrong), None, "{wrong} resolved to something");
        }
    }
}
