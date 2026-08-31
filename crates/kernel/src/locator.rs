// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Locator: the one grammar for referring to content across events and
//! sessions. Two schemes only:
//!
//! `cas:b3-<hex64>[#(L|B)<a>-<b>]` | `file:<address>@<hex40>[#(L|B)<a>-<b>]`
//!
//! Invariants owned here:
//! - fail-closed: whatever is not exactly the grammar is `E_LOCATOR_INVALID`;
//!   unknown schemes and unknown algorithm tags are errors, never fallbacks.
//! - canonical echo: `parse` additionally requires `Display(parsed) == raw`,
//!   which closes every non-canonical spelling (uppercase hex, leading
//!   zeros, `+` signs) with one rule.
//! - `secret:` never parses here: SecretRef has a separate parser by
//!   design, so a read path can never redeem a credential.
//! - range semantics: `L` is 1-based closed (editor precedent), `B` is
//!   0-based closed (HTTP Range precedent); `from <= to` always.
//!
//! [`B3Hash`] lives here because `b3-` is part of this grammar; the event
//! chain and CAS reuse the same type (one hash function library-wide).

use serde::{Deserialize, Serialize};

use crate::address::Address;
use crate::error::{AxCode, AxError};

/// BLAKE3 digest, 32 bytes; displayed as 64 lowercase hex digits.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct B3Hash([u8; 32]);

impl B3Hash {
    pub const fn from_bytes(bytes: [u8; 32]) -> Self {
        B3Hash(bytes)
    }

    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// The library-wide content-hash producer. Every non-chain hash (prefix
    /// segment hashes, stall fingerprints) comes through here so blake3 has
    /// exactly one home outside `chain_hash`.
    pub fn digest(bytes: &[u8]) -> Self {
        B3Hash(*blake3::hash(bytes).as_bytes())
    }

    fn parse_hex(raw: &str) -> Option<Self> {
        decode_hex_fixed::<32>(raw).map(B3Hash)
    }
}

impl std::fmt::Display for B3Hash {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_hex(f, &self.0)
    }
}

impl Serialize for B3Hash {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for B3Hash {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        B3Hash::parse_hex(&raw)
            .ok_or_else(|| serde::de::Error::custom("expected exactly 64 lowercase hex digits"))
    }
}

/// Git object id, 40 lowercase hex digits (SHA-1 repositories; other
/// lengths are rejected until a second length ships with git2 at S3).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct GitOid([u8; 20]);

impl GitOid {
    pub fn from_bytes(bytes: [u8; 20]) -> Self {
        GitOid(bytes)
    }

    /// The hex spelling, read back.
    ///
    /// Public because a checkpoint identity travels as a string in more
    /// than one direction: the wire deserialises one, and a client that
    /// was shown a checkpoint in a sentence has to turn it back into an
    /// oid to act on it. Without this the caller decodes its own hex,
    /// which is a second definition of what an oid looks like - the exact
    /// thing the note on `Deserialize` below says stays here.
    ///
    /// `None` on anything that is not exactly forty lowercase hex digits:
    /// a wrong length is a refusal, never a padded guess.
    #[must_use]
    pub fn parse(raw: &str) -> Option<Self> {
        Self::parse_hex(raw)
    }

    fn parse_hex(raw: &str) -> Option<Self> {
        decode_hex_fixed::<20>(raw).map(GitOid)
    }
}

impl std::fmt::Display for GitOid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write_hex(f, &self.0)
    }
}

impl Serialize for GitOid {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for GitOid {
    /// Same shape discipline as `B3Hash` above: the hex spelling is the only
    /// accepted form, and a wrong length is a refusal rather than a padded
    /// guess. Needed once a checkpoint identity has to cross the process
    /// boundary (`channels::wire`, S4.02); the shape authority stays here so
    /// the wire does not grow a second definition of what an oid looks like.
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        GitOid::parse_hex(&raw)
            .ok_or_else(|| serde::de::Error::custom("expected exactly 40 lowercase hex digits"))
    }
}

