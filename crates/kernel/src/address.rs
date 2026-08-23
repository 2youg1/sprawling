// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Address: canonical relative path inside a city, and
//! the WriteDomain primitive `is_within`.
//!
//! Invariants owned here:
//! - one constructor: [`Address::parse`]; whatever it accepts is already
//!   canonical (no normalization happens, non-canonical spellings are
//!   rejected), so `as_str` is a byte-exact round trip.
//! - comparison is byte-wise on every platform; Windows case aliases are
//!   two different addresses on purpose.
//! - symlink resolution is an effect-layer concern; this type only judges
//!   already-canonical relative paths.
//! - `is_reserved` answers C17: a `.sprawling` subtree can never enter a
//!   WriteDomain, at whatever depth it sits, and every WriteDomain
//!   constructor must ask first.

use serde::{Deserialize, Serialize};

use crate::error::{AxCode, AxError};

/// The directory name whose subtree never enters any WriteDomain (C17).
///
/// One of these belongs to each scope that has rules of its own: the
/// city's holds the ledger and the city configuration, a building's
/// holds the rules that govern it. What governs a scope is therefore
/// never writable by what runs inside it.
pub const RESERVED_PREFIX: &str = ".sprawling";

/// Canonical relative path; invariants enforced at the sole constructor.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct Address(String);

impl Address {
    /// Sole constructor. Rejects: empty input, absolute paths, backslash,
    /// `:` (drive letters and NTFS alternate data streams alike), NUL and
    /// control characters, empty segments (covers leading/trailing and
    /// doubled `/`), and `.`/`..` segments. Fail-closed: anything not
    /// exactly canonical is `E_INVALID_ARGS`.
    pub fn parse(raw: &str) -> Result<Self, AxError> {
        let reject = |violation: &str| {
            Err(
                AxError::failure(AxCode::InvalidArgs, "parse address", raw).with_recovery(format!(
                    "{violation}; use a canonical relative path: `/`-separated \
                     segments without `.`/`..`, backslash, `:`, or control characters"
                )),
            )
        };
        if raw.is_empty() {
            return reject("address is empty");
        }
        if raw.starts_with('/') {
            return reject("address is absolute");
        }
        if raw.contains('\\') {
            return reject("address contains a backslash");
        }
        if raw.contains(':') {
            return reject("address contains `:`");
        }
        if raw.chars().any(char::is_control) {
            return reject("address contains a control character");
        }
        for segment in raw.split('/') {
            if segment.is_empty() {
                return reject("address contains an empty segment");
            }
            if segment == "." || segment == ".." {
                return reject("address contains a `.` or `..` segment");
            }
        }
        Ok(Address(raw.to_owned()))
    }

    /// WriteDomain primitive: true iff `self` equals `prefix` or lies under
    /// it on a segment boundary. Byte-wise, reflexive, transitive.
    pub fn is_within(&self, prefix: &Address) -> bool {
        match self.0.strip_prefix(&prefix.0) {
            Some(rest) => rest.is_empty() || rest.starts_with('/'),
            None => false,
        }
    }

