// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The process boundary's vocabulary: the Commands, the Queries, and
//! the Event push. Encoding is JSON because the receiving
//! end is a browser and a human reading a network panel is a design goal.
//!
//! Two invariants live in the type system rather than in a check:
//!
//! - `Command` is generic over the carrier of a secret. `WireCommand` fixes
//!   that carrier to an uninhabited type, so a frame arriving from a socket
//!   cannot be a `PutSecret` - not "is rejected", but has no representation.
//!   Credentials are enrolled on the host machine, and that constraint is
//!   held by construction rather than by a check.
//! - Every state-changing Command owns an `IdemKey` field. There is no
//!   constructor that omits it, so "double-clicking twice opens two Runs" is
//!   not reachable from this type.
//!
//! Names of things this crate does not own - modes, providers, templates -
//! travel as validated newtypes with no closed value list. The authority for
//! which values are legal stays upstream (`runtime::Mode`, gateway, city);
//! the mapping point is the assembly layer, and an unknown value is an error
//! there, never a guess.

use kernel::{Address, AxError, B3Hash, EventRecord, GitOid, RunId, Seq};
use serde::{Deserialize, Serialize};

/// Wire format version. Bumped whenever the frame grammar changes shape in a
/// way the schema hash alone would not explain to a human reading a log.
///
/// 5: `Dispatch` carries the name of the session it starts (F2.11).
/// 6: and how hard that session thinks (F2.16).
/// 7: a provider can be asked what it serves before it is attached, and
///    an attachment names which of those models it admits (P3.01).
/// 8: a building's sandbox limits and external servers have a surface
///    (P3.02).
/// 9: a page can ask for the history that happened before it opened
///    (P3.04).
/// 12: a third class of frame carries what a model is saying while it is
///    still saying it (V3.13). It is not an event: it has no sequence
///    number, it is never written down, and a client that missed one has
///    lost nothing.
/// 13: the plan is a tree, so a building's answer carries its nodes,
///    what each is worth and what is ready; a branch that is stuck says
///    so once, at the node it is stuck at; and a city can be given a
///    goal it works towards until the work runs out (V3.17-V3.23).
pub const WIRE_V: u32 = 13;
use crate::answer::Answer;
use crate::command::{COMMAND_NAMES, WireCommand};

/// The Query surface, in declaration order.
pub const QUERY_NAMES: [&str; 14] = [
    "History",
    "RunHistory",
    "Changes",
    "RunView",
    "CityView",
    "ApprovalQueue",
    "InboxView",
    "Metrics",
    "CostView",
    "ArchiveSearch",
    "RegistryView",
    "DiscardView",
    "EndpointView",
    "BuildingView",
];

/// Queries read state. They are cacheable and free of side effects, so none
/// carries an `IdemKey` - a Query that needed one would have stopped being a
/// Query.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Query {
    /// A bounded slice of the one history, ending just before `before`
    /// or at the tail when that is absent.
    ///
    /// The server broadcasts what happens next and never what happened,
    /// so a page opened today saw a city that had been running for a
    /// month as an empty one. Bounded because the whole ledger is not a
    /// thing to put on a socket, and paged backwards because what a
    /// reader wants first is the end.
    History {
        before: Option<Seq>,
        limit: u32,
    },
    /// The same slice, narrowed to one session.
    ///
    /// [`Query::History`] carries no run, so a client watching four
    /// sessions divides one bounded slice between them and a session
    /// that started before the tab did is not in it at all - which is
    /// the whole of why opening yesterday's session showed a blank
    /// page. [`Query::RunView`] does not close the gap: five fields say
    /// whether a run exists and where it got to, not what happened in
    /// it.
    ///
    /// Answered with [`HistoryAnswer`], because "a page of history"
    /// already has a shape and a second one would be a second answer to
    /// the same question.
    RunHistory {
        run: RunId,
        before: Option<Seq>,
        limit: u32,
    },
    /// What moved between two checkpoints: paths and counts, never patch
    /// text.
    ///
    /// The caller names both ends because it already knows them - a
    /// checkpoint's oid is in the `checkpoint_committed` payload the
    /// client folded - and computing the pair a second time on the
    /// server would be a second answer to "which fences belong to this
    /// session". Both oids are immutable, so the answer is cacheable
    /// forever by anybody who wants to.
    ///
    /// `head` absent means the working tree: a wave still running has
    /// written files no checkpoint holds yet, and a list that ignored
    /// them would describe the session as it was one fence ago.
    Changes {
        base: GitOid,
        head: Option<GitOid>,
    },
    RunView {
        run: RunId,
    },
    CityView,
    ApprovalQueue,
    InboxView {
        addr: Address,
    },
    Metrics,
    CostView,
    ArchiveSearch {
        needle: String,
    },
    RegistryView,
    DiscardView,
    /// What is attached and what is chosen: the settings page's read.
    EndpointView,
    /// One building's own files and its archive - the pages an agent
    /// writes for the next agent, which are also the pages a person
    /// reads to know what happened in there.
    BuildingView {
        addr: Address,
    },
}