/// Sub-content range. `Lines` is 1-based closed; `Bytes` is 0-based closed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Range {
    Lines { from: u64, to: u64 },
    Bytes { from: u64, to: u64 },
}

impl Range {
    /// 1-based closed line range; rejects `from == 0` and `from > to`.
    pub fn lines(from: u64, to: u64) -> Result<Self, AxError> {
        if from == 0 || from > to {
            return Err(range_error("lines", from, to));
        }
        Ok(Range::Lines { from, to })
    }

    /// 0-based closed byte range; rejects `from > to`.
    pub fn bytes(from: u64, to: u64) -> Result<Self, AxError> {
        if from > to {
            return Err(range_error("bytes", from, to));
        }
        Ok(Range::Bytes { from, to })
    }
}

fn range_error(unit: &str, from: u64, to: u64) -> AxError {
    AxError::failure(
        AxCode::InvalidArgs,
        "construct range",
        format!("{unit} {from}-{to}"),
    )
    .with_recovery("require from <= to; line ranges start at 1, byte ranges at 0")
}

impl std::fmt::Display for Range {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Range::Lines { from, to } => write!(f, "L{from}-{to}"),
            Range::Bytes { from, to } => write!(f, "B{from}-{to}"),
        }
    }
}

/// The two-scheme reference grammar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Locator {
    Cas {
        hash: B3Hash,
        range: Option<Range>,
    },
    File {
        address: Address,
        oid: GitOid,
        range: Option<Range>,
    },
}

impl Locator {
    /// Fail-closed parser: exact grammar or `E_LOCATOR_INVALID`.
    pub fn parse(raw: &str) -> Result<Self, AxError> {
        let parsed = Locator::parse_inner(raw)?;
        // Canonical echo: one rule closes every non-canonical spelling.
        let echo = parsed.to_string();
        if echo != raw {
            return Err(invalid(
                raw,
                format!("non-canonical spelling; use `{echo}`"),
            ));
        }
        Ok(parsed)
    }

    fn parse_inner(raw: &str) -> Result<Self, AxError> {
        if let Some(rest) = raw.strip_prefix("cas:") {
            let (body, range) = split_range(raw, rest)?;
            let hex = body.strip_prefix("b3-").ok_or_else(|| {
                invalid(
                    raw,
                    "unknown or missing algorithm tag; the only tag is `b3-`",
                )
            })?;
            let hash = B3Hash::parse_hex(hex)
                .ok_or_else(|| invalid(raw, "hash must be exactly 64 lowercase hex digits"))?;
            return Ok(Locator::Cas { hash, range });
        }
        if let Some(rest) = raw.strip_prefix("file:") {
            let (body, range) = split_range(raw, rest)?;
            let (addr_raw, oid_raw) = body
                .rsplit_once('@')
                .ok_or_else(|| invalid(raw, "file locator needs `<address>@<git-oid>`"))?;
            let address = Address::parse(addr_raw)
                .map_err(|_| invalid(raw, "address part violates the address grammar"))?;
            let oid = GitOid::parse_hex(oid_raw)
                .ok_or_else(|| invalid(raw, "git oid must be exactly 40 lowercase hex digits"))?;
            return Ok(Locator::File {
                address,
                oid,
                range,
            });
        }
        if raw.starts_with("secret:") {
            return Err(invalid(
                raw,
                "SecretRef never enters the locator grammar (separate parser by design)",
            ));
        }
        Err(invalid(
            raw,
            "unknown scheme; only `cas:` and `file:` exist",
        ))
    }
}

impl std::fmt::Display for Locator {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Locator::Cas { hash, range } => {
                write!(f, "cas:b3-{hash}")?;
                write_range(f, range)
            }
            Locator::File {
                address,
                oid,
                range,
            } => {
                write!(f, "file:{address}@{oid}")?;
                write_range(f, range)
            }
        }
    }
}

impl Serialize for Locator {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Locator {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        Locator::parse(&raw).map_err(serde::de::Error::custom)
    }
}

