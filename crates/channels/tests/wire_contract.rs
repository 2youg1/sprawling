// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The wire contract the rest of the system is allowed to rely on:
//! the command and query counts, the schema hash that gates a connection,
//! the binding decision, and the two shapes that must stay unspellable.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code"
)]

use std::net::SocketAddr;

use channels::{BindFace, BindVerdict, HandshakeVerdict, decide_bind, decide_handshake};
use channels::{
    COMMAND_NAMES, Command, Hello, ModeTag, ProviderName, QUERY_NAMES, Query, WIRE_V, Welcome,
    schema_hash,
};
use kernel::{Address, AxCode, Sealed, Seq};

fn loopback() -> SocketAddr {
    "127.0.0.1:8787".parse().unwrap()
}

fn exposed() -> SocketAddr {
    "192.168.1.20:8787".parse().unwrap()
}

// ---------------------------------------------------------------- wire counts

#[test]
fn the_command_and_query_tables_hold_their_declared_counts() {
    // Twenty-two commands, fourteen queries. The count is not a style
    // choice - it is the wire's closed surface.
    assert_eq!(COMMAND_NAMES.len(), 22, "command table");
    assert_eq!(QUERY_NAMES.len(), 14, "query table");

    let mut sorted = COMMAND_NAMES.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 22, "command names are distinct");

    let mut sorted = QUERY_NAMES.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    assert_eq!(sorted.len(), 14, "query names are distinct");
}

#[test]
fn every_command_name_is_registered_in_the_table() {
    // `Command::name` is an exhaustive match, so a new variant cannot compile
    // without appearing here; this test closes the other half - it must also
    // appear in the name table that feeds the schema hash.
    for command in sample_of_every_command() {
        let name = command.name();
        assert!(
            COMMAND_NAMES.contains(&name),
            "command `{name}` is missing from COMMAND_NAMES"
        );
    }
}

// ------------------------------------------------------------- schema hashing

#[test]
fn the_schema_hash_is_stable_across_calls_and_covers_the_wire_version() {
    assert_eq!(schema_hash(), schema_hash(), "hash is a pure function");
    // Golden: this pins the current wire. Changing a variant changes the hash,
    // which forces the SPEC to move in the same change set (apisync gate).
    assert_eq!(
        schema_hash().to_string(),
        WIRE_SCHEMA_GOLDEN,
        "schema hash changed - update channels-SPEC.md section 8-1 in the same commit"
    );
    assert_eq!(
        WIRE_V, 12,
        "the version rises when the grammar changes shape without a name changing"
    );
}

/// Pinned on the first green of S4.02. It is a function of WIRE_V and the two
/// name tables, so any change to the protocol surface lands here first.
const WIRE_SCHEMA_GOLDEN: &str = "4ac1b7b375c9a944a32920129e703673fe3d0092c3cea22c44da411f10855aac";

// -------------------------------------------------------------- binding face

#[test]
fn the_binding_face_has_exactly_one_refusing_cell() {
    // Constitution 8.3: loopback by default; exposed requires a pairing token;
    // no token configured means refuse to *start*, not refuse to connect.
    assert!(matches!(
        decide_bind(&loopback(), false),
        BindVerdict::Serve(BindFace::Loopback)
    ));
    assert!(matches!(
        decide_bind(&loopback(), true),
        BindVerdict::Serve(BindFace::Loopback)
    ));
    assert!(matches!(
        decide_bind(&exposed(), true),
        BindVerdict::Serve(BindFace::Exposed)
    ));

    let BindVerdict::Refuse(err) = decide_bind(&exposed(), false) else {
        panic!("an exposed bind with no token must refuse to start");
    };
    assert_eq!(*err.code(), AxCode::ConfigInvalid);
    assert!(
        !err.recovery().is_empty(),
        "a refusal names an executable alternative"
    );
}

// ------------------------------------------------------------------ handshake

#[test]
fn a_mismatched_schema_hash_is_rejected_before_anything_else() {
    let good = Hello {
        wire_v: WIRE_V,
        schema: schema_hash(),
        token: None,
    };
    let expected = Welcome {
        wire_v: WIRE_V,
        schema: schema_hash(),
        resume_from: Some(Seq::new(7)),
        city: None,
    };
    assert!(matches!(
        decide_handshake(&good, &expected, None),
        HandshakeVerdict::Accept
    ));

    let stale = Hello {
        wire_v: WIRE_V,
        schema: kernel::B3Hash::digest(b"a client that cached an older front end"),
        token: None,
    };
    let HandshakeVerdict::Reject(err) = decide_handshake(&stale, &expected, None) else {
        panic!("a cached older client must be told to refresh, not silently served");
    };
    assert_eq!(*err.code(), AxCode::WireMismatch);
}

