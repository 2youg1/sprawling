// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The enrolment route answers what happened to the credential, not what
//! happened to the request.
//!
//! It used to answer 201 the moment the command reached the desk, so a
//! vault that refused told nobody and a person watched a success message
//! for a key that was never stored. Three outcomes now, each with its own
//! status: stored, refused, and neither within a bounded wait.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

use std::net::SocketAddr;
use std::sync::Arc;

use channels::{Answer, AxCode, AxError, Command, Reply, ServeConfig};
use kernel::{EventDraft, EventKind, EventRecord, GENESIS_PREV, Payload, RunId, Seq, TimeMs};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

/// What the worker does with the credential this test hands it.
enum Worker {
    /// Writes `secret_captured` for the reference, as the real one does.
    Stores,
    /// Refuses through the reply address the route gave it.
    Refuses,
    /// Takes it and says nothing, which is what a worker inside a long
    /// dispatch looks like from here.
    Silent,
}

fn captured(reference: &str) -> EventRecord {
    let mut map = serde_json::Map::new();
    map.insert(
        "ref".to_owned(),
        serde_json::Value::String(reference.to_owned()),
    );
    EventRecord::from_draft(
        EventDraft {
            run: RunId::CITY,
            t: TimeMs::new(1),
            who: "owner".to_owned(),
            addr: None,
            kind: EventKind::SecretCaptured,
            data: Payload::new(map).unwrap(),
            ig: false,
        },
        Seq::FIRST,
        GENESIS_PREV,
    )
}

async fn ask(worker: Worker, body: &str) -> (u16, String) {
    let (events, _held) = tokio::sync::broadcast::channel(16);
    let answering = events.clone();
    let config = ServeConfig {
        deltas: tokio::sync::broadcast::channel(16).0,
        addr: "127.0.0.1:0".parse().unwrap(),
        token_digest: None,
        client: Arc::new(channels::ClientAssets::Embedded(&[])),
        commands: Arc::new(|_, _| Ok(())),
        events: events.clone(),
        queries: Arc::new(|_| {
            Ok(Answer::Unavailable {
                query: "none".to_owned(),
            })
        }),
        secrets: Arc::new(
            move |command: Command<kernel::Sealed<String>>, reply: Reply| {
                let Command::PutSecret { realm, name, .. } = command else {
                    return Ok(());
                };
                let reference = format!("secret:{realm}/{name}");
                match worker_of(&worker) {
                    Worker::Stores => {
                        let _ = answering.send(captured(&reference));
                    }
                    Worker::Refuses => {
                        let _ = reply.refuse(
                            AxError::failure(
                                AxCode::CredentialMissing,
                                "store a credential",
                                reference,
                            )
                            .with_recovery("this machine has no credential service"),
                        );
                    }
                    Worker::Silent => {}
                }
                Ok(())
            },
        ),
        acp: Arc::new(|_, _| {
            Ok(channels::AcpProgress {
                run: String::new(),
                turns: 0,
                finished: true,
            })
        }),
        upload_sink: Arc::new(|_| {
            Err(AxError::failure(
                AxCode::InvalidArgs,
                "stage an attachment",
                "not in this test",
            ))
        }),
        city: None,
    };
    let app = channels::router(&config);
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let at = listener.local_addr().unwrap();
    tokio::spawn(async move {
        let _ = axum::serve(
            listener,
            app.into_make_service_with_connect_info::<SocketAddr>(),
        )
        .await;
    });

    let mut stream = tokio::net::TcpStream::connect(at).await.unwrap();
    let request = format!(
        "POST /enroll HTTP/1.1\r\nHost: {at}\r\nContent-Type: application/json\r\n\
         Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
        body.len()
    );
    stream.write_all(request.as_bytes()).await.unwrap();
    let mut said = String::new();
    stream.read_to_string(&mut said).await.unwrap();
    let status = said
        .split_whitespace()
        .nth(1)
        .and_then(|code| code.parse().ok())
        .unwrap_or(0);
    (status, said)
}

/// `Worker` is moved into a `Fn` closure, which may run more than once;
/// this reads it without taking it.
fn worker_of(worker: &Worker) -> Worker {
    match worker {
        Worker::Stores => Worker::Stores,
        Worker::Refuses => Worker::Refuses,
        Worker::Silent => Worker::Silent,
    }
}

const BODY: &str = r#"{"realm":"house","name":"key","value":"sk-not-a-real-key"}"#;

#[tokio::test]
async fn a_stored_credential_answers_with_the_reference_that_replaced_it() {
    let (status, said) = ask(Worker::Stores, BODY).await;
    assert_eq!(status, 201, "{said}");
    assert!(said.contains("secret:house/key"), "{said}");
    assert!(
        !said.contains("sk-not-a-real-key"),
        "the value must never come back out"
    );
}

/// The case the route could not express at all: the worker refused and
/// the person was told it had worked.
#[tokio::test]
async fn a_refusal_reaches_the_request_that_carried_the_credential() {
    let (status, said) = ask(Worker::Refuses, BODY).await;
    assert_eq!(status, 422, "{said}");
    assert!(said.contains("credential service"), "{said}");
}

#[tokio::test]
async fn neither_answer_within_the_wait_is_its_own_answer() {
    let (status, said) = ask(Worker::Silent, BODY).await;
    assert_eq!(status, 202, "{said}");
    assert!(
        said.contains("has not answered"),
        "202 has to say why it is 202: {said}"
    );
}