    /// C17 primitive: true iff any segment is [`RESERVED_PREFIX`].
    ///
    /// Any segment rather than the first: a building's own rules sit at
    /// `<building>/.sprawling/`, and a run whose write domain is its
    /// building must not reach them. Widening this predicate can only
    /// refuse more, which is the direction a fail-closed check may move
    /// in without a second authority to check it against.
    pub fn is_reserved(&self) -> bool {
        self.0.split('/').any(|segment| segment == RESERVED_PREFIX)
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for Address {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl<'de> Deserialize<'de> for Address {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Address::parse(&raw).map_err(serde::de::Error::custom)
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
    use crate::error::AxCode;

    #[test]
    fn parse_accepts_canonical_relative_paths() {
        for ok in [
            "a",
            "a/b",
            "docs/notes.md",
            "role@building.1/JOB.md",
            ".sprawling/ledger",
        ] {
            let addr = Address::parse(ok).unwrap();
            assert_eq!(addr.as_str(), ok);
            assert_eq!(addr.to_string(), ok);
        }
    }

    #[test]
    fn parse_rejects_every_banned_form() {
        for bad in [
            "",           // empty
            "/abs",       // absolute
            "a//b",       // empty segment
            "a/",         // trailing slash
            "/",          // both
            "..",         // parent escape
            "a/../b",     // parent escape inside
            ".",          // dot segment
            "a/./b",      // dot segment inside
            "a\\b",       // backslash
            "C:/x",       // drive letter (colon)
            "a/b:stream", // NTFS ADS (colon)
            "a\u{0}b",    // NUL
            "a\tb",       // control character
        ] {
            let err = Address::parse(bad).unwrap_err();
            assert_eq!(err.code(), &AxCode::InvalidArgs, "should reject {bad:?}");
        }
    }

    #[test]
    fn is_within_respects_segment_boundaries() {
        let a = Address::parse("a/b/c").unwrap();
        let prefix = Address::parse("a/b").unwrap();
        let stranger = Address::parse("a/bc").unwrap();
        assert!(a.is_within(&prefix));
        assert!(a.is_within(&a));
        assert!(!stranger.is_within(&prefix));
        assert!(!prefix.is_within(&a));
    }

    #[test]
    fn a_reserved_subtree_is_reserved_at_whatever_depth_it_sits() {
        assert!(Address::parse(".sprawling").unwrap().is_reserved());
        assert!(Address::parse(".sprawling/ledger/x").unwrap().is_reserved());
        assert!(!Address::parse(".sprawlingx/a").unwrap().is_reserved());
        // The rule this card widened. A building keeps what governs it -
        // its rules, its configuration, its own skills - in a reserved
        // subtree of its own, and the run that works in that building has
        // the whole building as its write domain unless BUILDING.md says
        // otherwise. Reserved on the first segment only meant the file
        // declaring the write domain sat inside the write domain.
        assert!(Address::parse("lab/.sprawling").unwrap().is_reserved());
        assert!(
            Address::parse("lab/.sprawling/CONFIG.toml")
                .unwrap()
                .is_reserved()
        );
        assert!(
            Address::parse("lab/room1/.sprawling/CONFIG.toml")
                .unwrap()
                .is_reserved()
        );
        assert!(
            !Address::parse("lab/sprawling/notes.md")
                .unwrap()
                .is_reserved(),
            "a segment that merely looks like it is not it"
        );
    }

    #[test]
    fn deserialize_revalidates() {
        let ok: Address = serde_json::from_str("\"a/b\"").unwrap();
        assert_eq!(ok.as_str(), "a/b");
        assert!(serde_json::from_str::<Address>("\"../up\"").is_err());
    }

    proptest::proptest! {
        #[test]
        fn accepted_addresses_roundtrip_and_never_contain_banned_parts(
            segs in proptest::collection::vec("[a-zA-Z0-9._@-]{1,8}", 1..5)
        ) {
            let raw = segs.join("/");
            if let Ok(addr) = Address::parse(&raw) {
                proptest::prop_assert_eq!(addr.as_str(), raw.as_str());
                proptest::prop_assert!(!raw.split('/').any(|s| s.is_empty() || s == "." || s == ".."));
            }
        }

        #[test]
        fn is_within_agrees_with_string_prefix_plus_boundary(
            a in "[a-z]{1,3}(/[a-z]{1,3}){0,3}",
            b in "[a-z]{1,3}(/[a-z]{1,3}){0,3}",
        ) {
            let x = Address::parse(&a).unwrap();
            let y = Address::parse(&b).unwrap();
            let expect = a == b || a.starts_with(&format!("{b}/"));
            proptest::prop_assert_eq!(x.is_within(&y), expect);
        }
    }
}