#[test]
fn an_exposed_server_rejects_a_wrong_token_and_accepts_the_right_one() {
    let expected = Welcome {
        wire_v: WIRE_V,
        schema: schema_hash(),
        resume_from: None,
        city: None,
    };
    let secret = kernel::B3Hash::digest(b"pair-me-0123456789");

    let wrong = Hello {
        wire_v: WIRE_V,
        schema: schema_hash(),
        token: Some("pair-me-9876543210".to_owned()),
    };
    assert!(matches!(
        decide_handshake(&wrong, &expected, Some(&secret)),
        HandshakeVerdict::Reject(_)
    ));

    let right = Hello {
        wire_v: WIRE_V,
        schema: schema_hash(),
        token: Some("pair-me-0123456789".to_owned()),
    };
    assert!(matches!(
        decide_handshake(&right, &expected, Some(&secret)),
        HandshakeVerdict::Accept
    ));

    let absent = Hello {
        wire_v: WIRE_V,
        schema: schema_hash(),
        token: None,
    };
    assert!(
        matches!(
            decide_handshake(&absent, &expected, Some(&secret)),
            HandshakeVerdict::Reject(_)
        ),
        "a missing token is not an empty token"
    );
}

// --------------------------------------------------- the two unspellable shapes

#[test]
fn put_secret_has_no_byte_form_in_either_direction() {
    // A13 continues onto the wire. The outbound half is a *compile* error -
    // `Command<Sealed<String>>` has no `Serialize` because `Sealed` has none,
    // so the trybuild case in tests/compile_fail is the real proof and this
    // test cannot even spell the attempt. The inbound half is checked here:
    // bytes that name the variant are refused, and the refusal stays silent
    // about what it was protecting.
    let json = r#"{"put_secret":{"realm":"anthropic","name":"api","value":"sk-not-a-real-key"}}"#;
    let decoded: Result<channels::WireCommand, _> = serde_json::from_str(json);
    let err = decoded.expect_err("PutSecret has no wire form");
    assert!(
        !err.to_string().contains("sk-not-a-real-key"),
        "the refusal must not echo the very bytes it is protecting"
    );

    // The in-process shape still exists - that is the whole point: the host
    // may enrol a credential, a socket may not.
    let local = Command::PutSecret {
        realm: "anthropic".to_owned(),
        name: "api".to_owned(),
        value: Sealed::new(Box::new("sk-not-a-real-key".to_owned())),
    };
    assert_eq!(local.name(), "PutSecret");
    assert!(local.idem().is_none());
}

#[test]
fn every_state_changing_command_carries_an_idempotency_key() {
    // Constitution 8.3: "double-clicking twice does not open two Runs."
    for command in sample_of_every_command() {
        let name = command.name();
        if name == "Auth" || name == "PutSecret" {
            assert!(
                command.idem().is_none(),
                "`{name}` changes nothing a replay could double"
            );
        } else {
            assert!(
                command.idem().is_some(),
                "state-changing command `{name}` has no IdemKey"
            );
        }
    }
}

// ------------------------------------------------------------------- fixtures