impl Query {
    /// Exhaustive, for the same reason as [`Command::name`].
    #[must_use]
    pub fn name(&self) -> &'static str {
        match *self {
            Self::History { .. } => "History",
            Self::RunHistory { .. } => "RunHistory",
            Self::Changes { .. } => "Changes",
            Self::RunView { .. } => "RunView",
            Self::CityView => "CityView",
            Self::ApprovalQueue => "ApprovalQueue",
            Self::InboxView { .. } => "InboxView",
            Self::Metrics => "Metrics",
            Self::CostView => "CostView",
            Self::ArchiveSearch { .. } => "ArchiveSearch",
            Self::RegistryView => "RegistryView",
            Self::DiscardView => "DiscardView",
            Self::EndpointView => "EndpointView",
            Self::BuildingView { .. } => "BuildingView",
        }
    }
}

/// A digest of the protocol surface, exchanged at connect time.
///
/// A browser can hold a cached older client while the server has moved on;
/// that mismatch is the one error WebUI has that a native window does not,
/// and this hash is its single answer. Pure: same inputs, same bytes, always.
#[must_use]
pub fn schema_hash() -> B3Hash {
    let mut material = Vec::new();
    material.extend_from_slice(b"sprawling/wire/");
    material.extend_from_slice(&WIRE_V.to_le_bytes());
    for name in COMMAND_NAMES {
        material.push(b'C');
        material.extend_from_slice(name.as_bytes());
    }
    for name in QUERY_NAMES {
        material.push(b'Q');
        material.extend_from_slice(name.as_bytes());
    }
    B3Hash::digest(&material)
}

/// The client's opening frame.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Hello {
    pub wire_v: u32,
    pub schema: B3Hash,
    /// Present only when the server binds a non-loopback address.
    pub token: Option<String>,
}

/// The server's answer to a `Hello` it accepted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Welcome {
    pub wire_v: u32,
    pub schema: B3Hash,
    /// Where the Event stream resumes, so a reconnect leaves no gap.
    pub resume_from: Option<Seq>,
    /// Which city answered. The handshake is where a connection learns
    /// whose city it is: the name is in the Ledger's first record, and a
    /// client that only ever hears what happens *next* would otherwise
    /// have to display "no city" over a city that has been running for a
    /// month.
    pub city: Option<Address>,
}

/// Everything a client may send.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ClientFrame {
    Hello(Hello),
    Command(Box<WireCommand>),
    Query(Query),
}

/// Everything a server may send. Events are the push half; a `Refusal`
/// carries the three-part refusal the interface renders verbatim.
///
/// **`Delta` is deliberately not an event.** An event is a thing that
/// happened, and a token increment is not: it has no sequence number, it
/// is never written to the Ledger, it cannot be replayed, and a client
/// that missed one has lost nothing. Giving it a frame class of its own
/// is what keeps that true — folded into the event stream it would
/// become a second, unverifiable history of what the model said.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ServerFrame {
    Welcome(Welcome),
    Event(Box<EventRecord>),
    Answer(Box<Answer>),
    Refusal(Box<AxError>),
    /// Text a model is saying, before the call it belongs to has
    /// settled. Discardable by construction: the run it belongs to is
    /// named so a client can throw the buffer away when `model_returned`
    /// arrives, and the settled text of that record is what a page
    /// draws. Where the two disagree, the record wins.
    Delta(Delta),
}

/// One piece of what a model is saying, on its way to a page.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Delta {
    pub run: RunId,
    pub text: String,
}
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, reason = "test code")]
mod tests {
    use super::*;

    #[test]
    fn the_query_names_match_the_variants() {
        let queries = [
            Query::History {
                before: None,
                limit: 20,
            },
            Query::RunHistory {
                run: RunId::from_bytes([1u8; 16]),
                before: None,
                limit: 20,
            },
            Query::Changes {
                base: kernel::GitOid::from_bytes([2u8; 20]),
                head: None,
            },
            Query::RunView {
                run: RunId::from_bytes([1u8; 16]),
            },
            Query::CityView,
            Query::ApprovalQueue,
            Query::InboxView {
                addr: Address::parse("acme").unwrap(),
            },
            Query::Metrics,
            Query::CostView,
            Query::ArchiveSearch {
                needle: "x".to_owned(),
            },
            Query::RegistryView,
            Query::DiscardView,
            Query::EndpointView,
            Query::BuildingView {
                addr: Address::parse("acme").unwrap(),
            },
        ];
        assert_eq!(queries.len(), QUERY_NAMES.len());
        for (query, expected) in queries.iter().zip(QUERY_NAMES) {
            assert_eq!(query.name(), expected, "declaration order must match");
        }
    }

    #[test]
    fn a_client_frame_round_trips_through_json() {
        let frame = ClientFrame::Query(Query::CityView);
        let text = serde_json::to_string(&frame).unwrap();
        let back: ClientFrame = serde_json::from_str(&text).unwrap();
        assert_eq!(frame, back);
    }
}
