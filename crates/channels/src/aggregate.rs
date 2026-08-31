// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Watching several cities from one interface.
//!
//! One machine, one City, one Ledger. Two cities never
//! share a history, because a shared history would need distributed
//! ordering - and that trades a single writer's simple fact for a new class
//! of failure. What the interface may do is *look* at several at once.
//!
//! The hard constraint is one sentence, and it has a shape here rather
//! than a rule somewhere: **the aggregation layer forwards Queries and Events and never
//! forwards a Command.** Changing anything in another city means connecting
//! to that city directly, authenticating with its token, and writing to its
//! Ledger. An aggregator that could relay commands would be a cross-city
//! authority appearing on nobody's books - the target city could not tie
//! the command back to a person, and `PutSecret` refuses the trip anyway.
//!
//! That rule is not a check in this file. There is no method here that
//! accepts a Command, so relaying one has no spelling.

use std::collections::BTreeMap;

use kernel::{AxCode, AxError, B3Hash, EventRecord, Seq, TimeMs};

use crate::wire::Query;

/// Names one upstream city as the interface labels it. Local to this
/// viewer: the upstream does not know or care what we call it.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CityLabel(String);

impl CityLabel {
    /// Sole constructor.
    ///
    /// # Errors
    /// Refuses empty labels and control characters; a label is shown in the
    /// top bar and read by a person choosing which city to look at.
    pub fn parse(raw: &str) -> Result<Self, AxError> {
        if raw.is_empty() || raw.chars().any(char::is_control) {
            return Err(AxError::failure(
                AxCode::ConfigInvalid,
                "label an upstream city",
                "the label is empty or holds control characters",
            )
            .with_recovery("give the city a short single-line name"));
        }
        Ok(Self(raw.to_owned()))
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// One upstream connection's settings. No plaintext token: same discipline
/// as `ServeConfig`, for the same reason.
#[derive(Debug, Clone)]
pub struct Upstream {
    pub label: CityLabel,
    /// Where to reach it, as the operator typed it. Reachability is the
    /// transport's problem, not this module's.
    pub address: String,
    /// `None` when the upstream binds loopback and needs no pairing.
    pub token_digest: Option<B3Hash>,
}

/// One event as the merged view carries it: which city it came from, and
/// the event itself. The pair is what makes a merged stream readable - an
/// event with no city is a fact with no address.
#[derive(Debug, Clone, PartialEq)]
pub struct Sighting {
    pub city: CityLabel,
    pub event: EventRecord,
}

/// The read-only multi-city view.
///
/// Deliberately not generic and deliberately without a transport: the merge
/// order is the part that must be deterministic and testable, and it needs
/// no socket to be either.
#[derive(Debug, Default)]
pub struct Aggregate {
    upstreams: BTreeMap<CityLabel, Upstream>,
}

impl Aggregate {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers an upstream. Re-registering the same label replaces its
    /// settings rather than adding a second entry, so a corrected address
    /// does not silently leave the old one attached.
    pub fn attach(&mut self, upstream: Upstream) {
        self.upstreams.insert(upstream.label.clone(), upstream);
    }

    pub fn detach(&mut self, label: &CityLabel) -> bool {
        self.upstreams.remove(label).is_some()
    }

    /// Every attached city, in label order. `BTreeMap` rather than a hash
    /// map because iteration order reaching an interface must not depend on
    /// hashing.
    pub fn cities(&self) -> impl Iterator<Item = &Upstream> {
        self.upstreams.values()
    }

    /// Forwards a read to one upstream.
    ///
    /// This is the only method that sends anything, and it accepts a
    /// [`Query`]. There is no sibling that accepts a Command, which is how
    /// "never forwards a Command" is held.
    ///
    /// # Errors
    /// Refuses a label that was never attached, rather than guessing.
    pub fn ask(&self, label: &CityLabel, query: Query) -> Result<Forwarded, AxError> {
        let upstream = self.upstreams.get(label).ok_or_else(|| {
            AxError::failure(
                AxCode::ConfigInvalid,
                "forward a query to another city",
                format!("no city is attached under the label `{}`", label.as_str()),
            )
            .with_recovery("attach the city with its address and pairing code first")
        })?;
        Ok(Forwarded {
            address: upstream.address.clone(),
            query,
        })
    }

    /// Interleaves per-city event streams into one readable sequence.
    ///
    /// Ordering is `(t, city, seq)`. Sequence numbers are meaningless across
    /// cities - each Ledger numbers its own history - so time leads, and the
    /// label breaks ties. The label is in the key rather than a tiebreak of
    /// last resort because two cities can genuinely stamp the same
    /// millisecond, and the merged view must still be the same twice.
    #[must_use]
    pub fn merge(streams: Vec<(CityLabel, Vec<EventRecord>)>) -> Vec<Sighting> {
        let mut merged: Vec<Sighting> = streams
            .into_iter()
            .flat_map(|(city, events)| {
                events.into_iter().map(move |event| Sighting {
                    city: city.clone(),
                    event,
                })
            })
            .collect();
        merged.sort_by_key(sort_key);
        merged
    }
}

fn sort_key(sighting: &Sighting) -> (TimeMs, CityLabel, Seq) {
    (
        sighting.event.t(),
        sighting.city.clone(),
        sighting.event.seq(),
    )
}

/// What `ask` produced: an address and a read. It carries no Command field,
/// so nothing downstream can promote it into one.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Forwarded {
    pub address: String,
    pub query: Query,
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code"
)]
mod tests {
    use super::*;

