// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Every judgement the process boundary makes, as pure functions with
//! exhaustive verdicts.
//!
//! Four questions, asked in four different places and answered here:
//! may this address be bound at all, may this peer put a credential in
//! the vault, may this peer's opening frame be accepted, and what does
//! a frame mean in the state the session is in. None of them touches a
//! socket, a clock or a file, so all four are tested by calling them.
//!
//! **This is the thick half of the Humble Object** the listener is
//! built as (ARCHITECTURE section 9). `channels::server` is declared an
//! adapter — "thin, no policy" — and policy is what these functions
//! are; a shell that also holds the rules it applies has no way to show
//! that it applies all of them. Every branch left in the shell is a
//! send, a receive, or the end of a session.
//!
//! Binding is loopback by default. Exposing the port demands a pairing
//! token, and a missing token refuses the *start*, not the connection:
//! "data stays on your team" is a judgement or it is decoration.

use std::net::SocketAddr;

use kernel::{Address, AxCode, AxError, B3Hash};

use crate::auth;
use crate::wire::{ClientFrame, Hello, Query, WIRE_V, Welcome, WireCommand, schema_hash};

/// Which face the listener presents. An enum rather than `bool` so the
/// exposed case can never be reached by passing the wrong literal.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindFace {
    Loopback,
    Exposed,
}

/// The whole of the binding policy.
#[derive(Debug)]
pub enum BindVerdict {
    Serve(BindFace),
    Refuse(AxError),
}

/// Decides whether the listener may bind `addr`.
///
/// Pure. Four cells, one of which refuses: an address reachable from outside
/// this machine with no pairing token configured. The refusal happens before
/// the socket exists, so there is no window in which the port is open and
/// unauthenticated.
#[must_use]
pub fn decide_bind(addr: &SocketAddr, token_configured: bool) -> BindVerdict {
    if addr.ip().is_loopback() {
        return BindVerdict::Serve(BindFace::Loopback);
    }
    if token_configured {
        return BindVerdict::Serve(BindFace::Exposed);
    }
    BindVerdict::Refuse(
        AxError::failure(
            AxCode::ConfigInvalid,
            "bind the control surface",
            format!("{addr} is reachable beyond this machine and no pairing token is configured"),
        )
        .with_recovery(
            "configure a pairing token before exposing the port, or bind a loopback address",
        ),
    )
}
/// Whether one peer may enrol a credential.
#[derive(Debug)]
pub enum EnrollVerdict {
    Accept,
    Refuse(AxError),
}

/// Decides whether `peer` may put plaintext into this machine's vault.
///
/// Pure, and the whole of the policy: only a caller on this machine may.
/// A pairing token is not enough, because a token authenticates a person
/// and this rule is about where the bytes travel. The design's guarantee
/// is that a credential cannot be enrolled remotely at all - the socket
/// half is type-level (`PutSecret` has no wire form), and this is the
/// HTTP half, which needs a runtime check because bytes can always be
/// posted at a route.
#[must_use]
pub fn decide_enroll(peer: &SocketAddr) -> EnrollVerdict {
    if peer.ip().is_loopback() {
        return EnrollVerdict::Accept;
    }
    EnrollVerdict::Refuse(
        AxError::failure(
            AxCode::GateDenied,
            "enrol a credential",
            format!("{peer} is not on this machine"),
        )
        .with_recovery(
            "enrol the credential from the machine running sprawling; a tunnelled session can \
             use it afterwards but cannot deliver it",
        ),
    )
}

/// The verdict on one peer's opening frame.
#[derive(Debug)]
pub enum HandshakeVerdict {
    Accept,
    Reject(AxError),
}