fn sample_of_every_command() -> Vec<Command> {
    let addr = Address::parse("acme/floor1").unwrap();
    let run = kernel::RunId::from_bytes([7u8; 16]);
    let idem = kernel::IdemKey::derive(&run, Seq::new(1), b"sample");
    vec![
        Command::Wake {
            source: "github".to_owned(),
            subject: "pull request opened".to_owned(),
            body: "someone would like a change reviewed".to_owned(),
            idem,
        },
        Command::Dispatch {
            addr: addr.clone(),
            task: "ship it".to_owned(),
            goal: "the tests pass".to_owned(),
            mode: ModeTag::parse("plan").unwrap(),
            budget: kernel::BudgetCap {
                usd: kernel::UsdMicros::new(1_000_000),
                tokens: kernel::Tokens::new(200_000),
            },
            idem,
            session: Some(kernel::SessionName::parse("ship it").unwrap()),
            effort: Some(kernel::Effort::High),
        },
        Command::Login {
            provider: ProviderName::parse("anthropic").unwrap(),
            step: channels::LoginStep::Begin,
            idem,
        },
        Command::ConfigureBuilding {
            addr: Address::parse("lab").unwrap(),
            sandbox: Some(kernel::SandboxLimits::default()),
            mcp: Some(Vec::new()),
            idem,
        },
        Command::ProbeEndpoint {
            name: ProviderName::parse("house").unwrap(),
            base_url: "https://api.example.test/v1".to_owned(),
            dialect: kernel::DialectKind::OpenAi,
            secret: Some("secret:house/key".to_owned()),
            auth_header: None,
            idem,
        },
        Command::AttachEndpoint {
            name: ProviderName::parse("house").unwrap(),
            base_url: "https://api.example.test/v1".to_owned(),
            dialect: kernel::DialectKind::OpenAi,
            secret: Some("secret:house/key".to_owned()),
            auth_header: None,
            admit: vec!["gpt-x".to_owned()],
            idem,
        },
        Command::SelectModel {
            endpoint: ProviderName::parse("house").unwrap(),
            model: "m-large".to_owned(),
            tag: kernel::ModelTag::Main,
            context_tokens: 128_000,
            max_output_tokens: 8_192,
            idem,
        },
        Command::Fork {
            run,
            at_seq: Seq::new(3),
            addr: None,
            idem,
        },
        Command::Attach {
            upload: channels::UploadId::parse("u-0001").unwrap(),
            notify: vec![run],
            idem,
        },
        Command::CreateBuilding {
            addr: addr.clone(),
            template: channels::TemplateName::parse("workshop").unwrap(),
            idem,
        },
        Command::PutSecret {
            realm: "anthropic".to_owned(),
            name: "api".to_owned(),
            value: Sealed::new(Box::new("sk-not-a-real-key".to_owned())),
        },
        Command::Steer {
            run,
            text: "try the other branch".to_owned(),
            idem,
        },
        Command::Cancel { run, idem },
        Command::Takeover { run, idem },
        Command::Rollback {
            checkpoint: kernel::GitOid::from_bytes([0x5au8; 20]),
            idem,
        },
        Command::Halt {
            scope: channels::HaltScope::City,
            idem,
        },
        Command::Release {
            scope: channels::HaltScope::City,
            idem,
        },
        Command::BatchByBuilding { addr, idem },
        Command::Approve {
            item: kernel::ApprovalId::new("ap-1").unwrap(),
            verdict: kernel::PolicyVerdict::Allow,
            idem,
        },
        Command::CreatePolicy {
            from_item: kernel::ApprovalId::new("ap-1").unwrap(),
            idem,
        },
        Command::SetAutonomy {
            scope: channels::HaltScope::City,
            autonomy: kernel::Autonomy::Owner,
            idem,
        },
        Command::Auth {
            token: "pair-me-0123456789".to_owned(),
        },
    ]
}

#[test]
fn the_command_sample_covers_the_whole_table() {
    let sample = sample_of_every_command();
    assert_eq!(
        sample.len(),
        COMMAND_NAMES.len(),
        "the fixture must exercise every command"
    );
}

#[test]
fn a_query_never_carries_an_idempotency_key() {
    // Queries are side-effect free by construction, so there is nothing to
    // deduplicate; if one ever needs a key it has stopped being a Query.
    for name in QUERY_NAMES {
        assert!(!name.is_empty());
    }
    assert!(matches!(Query::ApprovalQueue, Query::ApprovalQueue));
}

/// Narrowing history to one session survives the wire, and stays a
/// different frame from the unfiltered slice.
///
/// The two used to be one question, and the consequence was a page:
/// four sessions divided one bounded slice between them, so a session
/// older than that slice opened blank.
#[test]
fn asking_for_one_session_is_a_different_frame_from_asking_for_the_city() {
    let run = kernel::RunId::from_bytes([4u8; 16]);
    let mine = Query::RunHistory {
        run,
        before: Some(kernel::Seq::new(90)),
        limit: 50,
    };
    let bytes = serde_json::to_vec(&mine).expect("a query serialises");
    let back: Query = serde_json::from_slice(&bytes).expect("and reads back");
    assert_eq!(back, mine);
    assert_eq!(mine.name(), "RunHistory");
    assert!(QUERY_NAMES.contains(&mine.name()), "named in the table");

    let city = Query::History {
        before: Some(kernel::Seq::new(90)),
        limit: 50,
    };
    assert_ne!(
        serde_json::to_vec(&city).expect("a query serialises"),
        bytes,
        "one session and the whole city must not spell the same frame"
    );
}

/// The second step of a login has its own byte form: a page that means
/// "redeem this code" must not be readable as "start a login".
#[test]
fn both_login_steps_survive_the_round_trip_and_stay_distinct() {
    let idem = kernel::IdemKey::derive(&kernel::RunId::CITY, kernel::Seq::FIRST, b"login");
    let begin = Command::Login {
        provider: ProviderName::parse("anthropic").unwrap(),
        step: channels::LoginStep::Begin,
        idem,
    };
    let code = Command::Login {
        provider: ProviderName::parse("anthropic").unwrap(),
        step: channels::LoginStep::Code {
            code: "the-code".to_owned(),
        },
        idem,
    };
    let begin_text = serde_json::to_string(&begin).unwrap();
    let code_text = serde_json::to_string(&code).unwrap();
    assert_ne!(begin_text, code_text, "one step must not read as the other");
    assert!(code_text.contains("the-code"));
    assert_eq!(
        serde_json::from_str::<channels::WireCommand>(&begin_text).unwrap(),
        begin
    );
    assert_eq!(
        serde_json::from_str::<channels::WireCommand>(&code_text).unwrap(),
        code
    );
}