fn invalid(raw: &str, violation: impl Into<String>) -> AxError {
    AxError::failure(AxCode::LocatorInvalid, "parse locator", raw).with_recovery(violation.into())
}

/// Splits an optional `#(L|B)<a>-<b>` suffix off `rest`; `raw` only feeds
/// error context.
fn split_range(raw: &str, rest: &str) -> Result<(String, Option<Range>), AxError> {
    match rest.split_once('#') {
        None => Ok((rest.to_owned(), None)),
        Some((body, fragment)) => {
            let range = parse_range(raw, fragment)?;
            Ok((body.to_owned(), Some(range)))
        }
    }
}

fn parse_range(raw: &str, fragment: &str) -> Result<Range, AxError> {
    let mut chars = fragment.chars();
    let unit = chars
        .next()
        .ok_or_else(|| invalid(raw, "empty range fragment"))?;
    let body: &str = chars.as_str();
    let (from_raw, to_raw) = body
        .split_once('-')
        .ok_or_else(|| invalid(raw, "range needs `<from>-<to>`"))?;
    let from = parse_canonical_u64(raw, from_raw)?;
    let to = parse_canonical_u64(raw, to_raw)?;
    let range = match unit {
        'L' => Range::lines(from, to),
        'B' => Range::bytes(from, to),
        _ => {
            return Err(invalid(
                raw,
                "range unit must be `L` (lines) or `B` (bytes)",
            ));
        }
    };
    range.map_err(|_| invalid(raw, "range bounds are out of order or out of base"))
}

/// Digits only, no leading zeros (except "0" itself), no signs.
fn parse_canonical_u64(raw: &str, digits: &str) -> Result<u64, AxError> {
    if digits.is_empty() || !digits.chars().all(|c| c.is_ascii_digit()) {
        return Err(invalid(raw, "range bound must be decimal digits"));
    }
    if digits.len() > 1 && digits.starts_with('0') {
        return Err(invalid(raw, "range bound must not carry leading zeros"));
    }
    digits
        .parse::<u64>()
        .map_err(|_| invalid(raw, "range bound exceeds u64"))
}

pub(crate) fn write_hex(f: &mut std::fmt::Formatter<'_>, bytes: &[u8]) -> std::fmt::Result {
    for b in bytes {
        write!(f, "{b:02x}")?;
    }
    Ok(())
}

fn write_range(f: &mut std::fmt::Formatter<'_>, range: &Option<Range>) -> std::fmt::Result {
    match range {
        Some(r) => write!(f, "#{r}"),
        None => Ok(()),
    }
}

/// Strict lowercase hex into a fixed-width array; `None` on any deviation.
pub(crate) fn decode_hex_fixed<const N: usize>(raw: &str) -> Option<[u8; N]> {
    if raw.len() != N.checked_mul(2)? {
        return None;
    }
    let mut out = Vec::with_capacity(N);
    let mut bytes = raw.bytes();
    while let Some(hi) = bytes.next() {
        let lo = bytes.next()?;
        let value = nibble(hi)?.checked_mul(16)?.checked_add(nibble(lo)?)?;
        out.push(value);
    }
    <[u8; N]>::try_from(out.as_slice()).ok()
}

