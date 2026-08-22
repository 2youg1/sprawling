// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! IdemKey: the dedup key for outward actions.
//!
//! Derivation is the whole point: `BLAKE3-XOF(run(16B) || seq(8B LE) ||
//! action_canonical)` taken as a direct 16-byte output (not a truncation),
//! plus one version byte carried alongside so a future derivation change
//! cannot collide old keys with new ones. No `From<Uuid>`, no randomness,
//! no timestamps: resume and replay must re-derive the identical key, or
//! the double-payment defense dies exactly on the recovery path.

use serde::{Deserialize, Serialize};

use crate::event::{RunId, Seq};

/// Current derivation-scheme version byte.
pub const IDEM_DERIVE_V: u8 = 1;

/// Idempotency key: 16 digest bytes plus the derivation version.
/// Private fields; [`IdemKey::derive`] is the only mint.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct IdemKey {
    v: u8,
    digest: [u8; 16],
}

impl IdemKey {
    /// The sole construction point (`kernel::idem::derive`).
    /// Fixed-width run and seq make the framing injective without length
    /// prefixes; the action canonicalization rule is the tool layer's (S2).
    pub fn derive(run: &RunId, seq: Seq, action_canonical: &[u8]) -> IdemKey {
        let mut hasher = blake3::Hasher::new();
        hasher.update(run.as_bytes());
        hasher.update(&seq.value().to_le_bytes());
        hasher.update(action_canonical);
        let mut digest = [0u8; 16];
        hasher.finalize_xof().fill(&mut digest);
        IdemKey {
            v: IDEM_DERIVE_V,
            digest,
        }
    }

    fn parse(raw: &str) -> Option<IdemKey> {
        let rest = raw.strip_prefix("idem")?;
        let (v_raw, hex) = rest.split_once('-')?;
        if v_raw != "1" {
            return None;
        }
        let digest = crate::locator::decode_hex_fixed::<16>(hex)?;
        Some(IdemKey {
            v: IDEM_DERIVE_V,
            digest,
        })
    }
}

impl std::fmt::Display for IdemKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "idem{}-", self.v)?;
        crate::locator::write_hex(f, &self.digest)
    }
}

impl Serialize for IdemKey {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for IdemKey {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let raw = String::deserialize(deserializer)?;
        IdemKey::parse(&raw).ok_or_else(|| {
            serde::de::Error::custom("expected `idem1-` plus 32 lowercase hex digits")
        })
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
    use crate::event::{RunId, Seq};

    fn run() -> RunId {
        RunId::from_bytes([7; 16])
    }

    #[test]
    fn same_inputs_same_key_always() {
        let a = IdemKey::derive(&run(), Seq::new(42), b"tool:exec{cmd}");
        let b = IdemKey::derive(&run(), Seq::new(42), b"tool:exec{cmd}");
        assert_eq!(a, b);
    }

    #[test]
    fn any_input_change_changes_the_key() {
        let base = IdemKey::derive(&run(), Seq::new(42), b"action");
        assert_ne!(base, IdemKey::derive(&run(), Seq::new(43), b"action"));
        assert_ne!(
            base,
            IdemKey::derive(&RunId::from_bytes([8; 16]), Seq::new(42), b"action")
        );
        assert_ne!(base, IdemKey::derive(&run(), Seq::new(42), b"actioN"));
    }

    #[test]
    fn display_carries_the_derivation_version() {
        let key = IdemKey::derive(&run(), Seq::FIRST, b"x");
        let text = key.to_string();
        assert!(text.starts_with("idem1-"), "{text}");
        assert_eq!(text.len(), "idem1-".len() + 32, "16 bytes as 32 hex digits");
    }

    #[test]
    fn serde_roundtrips_the_string_form() {
        let key = IdemKey::derive(&run(), Seq::new(5), b"payload");
        let json = serde_json::to_string(&key).unwrap();
        let back: IdemKey = serde_json::from_str(&json).unwrap();
        assert_eq!(back, key);
        assert!(serde_json::from_str::<IdemKey>("\"idem1-zz\"").is_err());
        assert!(
            serde_json::from_str::<IdemKey>("\"idem9-00000000000000000000000000000000\"").is_err()
        );
    }

    proptest::proptest! {
        #[test]
        fn rederivation_is_identity(
            run_bytes in proptest::array::uniform16(0u8..=255),
            seq in 0u64..,
            action in proptest::collection::vec(0u8..=255, 0..64),
        ) {
            let r = RunId::from_bytes(run_bytes);
            let first = IdemKey::derive(&r, Seq::new(seq), &action);
            let second = IdemKey::derive(&r, Seq::new(seq), &action);
            proptest::prop_assert_eq!(first, second);
        }
    }
}
