// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! What guards the door one serve opens (sprawling-SPEC.md section
//! 8-22).
//!
//! Pure, and pure on purpose: the entropy a minted key is made of is
//! drawn in `bin::assembly`, which is where this crate draws randomness,
//! and `channels::auth` documents that arrangement from the other side
//! ("Entropy arrives as a parameter. This module samples nothing").
//! What is left here is the policy, which is four cells over two facts
//! and therefore something a test can state in full.
//!
//! This decides only where a key comes from. Whether the listener may
//! bind at all stays with `channels::decide_bind`, and nothing here
//! weakens it: the assembly layer satisfies that guard before the socket
//! exists rather than moving it.

use std::net::SocketAddr;

/// Where this serve's pairing key comes from.
///
/// Exhaustive over the two facts that decide it — whether the address
/// reaches beyond this machine, and whether the operator configured a
/// token — because "no key" and "a key nobody has to be shown" are
/// different situations and the caller prints different things about
/// them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Keying {
    /// A loopback listener asks for nothing, so nothing is minted and
    /// nothing is shown. Local use stays frictionless, which is the
    /// property `decide_bind` exists to preserve.
    NothingToPresent,
    /// The operator configured one. It is adopted as it stands and never
    /// displayed: we did not mint it, so printing it would only copy a
    /// standing secret into one more place that keeps text.
    Adopt,
    /// Nothing was configured and the address reaches past this machine.
    /// One key is minted for this serve, shown once, and forgotten when
    /// the process ends — which is the whole of what makes it one-time.
    Mint,
}

impl Keying {
    /// The four cells, with the configured value taking precedence
    /// wherever it exists.
    ///
    /// A configured token is honoured even on loopback so that a person
    /// can rehearse the exposed setup locally; the alternative silently
    /// ignores what they configured and then behaves differently the one
    /// time it matters.
    pub(crate) fn decide(bind: SocketAddr, configured: bool) -> Self {
        match (configured, bind.ip().is_loopback()) {
            (true, _) => Self::Adopt,
            (false, true) => Self::NothingToPresent,
            (false, false) => Self::Mint,
        }
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
    use super::Keying;

    fn at(raw: &str) -> std::net::SocketAddr {
        raw.parse().unwrap()
    }

    /// The whole policy, stated as the table it is. Written out cell by
    /// cell rather than as two branches, because the value of an
    /// exhaustive enum is that the reader can see every case at once.
    #[test]
    fn the_four_cells_are_these_four() {
        assert_eq!(
            Keying::decide(at("127.0.0.1:8787"), false),
            Keying::NothingToPresent
        );
        assert_eq!(Keying::decide(at("127.0.0.1:8787"), true), Keying::Adopt);
        assert_eq!(Keying::decide(at("0.0.0.0:8787"), true), Keying::Adopt);
        assert_eq!(Keying::decide(at("0.0.0.0:8787"), false), Keying::Mint);
    }

    /// The cell that used to be a refusal. An address reachable from
    /// elsewhere with nothing configured was the one combination
    /// `decide_bind` turned away; it now grows a key instead, and the
    /// guard is satisfied rather than moved.
    #[test]
    fn an_address_beyond_this_machine_never_ends_up_without_a_key() {
        for raw in ["0.0.0.0:8787", "192.168.1.10:8787", "[::]:8787"] {
            assert_ne!(
                Keying::decide(at(raw), false),
                Keying::NothingToPresent,
                "{raw} reaches past this machine"
            );
        }
    }

    /// IPv6 loopback is loopback. Spelling the check as `is_loopback`
    /// rather than comparing against `127.0.0.1` is what makes this true
    /// without a second branch.
    #[test]
    fn the_other_spelling_of_loopback_is_also_loopback() {
        assert_eq!(
            Keying::decide(at("[::1]:8787"), false),
            Keying::NothingToPresent
        );
    }
}