/// Decides whether to accept a peer.
///
/// Order is deliberate: protocol agreement is settled before credentials.
/// A browser holding a cached older client is the common case and deserves
/// "refresh", not "wrong password". Pure - `expected` and `configured` are
/// parameters, never read from ambient state.
///
/// `configured` is a digest, not the token. This crate never holds the
/// plaintext of a pairing token: the side that owns the token digests it
/// once, and the boundary compares digests. That keeps credential exposure
/// at the redemption points where it is audited, and it costs nothing here
/// because the comparison hashes both sides anyway.
#[must_use]
pub fn decide_handshake(
    hello: &Hello,
    expected: &Welcome,
    configured: Option<&B3Hash>,
) -> HandshakeVerdict {
    if hello.wire_v != expected.wire_v || hello.schema != expected.schema {
        return HandshakeVerdict::Reject(
            AxError::failure(
                AxCode::WireMismatch,
                "accept a client connection",
                format!(
                    "client speaks wire v{} and this server speaks v{}",
                    hello.wire_v, expected.wire_v
                ),
            )
            .with_recovery("reload the page to fetch the client this server was built with"),
        );
    }
    let Some(expected_digest) = configured else {
        return HandshakeVerdict::Accept;
    };
    if auth::verify(hello.token.as_deref(), expected_digest) {
        return HandshakeVerdict::Accept;
    }
    HandshakeVerdict::Reject(
        AxError::failure(
            AxCode::ConfigInvalid,
            "accept a client connection",
            "the pairing token does not match",
        )
        .with_recovery("re-enter the pairing code shown on the host machine"),
    )
}
/// How far a socket session has got. Two states, because there are two:
/// a peer that has not identified itself, and one that has.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionState {
    AwaitingHello,
    Live,
}

/// What the shell does with one client frame. Exhaustive: a new frame
/// kind has to be answered here rather than falling through to silence.
#[derive(Debug)]
pub enum SessionStep {
    /// Answer with this welcome and move to [`SessionState::Live`].
    Welcome(Box<Welcome>),
    /// Hand this command to the sink.
    Deliver(Box<WireCommand>),
    /// Evaluate this query and answer it.
    Answer(Box<Query>),
    /// Send this refusal; `close` ends the session afterwards.
    Refuse { error: Box<AxError>, close: bool },
}