/// Lowercase-only nibble; uppercase is non-canonical and refused.
fn nibble(c: u8) -> Option<u8> {
    match c {
        b'0'..=b'9' => c.checked_sub(b'0'),
        b'a'..=b'f' => c.checked_sub(b'a')?.checked_add(10),
        _ => None,
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

    const H64: &str = "aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";
    const OID: &str = "0123456789abcdef0123456789abcdef01234567";

    #[test]
    fn cas_locator_roundtrips() {
        let raw = format!("cas:b3-{H64}");
        let loc = Locator::parse(&raw).unwrap();
        assert_eq!(loc.to_string(), raw);
        let ranged = format!("cas:b3-{H64}#L10-20");
        assert_eq!(Locator::parse(&ranged).unwrap().to_string(), ranged);
        let bytes = format!("cas:b3-{H64}#B0-1023");
        assert_eq!(Locator::parse(&bytes).unwrap().to_string(), bytes);
    }

    #[test]
    fn file_locator_roundtrips_and_splits_on_last_at() {
        let raw = format!("file:role@building.1/notes.md@{OID}");
        let loc = Locator::parse(&raw).unwrap();
        assert_eq!(loc.to_string(), raw);
        match loc {
            Locator::File { address, .. } => {
                assert_eq!(address.as_str(), "role@building.1/notes.md");
            }
            Locator::Cas { .. } => panic!("expected file locator"),
        }
    }

    #[test]
    fn fail_closed_on_every_malformed_shape() {
        let cases = [
            "".to_string(),
            "cas:".to_string(),
            format!("CAS:b3-{H64}"),                  // scheme case
            format!("cas:B3-{H64}"),                  // tag case
            format!("cas:sha256-{H64}"),              // unknown algorithm tag
            "cas:b3-abc".to_string(),                 // short hex
            format!("cas:b3-{}", H64.to_uppercase()), // uppercase hex
            format!("cas:b3-{H64}#L0-5"),             // lines are 1-based
            format!("cas:b3-{H64}#L9-5"),             // from > to
            format!("cas:b3-{H64}#B01-2"),            // leading zero (non-canonical)
            format!("cas:b3-{H64}#B+1-2"),            // plus sign
            format!("cas:b3-{H64}#X1-2"),             // unknown range unit
            format!("cas:b3-{H64}#L1-2 "),            // trailing junk
            format!("file:notes.md@{OID}#"),          // empty range
            "file:notes.md".to_string(),              // missing oid
            format!("file:../up@{OID}"),              // address grammar inside
            format!("file:notes.md@{}", OID.get(..39).unwrap()), // short oid
            "secret:openai/key".to_string(),          // SecretRef never parses here
            format!("cas:b3-{H64}extra"),             // overlong
        ];
        for bad in &cases {
            let err = Locator::parse(bad).unwrap_err();
            assert_eq!(err.code(), &AxCode::LocatorInvalid, "should reject {bad:?}");
        }
    }

    #[test]
    fn range_constructor_validates_bounds() {
        assert!(Range::lines(1, 1).is_ok());
        assert!(Range::lines(0, 4).is_err());
        assert!(Range::bytes(0, 0).is_ok());
        assert!(Range::bytes(9, 3).is_err());
    }

    #[test]
    fn b3hash_hex_roundtrip() {
        let h = B3Hash::from_bytes([0xab; 32]);
        let hex = h.to_string();
        assert_eq!(hex.len(), 64);
        assert_eq!(hex, "ab".repeat(32));
    }

    #[test]
    fn serde_is_the_string_form() {
        let raw = format!("cas:b3-{H64}#B0-9");
        let loc: Locator = serde_json::from_str(&format!("\"{raw}\"")).unwrap();
        assert_eq!(serde_json::to_string(&loc).unwrap(), format!("\"{raw}\""));
    }

    proptest::proptest! {
        #[test]
        fn parse_display_roundtrip_is_identity_on_accepted_inputs(
            hexbytes in proptest::collection::vec(0u8..=255, 32),
            unit in 0..=1u8, a in 1u64..1000, span in 0u64..1000,
        ) {
            let h = B3Hash::from_bytes(<[u8; 32]>::try_from(hexbytes.as_slice()).unwrap());
            let b = a.checked_add(span).unwrap();
            let range = if unit == 0 { format!("#L{a}-{b}") } else { format!("#B{a}-{b}") };
            let raw = format!("cas:b3-{h}{range}");
            let parsed = Locator::parse(&raw).unwrap();
            proptest::prop_assert_eq!(parsed.to_string(), raw);
        }

        #[test]
        fn arbitrary_strings_never_panic(s in "\\PC*") {
            let _ = Locator::parse(&s);
        }
    }
}
