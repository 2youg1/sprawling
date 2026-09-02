// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The one translation between a [`View`] and the address bar
//! (web-SPEC.md sections 8-14 and 8-53 B1).
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
//!
//! **Six destinations write fragments; nine more still read.** The
//! v0.0.3 information architecture replaced eleven flat pages with six,
//! and every fragment the old set wrote still resolves — to the page
//! that inherited its question. A link a person kept is a promise this
//! build did not get to withdraw, so the old spellings are read forever
//! and written never.

use channels::{Address, RunId};

use crate::app::Snapshot;
use crate::lang::Msg;

/// Which page the content region is showing (web-SPEC.md section 8-53
/// B1). Three regions fill the window — top bar, left nav, content — and
/// only the content region routes.
///
/// **Six destinations, and two shapes that are reached from them.** The
/// previous set had eleven entries and no page for the one object this
/// product has: a session. Eight of those eleven were a list of
/// sessions, a history of sessions, or settings, and each had a nav
/// entry of its own — so the interface asked a person to choose between
/// eleven answers to a question they had not asked yet.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum View {
    /// The list, and the box that starts work. Default because the
    /// action a person arrives to take is on it.
    #[default]
    Sessions,
    /// One session: a work line in one room, named by the name a person
    /// gave it. The object page this interface never had.
    Session(Address),
    /// Everything that cannot move until a person answers.
    Waiting,
    /// One history, in three lenses. They were three pages with three
    /// nav entries, and no reader ever had to choose between them
    /// before knowing what they were looking for.
    Record(Lens),
    /// Spend, in five cuts.
    Cost,
    /// Where a provider is registered, and which language this reads in.
    /// A region rather than a modal: registering is work, and work that
    /// can be interrupted needs a place to return to.
    Setup,
    /// One building's own files and archive. Reached from a session and
    /// from the city drawing, not from the nav: it is where sessions
    /// live rather than a sixth thing to check.
    Building(Address),
    /// A run named by a link written before sessions had addresses.
    /// Held as its own view because the router is pure and the room a
    /// run is in is a fact only the snapshot has; the page resolves it
    /// and moves on, so this is a state the address bar passes through.
    Run(RunId),
}

/// Which lens the record is read through.
///
/// One page, three lenses, because they are three questions about one
/// history and a person picks the lens after deciding to look — not
/// before. `Bin` is spelled short in the fragment and long on screen:
/// the address bar is typed and the heading is read.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Lens {
    /// Every record, newest first.
    #[default]
    Ledger,
    /// What was filed on the shelves, and what went there lately.
    Archive,
    /// What was discarded, and the way each row comes back.
    Bin,
}

impl Lens {
    /// Every lens, in the order the page offers them: the whole history,
    /// then what was kept, then what was thrown away.
    pub const ALL: [Lens; 3] = [Lens::Ledger, Lens::Archive, Lens::Bin];

    /// What this lens is called on screen.
    #[must_use]
    pub fn word(self) -> Msg {
        match self {
            Self::Ledger => Msg::RecordLensLedger,
            Self::Archive => Msg::RecordLensArchive,
            Self::Bin => Msg::RecordLensBin,
        }
    }
}