/// The whole session policy, as a pure function: which frames are legal
/// when, and what a mismatch does. Tested without a socket, for the same
/// reason [`decide_bind`] is.
///
/// A command that arrives before the hello is refused rather than queued:
/// the peer has not yet shown that it speaks this wire, and running work
/// for it would be trusting a stranger's first sentence.
#[must_use]
pub fn decide_frame(
    state: SessionState,
    frame: ClientFrame,
    configured: Option<&B3Hash>,
    city: Option<&Address>,
) -> SessionStep {
    let expected = Welcome {
        wire_v: WIRE_V,
        schema: schema_hash(),
        resume_from: None,
        city: city.cloned(),
    };
    match (state, frame) {
        (SessionState::AwaitingHello, ClientFrame::Hello(hello)) => {
            match decide_handshake(&hello, &expected, configured) {
                HandshakeVerdict::Accept => SessionStep::Welcome(Box::new(expected)),
                HandshakeVerdict::Reject(error) => SessionStep::Refuse {
                    error: Box::new(error),
                    close: true,
                },
            }
        }
        (SessionState::AwaitingHello, _) => SessionStep::Refuse {
            error: Box::new(
                AxError::failure(
                    AxCode::WireMismatch,
                    "accept a client frame",
                    "the session has not been opened with a hello",
                )
                .with_recovery("send hello first; reload the page if the client did not"),
            ),
            close: true,
        },
        (SessionState::Live, ClientFrame::Command(command)) => SessionStep::Deliver(command),
        (SessionState::Live, ClientFrame::Query(query)) => SessionStep::Answer(Box::new(query)),
        (SessionState::Live, ClientFrame::Hello(_)) => SessionStep::Refuse {
            error: Box::new(
                AxError::failure(
                    AxCode::WireMismatch,
                    "accept a client frame",
                    "this session is already open",
                )
                .with_recovery("open a second connection instead of re-greeting on this one"),
            ),
            close: false,
        },
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

    #[test]
    fn an_ipv6_loopback_is_also_loopback() {
        let addr: SocketAddr = "[::1]:8787".parse().unwrap();
        assert!(matches!(
            decide_bind(&addr, false),
            BindVerdict::Serve(BindFace::Loopback)
        ));
    }

    #[test]
    fn only_a_caller_on_this_machine_may_enrol_a_credential() {
        for local in ["127.0.0.1:51000", "[::1]:51000"] {
            let peer: SocketAddr = local.parse().unwrap();
            assert!(matches!(decide_enroll(&peer), EnrollVerdict::Accept));
        }
        let remote: SocketAddr = "203.0.113.7:51000".parse().unwrap();
        let EnrollVerdict::Refuse(err) = decide_enroll(&remote) else {
            panic!("a peer beyond this machine cannot enrol a credential");
        };
        assert_eq!(*err.code(), AxCode::GateDenied);
        // The third part points at the one place it can be done, which is
        // what keeps this a constraint rather than a dead end.
        assert!(err.recovery().contains("machine running sprawling"));
    }

    #[test]
    fn a_pairing_token_does_not_buy_the_right_to_enrol() {
        // An exposed bind is legal with a token; enrolment still is not.
        let exposed: SocketAddr = "203.0.113.7:8787".parse().unwrap();
        assert!(matches!(
            decide_bind(&exposed, true),
            BindVerdict::Serve(BindFace::Exposed)
        ));
        assert!(matches!(decide_enroll(&exposed), EnrollVerdict::Refuse(_)));
    }

    fn hello(wire_v: u32, token: Option<&str>) -> ClientFrame {
        ClientFrame::Hello(Hello {
            wire_v,
            schema: schema_hash(),
            token: token.map(str::to_owned),
        })
    }

    fn a_command() -> ClientFrame {
        ClientFrame::Command(Box::new(crate::wire::Command::Cancel {
            run: kernel::RunId::CITY,
            idem: kernel::IdemKey::derive(&kernel::RunId::CITY, kernel::Seq::FIRST, b"cancel"),
        }))
    }

    #[test]
    fn a_matching_hello_opens_the_session() {
        let step = decide_frame(SessionState::AwaitingHello, hello(WIRE_V, None), None, None);
        let SessionStep::Welcome(welcome) = step else {
            panic!("a matching hello is welcomed");
        };
        assert_eq!(welcome.wire_v, WIRE_V);
        assert_eq!(welcome.schema, schema_hash());
    }

    #[test]
    fn a_different_wire_closes_the_session_rather_than_negotiating() {
        let step = decide_frame(
            SessionState::AwaitingHello,
            hello(WIRE_V.saturating_add(1), None),
            None,
            None,
        );
        let SessionStep::Refuse { error, close } = step else {
            panic!("a wire mismatch is refused");
        };
        assert!(close, "the session ends; two wire versions are two servers");
        assert!(!error.recovery().is_empty());
    }

    #[test]
    fn a_command_before_the_hello_is_refused_rather_than_queued() {
        let step = decide_frame(SessionState::AwaitingHello, a_command(), None, None);
        let SessionStep::Refuse { close, .. } = step else {
            panic!("an unopened session runs nothing");
        };
        assert!(close);
    }

    #[test]
    fn a_live_session_delivers_commands_and_answers_queries() {
        assert!(matches!(
            decide_frame(SessionState::Live, a_command(), None, None),
            SessionStep::Deliver(_)
        ));
        assert!(matches!(
            decide_frame(
                SessionState::Live,
                ClientFrame::Query(crate::wire::Query::CityView),
                None,
                None
            ),
            SessionStep::Answer(_)
        ));
    }

    #[test]
    fn a_second_hello_is_refused_without_ending_the_session() {
        let step = decide_frame(SessionState::Live, hello(WIRE_V, None), None, None);
        let SessionStep::Refuse { close, .. } = step else {
            panic!("one session, one greeting");
        };
        assert!(!close, "a confused client is corrected, not disconnected");
    }

    #[test]
    fn an_exposed_session_needs_the_pairing_token() {
        let digest = B3Hash::digest(b"pairing-code");
        assert!(matches!(
            decide_frame(
                SessionState::AwaitingHello,
                hello(WIRE_V, None),
                Some(&digest),
                None
            ),
            SessionStep::Refuse { close: true, .. }
        ));
    }

    #[test]
    fn an_unspecified_address_is_not_loopback() {
        // 0.0.0.0 reaches every interface; treating it as local would be the
        // exact mistake this judgement exists to prevent.
        let addr: SocketAddr = "0.0.0.0:8787".parse().unwrap();
        assert!(matches!(decide_bind(&addr, false), BindVerdict::Refuse(_)));
        assert!(matches!(
            decide_bind(&addr, true),
            BindVerdict::Serve(BindFace::Exposed)
        ));
    }
}
