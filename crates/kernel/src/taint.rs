// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The taint ring. One axiom: external content is
//! always data. `Tainted::new` is the city-wide sole entrance for foreign
//! bytes; reads hand out borrows, derivation unions provenance, and no
//! method ever returns the bare value — "washing it clean" is a compile
//! error, not a review finding.
//!
//! Deliberately absent: serde on `Tainted` (deserialization would be a
//! second entrance and a forged-empty-taint hole) and any `into_inner`.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// Non-empty provenance label, e.g. `web:example.com`, `file:upload`.
/// Grammar tightens with Endpoint identity in P1.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct TaintSource(String);

impl TaintSource {
    pub fn new(label: impl Into<String>) -> Option<TaintSource> {
        let label = label.into();
        if label.is_empty() {
            None
        } else {
            Some(TaintSource(label))
        }
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// Union semilattice of provenance. Empty means internally produced.
/// Serializable (event payloads and approval items carry source lists);
/// order is BTreeSet's, hence deterministic.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaintSet(BTreeSet<TaintSource>);

impl TaintSet {
    pub fn empty() -> TaintSet {
        TaintSet(BTreeSet::new())
    }

    pub fn of(source: TaintSource) -> TaintSet {
        let mut set = BTreeSet::new();
        set.insert(source);
        TaintSet(set)
    }

    pub fn union(&self, other: &TaintSet) -> TaintSet {
        TaintSet(self.0.union(&other.0).cloned().collect())
    }

    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn contains(&self, source: &TaintSource) -> bool {
        self.0.contains(source)
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn sources(&self) -> impl Iterator<Item = &TaintSource> {
        self.0.iter()
    }
}

/// External content with its provenance welded on. The value is private;
/// `peek` lends, `map`/`join` derive with automatic union, and nothing
/// returns `T` by value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Tainted<T> {
    value: T,
    taint: TaintSet,
}

impl<T> Tainted<T> {
    /// City-wide sole entrance for external content. Custody (secret scan
    /// before CAS) composes at this call site in the effect layer (S3);
    /// the type itself stays pure.
    pub fn new(value: T, source: TaintSource) -> Tainted<T> {
        Tainted {
            value,
            taint: TaintSet::of(source),
        }
    }

    /// Borrow the content: you can read it, you cannot walk away with it.
    pub fn peek(&self) -> &T {
        &self.value
    }

    pub fn taint(&self) -> &TaintSet {
        &self.taint
    }

    /// Derivation keeps the full provenance. The closure borrows — moving
    /// the value out through it cannot be spelled.
    pub fn map<U>(self, f: impl FnOnce(&T) -> U) -> Tainted<U> {
        Tainted {
            value: f(&self.value),
            taint: self.taint,
        }
    }

    /// Two-source derivation: the result carries the union.
    pub fn join<U, V>(self, other: Tainted<U>, f: impl FnOnce(&T, &U) -> V) -> Tainted<V> {
        Tainted {
            value: f(&self.value, &other.value),
            taint: self.taint.union(&other.taint),
        }
    }
}

#[cfg(kani)]
mod verification {
    //! V5: union monotonicity — joining never drops a source. Runs on the
    //! CI Linux job (kani has no Windows host); the proptest mirror below
    //! keeps the property exercised locally.

    use super::*;

    #[kani::proof]
    fn join_never_drops_a_source() {
        let pick_a: bool = kani::any();
        let pick_b: bool = kani::any();
        let sa = TaintSource::new(if pick_a { "a1" } else { "a2" });
        let sb = TaintSource::new(if pick_b { "b1" } else { "b2" });
        let (Some(sa), Some(sb)) = (sa, sb) else {
            unreachable!("literals are non-empty")
        };
        let ta = Tainted::new(1u8, sa.clone());
        let tb = Tainted::new(2u8, sb.clone());
        let joined = ta.join(tb, |x, y| x.wrapping_add(*y));
        assert!(joined.taint().contains(&sa));
        assert!(joined.taint().contains(&sb));
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
    use proptest::prelude::*;

    fn source(label: &str) -> TaintSource {
        TaintSource::new(label).unwrap()
    }

    #[test]
    fn empty_label_is_not_a_source() {
        assert!(TaintSource::new("").is_none());
    }

    #[test]
    fn map_keeps_the_full_set_and_join_unions() {
        let web = Tainted::new("body".to_owned(), source("web:example.com"));
        let derived = web.map(|s| s.len());
        assert_eq!(derived.taint().len(), 1);
        let upload = Tainted::new(3usize, source("file:upload"));
        let joined = derived.join(upload, |a, b| a.saturating_add(*b));
        assert_eq!(*joined.peek(), 7);
        assert!(joined.taint().contains(&source("web:example.com")));
        assert!(joined.taint().contains(&source("file:upload")));
    }

    #[test]
    fn peek_lends_without_moving() {
        let tainted = Tainted::new(vec![1u8, 2], source("mail:inbound"));
        assert_eq!(tainted.peek().len(), 2);
        // Still usable afterwards: peek did not consume.
        assert_eq!(tainted.taint().len(), 1);
    }

    proptest! {
        /// The kani mirror (union monotonicity), runnable on every host.
        #[test]
        fn join_output_contains_both_inputs(labels_a in proptest::collection::vec("[a-z]{1,8}", 1..4),
                                            labels_b in proptest::collection::vec("[a-z]{1,8}", 1..4)) {
            let mut a = Tainted::new(0u32, source(&format!("a:{}", labels_a[0])));
            for label in &labels_a[1..] {
                let extra = Tainted::new(1u32, source(&format!("a:{label}")));
                a = a.join(extra, |x, y| x.saturating_add(*y));
            }
            let mut b = Tainted::new(0u32, source(&format!("b:{}", labels_b[0])));
            for label in &labels_b[1..] {
                let extra = Tainted::new(1u32, source(&format!("b:{label}")));
                b = b.join(extra, |x, y| x.saturating_add(*y));
            }
            let a_set = a.taint().clone();
            let b_set = b.taint().clone();
            let joined = a.join(b, |x, y| x.saturating_add(*y));
            for s in a_set.sources() {
                prop_assert!(joined.taint().contains(s));
            }
            for s in b_set.sources() {
                prop_assert!(joined.taint().contains(s));
            }
            prop_assert_eq!(joined.taint().len(), a_set.union(&b_set).len());
        }
    }
}
