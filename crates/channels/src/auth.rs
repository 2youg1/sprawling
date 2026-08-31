// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Pairing tokens: minting, the one form a person may read, and the
//! comparison. Local use is frictionless - a loopback listener asks for
//! nothing. A listener reachable from elsewhere on the network asks for a
//! token, and `server::decide_bind` refuses to start without one.
//!
//! The token's whole life is here so that no other module needs its
//! plaintext: callers hand [`PairingToken::digest`] to the server and keep
//! the token itself, and comparison happens on digests.
//!
//! Entropy arrives as a parameter. This module samples nothing - the seeded
//! RNG is handed down from the assembly layer,
//! which is also what makes minting testable.

use kernel::{AxCode, AxError, B3Hash};

/// Digits and letters that survive being read aloud and typed back:
/// `0/O`, `1/l/I` and `5/S` are absent because a pairing code is transcribed
/// by a person, and a code that cannot be dictated fails in the one moment
/// it exists for.
const ALPHABET: &[u8; 29] = b"2346789abcdefghjkmnpqrtuvwxyz";

/// Characters per readable group, and number of groups. Four groups of five
/// over a 29-symbol alphabet is a little over 97 bits - far beyond what an
/// attacker on the same network segment can exhaust, and still short enough
/// to read to somebody in the next room.
const GROUP_LEN: usize = 5;
const GROUP_COUNT: usize = 4;

/// The shortest configured token this module will accept. Below this a token
/// is decoration; refusing it is cheaper than explaining later why the port
/// was open.
const MIN_CONFIGURED_LEN: usize = 16;

/// A pairing token, held as the digest of the code and nothing else.
///
/// The plaintext never lives in this type. [`PairingToken::mint`] hands the
/// readable code straight back to its caller, who shows it to a person once,
/// and keeps only what verification needs. Sealing a value and then exposing
/// it on the next line would be theatre; not holding it at all is the
/// property we actually wanted.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PairingToken(B3Hash);

impl PairingToken {
    /// Mints a token from caller-supplied entropy, returning the token and
    /// the code to display exactly once.
    ///
    /// Deterministic in `entropy`: same bytes, same token. The randomness
    /// lives in whoever fills `entropy`, which is the assembly layer's seeded
    /// generator, never a sample taken here.
    #[must_use]
    pub fn mint(entropy: [u8; 32]) -> (Self, String) {
        let mut text = String::new();
        for (index, byte) in entropy
            .iter()
            .take(GROUP_LEN.saturating_mul(GROUP_COUNT))
            .enumerate()
        {
            if index != 0 && index.checked_rem(GROUP_LEN) == Some(0) {
                text.push('-');
            }
            let slot = usize::from(*byte)
                .checked_rem(ALPHABET.len())
                .unwrap_or_default();
            if let Some(symbol) = ALPHABET.get(slot) {
                text.push(char::from(*symbol));
            }
        }
        let token = Self(B3Hash::digest(text.as_bytes()));
        (token, text)
    }

    /// Adopts a token the operator configured rather than one we minted.
    ///
    /// # Errors
    /// Refuses anything too short to be worth comparing.
    pub fn from_configured(raw: &str) -> Result<Self, AxError> {
        if raw.chars().count() < MIN_CONFIGURED_LEN {
            return Err(AxError::failure(
                AxCode::ConfigInvalid,
                "adopt a configured pairing token",
                format!("a pairing token needs at least {MIN_CONFIGURED_LEN} characters"),
            )
            .with_recovery("let the host mint one, or configure a longer value"));
        }
        Ok(Self(B3Hash::digest(raw.as_bytes())))
    }

    /// What the server is given, and all it ever needs.
    #[must_use]
    pub fn digest(&self) -> B3Hash {
        self.0
    }
}

/// Compares a presented token against a stored digest in constant time.
///
/// `None` is not an empty string: a peer that sent no token at all is
/// refused without ever entering the comparison, and a peer that sent an
/// empty one still pays the full comparison so the two are indistinguishable
/// from outside.
#[must_use]
pub fn verify(presented: Option<&str>, expected: &B3Hash) -> bool {
    let Some(presented) = presented else {
        return false;
    };
    let offered = B3Hash::digest(presented.as_bytes());
    let mut difference = 0u8;
    for (a, b) in offered.as_bytes().iter().zip(expected.as_bytes().iter()) {
        difference |= a ^ b;
    }
    difference == 0
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

    #[test]
    fn the_alphabet_omits_every_pair_a_person_would_confuse() {
        for confusable in *b"0O1lI5S" {
            assert!(
                !ALPHABET.contains(&confusable),
                "{} is transcribed wrong often enough to matter",
                char::from(confusable)
            );
        }
        let mut deduped = ALPHABET.to_vec();
        deduped.sort_unstable();
        deduped.dedup();
        assert_eq!(deduped.len(), ALPHABET.len(), "no symbol appears twice");
    }

    #[test]
    fn a_minted_code_has_the_shape_the_constants_promise() {
        let (_token, shown) = PairingToken::mint([0x44u8; 32]);
        let groups: Vec<&str> = shown.split('-').collect();
        assert_eq!(groups.len(), GROUP_COUNT);
        for group in groups {
            assert_eq!(group.chars().count(), GROUP_LEN);
        }
    }

    #[test]
    fn verification_is_blind_to_how_long_the_wrong_guess_was() {
        let (token, _shown) = PairingToken::mint([0x55u8; 32]);
        let expected = token.digest();
        assert!(!verify(Some(""), &expected));
        assert!(!verify(Some("a"), &expected));
        assert!(!verify(Some(&"a".repeat(4096)), &expected));
    }
}