/// The address-bar form of a view, fragment marker included.
///
/// Always begins `#/`, so a fragment written by hand and one written
/// here are the same string. Each view has exactly one spelling: the
/// older spellings resolve in [`from_fragment`] and are never produced,
/// which is what keeps the address bar from teaching two names for one
/// place.
#[must_use]
pub fn to_fragment(view: &View) -> String {
    match view {
        View::Sessions => "#/".to_owned(),
        View::Session(addr) => format!("#/s/{}", addr.as_str()),
        View::Waiting => "#/waiting".to_owned(),
        View::Record(Lens::Ledger) => "#/record".to_owned(),
        View::Record(Lens::Archive) => "#/record/archive".to_owned(),
        View::Record(Lens::Bin) => "#/record/bin".to_owned(),
        View::Cost => "#/cost".to_owned(),
        View::Setup => "#/setup".to_owned(),
        View::Building(addr) => format!("#/b/{}", addr.as_str()),
        View::Run(run) => format!("#/live/{run}"),
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
        ("", "") => Some(View::Sessions),
        ("s", addr) => Address::parse(addr).ok().map(View::Session),
        ("waiting", "") => Some(View::Waiting),
        ("record", "") => Some(View::Record(Lens::Ledger)),
        ("record", "archive") => Some(View::Record(Lens::Archive)),
        ("record", "bin") => Some(View::Record(Lens::Bin)),
        ("cost", "") => Some(View::Cost),
        ("setup", "") => Some(View::Setup),
        ("b", addr) => Address::parse(addr).ok().map(View::Building),
        ("live", run) if !run.is_empty() => RunId::parse(run).ok().map(View::Run),

        // Everything below this line is a spelling this build no longer
        // writes. Each one lands on the page that inherited its
        // question, and the three that had a page of their own became
        // one lens each of the record.
        ("overview", "") | ("city", "") | ("live", "") => Some(View::Sessions),
        ("approvals", "") => Some(View::Waiting),
        ("ledger", "") => Some(View::Record(Lens::Ledger)),
        ("archive", "") => Some(View::Record(Lens::Archive)),
        ("recycle-bin", "") => Some(View::Record(Lens::Bin)),
        ("dashboard", "") => Some(View::Cost),
        ("settings", "") => Some(View::Setup),
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
/// `hashchange` is what moves the signal, so a click, an `<a href>` and
/// the browser's own back button take the same path and cannot disagree.
#[cfg(target_arch = "wasm32")]
pub fn go(view: &View) {
    let Some(window) = web_sys::window() else {
        return;
    };
    // Assigning the hash is what pushes a history entry; `replace` would
    // make the back button skip the page a person just left.
    let _ = window.location().set_hash(&to_fragment(view));
}

/// One entry of the left nav.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Destination {
    pub view: View,
    /// What this destination is called, in whichever language the
    /// person reads. The word itself lives in `web::lang`, so the nav
    /// and the translation cannot disagree about which page is which.
    pub label: crate::lang::Msg,
    /// How many things are waiting behind this destination, when waiting
    /// is a thing that can happen there.
    pub waiting: Option<u32>,
}

/// Every destination the left nav offers, in reading order.
///
/// One producer for the list, its wording and its badge: a destination
/// added here appears in the nav, in the router and in the test that
/// walks them, and cannot appear in two of the three.
///
/// **Five, and flat.** The previous nav had nine entries under three
/// headings, and the headings existed because nine entries read as a
/// menu to be searched rather than a place to go. Five is inside the
/// span a person holds without searching, so the headings are not
/// replaced by better headings — they are not needed.
#[must_use]
pub fn destinations(snapshot: &Snapshot) -> Vec<Destination> {
    let waiting = snapshot.waiting_on_you();
    vec![
        Destination {
            view: View::Sessions,
            label: crate::lang::Msg::NavSessions,
            waiting: None,
        },
        // The one badge in this interface. It is here on every page
        // because an unfinished thing that is out of sight stops being
        // an unfinished thing and starts being a surprise.
        Destination {
            view: View::Waiting,
            label: crate::lang::Msg::NavWaiting,
            waiting: (waiting > 0).then_some(waiting),
        },
        Destination {
            view: View::Record(Lens::Ledger),
            label: crate::lang::Msg::NavTheRecord,
            waiting: None,
        },
        Destination {
            view: View::Cost,
            label: crate::lang::Msg::NavCost,
            waiting: None,
        },
        Destination {
            view: View::Setup,
            label: crate::lang::Msg::NavSettings,
            waiting: None,
        },
    ]
}

/// Whether this destination is the page being shown.
///
/// The record's three lenses are one destination, so the nav entry stays
/// marked while a person moves between them: an entry that unhighlights
/// when the reader is still inside it says they have left.
#[must_use]
pub fn showing(destination: &View, view: &View) -> bool {
    match (destination, view) {
        (View::Record(_), View::Record(_)) => true,
        // A session and a building are reached from the list, and the
        // list stays lit while a person is inside one: they went deeper
        // into what the first entry offers rather than somewhere else.
        (View::Sessions, View::Session(_) | View::Building(_) | View::Run(_)) => true,
        (left, right) => left == right,
    }
}

/// The building a person is looking at, if the city page has one
/// selected. The nav does not carry buildings - a city may have fifty -
/// so the way in is the city page, and this is what it hands over.
#[must_use]
pub fn opened_building(selected: Option<&str>) -> Option<Address> {
    selected.and_then(|name| Address::parse(name).ok())
}

/// Where the `g` sequence's second key goes.
///
/// Here rather than in `web::keys` because a `View` carries a run id and
/// an address, and a module that decides what a key means has no business
/// holding either.
#[cfg_attr(
    not(target_arch = "wasm32"),
    expect(
        dead_code,
        reason = "the only caller is the browser's keydown listener"
    )
)]
pub(crate) fn place_view(place: crate::keys::Place) -> View {
    match place {
        crate::keys::Place::Sessions => View::Sessions,
        crate::keys::Place::Waiting => View::Waiting,
        crate::keys::Place::Record => View::Record(Lens::Ledger),
        crate::keys::Place::Cost => View::Cost,
        crate::keys::Place::Setup => View::Setup,
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
    use super::{Lens, View, from_fragment, to_fragment};
    use channels::{Address, RunId};

    /// Every view this client has, so the round trip is exhaustive by
    /// construction: a variant added without a fragment fails to compile
    /// here rather than shipping as a page nobody can link to.
    fn every_view() -> Vec<View> {
        let all = [
            View::Sessions,
            View::Session(Address::parse("lab/parser").unwrap()),
            View::Waiting,
            View::Record(Lens::Ledger),
            View::Record(Lens::Archive),
            View::Record(Lens::Bin),
            View::Cost,
            View::Setup,
            View::Building(Address::parse("lab").unwrap()),
            View::Run(RunId::from_bytes([7u8; 16])),
        ];
        // The match is what makes the list exhaustive: adding a variant
        // stops this compiling until the list names it.
        for view in &all {
            match view {
                View::Sessions
                | View::Session(_)
                | View::Waiting
                | View::Record(_)
                | View::Cost
                | View::Setup
                | View::Building(_)
                | View::Run(_) => {}
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

    /// An empty address bar is the sessions list, which is what a person
    /// gets by opening the URL the terminal printed.
    #[test]
    fn nothing_in_the_address_bar_is_the_first_page() {
        for empty in ["", "#", "#/"] {
            assert_eq!(from_fragment(empty), Some(View::Sessions));
        }
    }

    /// A session is addressed by the name a person gave it, which is the
    /// whole reason this page exists: `#/live/<uuid>` named a session by
    /// a number nobody chose, so "open yesterday's session" had no
    /// answer even after the query behind it was built.
    #[test]
    fn a_session_is_named_by_its_room_and_not_by_a_number() {
        let addr = Address::parse("lab/parser").unwrap();
        assert_eq!(to_fragment(&View::Session(addr.clone())), "#/s/lab/parser");
        assert_eq!(from_fragment("#/s/lab/parser"), Some(View::Session(addr)));
    }

    /// Every fragment the previous information architecture wrote still
    /// lands, on the page that inherited its question. This is the whole
    /// promise; the table is the promise written down.
    #[test]
    fn every_fragment_the_old_pages_wrote_still_lands() {
        let kept = [
            ("#/overview", View::Sessions),
            ("#/city", View::Sessions),
            ("#/live", View::Sessions),
            ("#/approvals", View::Waiting),
            ("#/ledger", View::Record(Lens::Ledger)),
            ("#/archive", View::Record(Lens::Archive)),
            ("#/recycle-bin", View::Record(Lens::Bin)),
            ("#/dashboard", View::Cost),
            ("#/settings", View::Setup),
        ];
        for (fragment, landing) in kept {
            assert_eq!(
                from_fragment(fragment),
                Some(landing),
                "{fragment} was a link somebody kept"
            );
        }
        // Two that carry an argument, so they cannot go in the table.
        assert_eq!(
            from_fragment("#/building/lab"),
            Some(View::Building(Address::parse("lab").unwrap()))
        );
        assert_eq!(
            from_fragment("#/live/07070707-0707-0707-0707-070707070707"),
            Some(View::Run(RunId::from_bytes([7u8; 16]))),
            "a run named by an old link is resolved to its room by the page, not by the router"
        );
    }

    /// An old spelling resolves and is never written back, so a person
    /// following one lands on the page and then sees its real name.
    #[test]
    fn no_view_writes_a_fragment_this_build_no_longer_uses() {
        let retired = [
            "#/overview",
            "#/city",
            "#/approvals",
            "#/ledger",
            "#/archive",
            "#/recycle-bin",
            "#/dashboard",
            "#/settings",
            "#/building/lab",
        ];
        for view in every_view() {
            let written = to_fragment(&view);
            assert!(
                !retired.contains(&written.as_str()),
                "{written} is a retired spelling and something still writes it"
            );
        }
    }

    /// A link that does not resolve says so rather than landing
    /// somewhere else quietly.
    #[test]
    fn a_fragment_that_names_nothing_answers_nothing() {
        for wrong in [
            "#/nowhere",
            "#/s/",
            "#/b/",
            "#/building/",
            "#/live/not-a-run",
            "#/city/extra",
            "#/record/nowhere",
            "#/waiting/extra",
        ] {
            assert_eq!(from_fragment(wrong), None, "{wrong} resolved to something");
        }
    }

    #[test]
    fn the_default_view_answers_the_question_somebody_arrives_with() {
        // A person opening this product is asking "is anything
        // happening, does any of it need me, and how do I start
        // something". One page answers all three, and it is the page the
        // bare fragment lands on.
        assert_eq!(View::default(), View::Sessions);
        assert_eq!(
            crate::route::from_fragment("#/"),
            Some(View::Sessions),
            "the bare fragment and the default view are the same page"
        );
    }
}