    fn city(name: &str) -> CityLabel {
        CityLabel::parse(name).unwrap()
    }

    fn upstream(name: &str) -> Upstream {
        Upstream {
            label: city(name),
            address: format!("{name}.local:8787"),
            token_digest: None,
        }
    }

    #[test]
    fn attaching_the_same_label_twice_replaces_rather_than_duplicates() {
        let mut aggregate = Aggregate::new();
        aggregate.attach(upstream("studio"));
        let mut corrected = upstream("studio");
        corrected.address = "192.168.1.9:8787".to_owned();
        aggregate.attach(corrected);
        assert_eq!(aggregate.cities().count(), 1);
        let only = aggregate.cities().next().unwrap();
        assert_eq!(only.address, "192.168.1.9:8787");
    }

    #[test]
    fn asking_an_unattached_city_is_refused_not_guessed() {
        let aggregate = Aggregate::new();
        let err = aggregate
            .ask(&city("nowhere"), Query::CityView)
            .expect_err("an unknown label has no address to guess");
        assert_eq!(*err.code(), AxCode::ConfigInvalid);
        assert!(!err.recovery().is_empty());
    }

    fn event(at: u64, seq: u64) -> EventRecord {
        EventRecord::from_draft(
            kernel::EventDraft {
                run: kernel::RunId::from_bytes([4u8; 16]),
                t: TimeMs::new(at),
                who: "test".to_owned(),
                addr: None,
                kind: kernel::EventKind::RunStarted,
                data: kernel::Payload::empty(),
                ig: false,
            },
            Seq::new(seq),
            B3Hash::digest(b"prev"),
        )
    }

    #[test]
    fn merging_is_the_same_sequence_however_the_streams_arrive() {
        // Sequence numbers mean nothing across cities - each Ledger numbers
        // its own history - so time leads and the label breaks ties. Two
        // cities stamping the same millisecond is ordinary, and the merged
        // view must still be identical on a second pass.
        let left = (city("attic"), vec![event(10, 1), event(30, 2)]);
        let right = (city("studio"), vec![event(10, 7), event(20, 8)]);

        let one = Aggregate::merge(vec![left.clone(), right.clone()]);
        let other = Aggregate::merge(vec![right, left]);
        assert_eq!(one, other, "merge order cannot depend on arrival order");

        let trail: Vec<(&str, u64)> = one
            .iter()
            .map(|s| (s.city.as_str(), s.event.t().value()))
            .collect();
        assert_eq!(
            trail,
            [("attic", 10), ("studio", 10), ("studio", 20), ("attic", 30)]
        );
    }

    #[test]
    fn cities_iterate_in_label_order_regardless_of_attachment_order() {
        let mut aggregate = Aggregate::new();
        for name in ["studio", "attic", "shed"] {
            aggregate.attach(upstream(name));
        }
        let order: Vec<&str> = aggregate.cities().map(|u| u.label.as_str()).collect();
        assert_eq!(order, ["attic", "shed", "studio"]);
    }
}
