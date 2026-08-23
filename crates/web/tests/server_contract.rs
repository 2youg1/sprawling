// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! The client against the bytes a real server sends. No browser: the
//! frames are built with the server's own types, serialized the way the
//! socket serializes them, and read back through the link and the
//! snapshot. What this catches is the failure a headless browser would
//! also catch and nothing else does - the two ends disagreeing about the
//! wire while each remains internally consistent.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::arithmetic_side_effects,
    reason = "test code"
)]

use channels::{
    Address, Answer, BuildingProgress, CityAnswer, EventDraft, EventKind, EventRecord, Payload,
    Progress, RunId, ServerFrame, WIRE_V, Welcome, schema_hash,
};
use channels::{B3Hash, PlannedProgress, Seq, TimeMs};
use web::{Link, LinkAction, LinkEvent, Snapshot, rebuild};

/// Serializes like the server and parses like the client, so a shape that
/// only one side understands fails here.
fn round_trip(frame: &ServerFrame) -> Box<ServerFrame> {
    let text = serde_json::to_string(frame).unwrap();
    Box::new(serde_json::from_str(&text).unwrap())
}

fn record(seq: u64, kind: EventKind, run: RunId) -> EventRecord {
    EventRecord::from_draft(
        EventDraft {
            run,
            t: TimeMs::new(seq),
            who: "lab/room1".to_owned(),
            addr: None,
            kind,
            data: Payload::empty(),
            ig: false,
        },
        Seq::new(seq),
        // The client never verifies a chain; a prev hash is a value it
        // carries, not one it checks, so any digest serves here.
        B3Hash::digest(b"prev"),
    )
}

#[test]
fn a_client_walks_from_hello_to_a_live_stream_and_folds_what_arrives() {
    let mut link = Link::new(None);
    assert!(matches!(link.connect(), LinkAction::OpenSocket));

    let LinkAction::Send(hello) = link.advance(LinkEvent::Opened) else {
        panic!("an opened socket greets");
    };
    // The client greets with the wire it was built against; the server
    // compares exactly this pair before answering anything.
    assert_eq!(hello.wire_v, WIRE_V);
    assert_eq!(hello.schema, schema_hash());

    let welcome = round_trip(&ServerFrame::Welcome(Welcome {
        wire_v: WIRE_V,
        schema: schema_hash(),
        resume_from: None,
        city: channels::Address::parse("kiln").ok(),
    }));
    assert!(matches!(
        link.advance(LinkEvent::Received(welcome)),
        LinkAction::Nothing
    ));
    assert!(link.is_live());

    let run = RunId::from_bytes([3; 16]);
    let mut snapshot = Snapshot::new();
    for (seq, kind) in [
        (1, EventKind::RunStarted),
        (2, EventKind::ModelCalled),
        (3, EventKind::RunFrozen),
    ] {
        let frame = round_trip(&ServerFrame::Event(Box::new(record(seq, kind, run))));
        let LinkAction::Deliver(event) = link.advance(LinkEvent::Received(frame)) else {
            panic!("a live link delivers events");
        };
        snapshot.apply(&event);
    }

    assert_eq!(snapshot.runs().count(), 1);
    assert_eq!(snapshot.resume_from(), Some(Seq::new(3)));

    // The same events in a cold rebuild give the same snapshot: the
    // client's view is as disposable as the server's.
    let events: Vec<EventRecord> = [
        (1, EventKind::RunStarted),
        (2, EventKind::ModelCalled),
        (3, EventKind::RunFrozen),
    ]
    .into_iter()
    .map(|(seq, kind)| record(seq, kind, run))
    .collect();
    assert_eq!(rebuild(events.iter()), snapshot);
}

#[test]
fn an_answer_reaches_the_view_that_asked_for_it() {
    let mut link = Link::new(None);
    link.connect();
    link.advance(LinkEvent::Opened);
    link.advance(LinkEvent::Received(round_trip(&ServerFrame::Welcome(
        Welcome {
            wire_v: WIRE_V,
            schema: schema_hash(),
            resume_from: None,
            city: None,
        },
    ))));

    let answer = ServerFrame::Answer(Box::new(Answer::City(CityAnswer {
        runs: Vec::new(),
        active: 0,
        frozen: 1,
        buildings: vec![BuildingProgress {
            addr: Address::parse("lab").unwrap(),
            progress: Progress::Planned(PlannedProgress {
                done: 1,
                blocked: 1,
                total: 4,
            }),
            problems: Vec::new(),
        }],
    })));
    let LinkAction::Answered(answer) = link.advance(LinkEvent::Received(round_trip(&answer)))
    else {
        panic!("a live link hands answers back");
    };
    let Answer::City(city) = *answer else {
        panic!("the answer keeps its shape across the wire");
    };
    let Progress::Planned(planned) = city.buildings[0].progress else {
        panic!("a building with a roadmap arrives with its denominator");
    };
    assert_eq!((planned.done, planned.blocked, planned.total), (1, 1, 4));

    // And the bar that renders it is the one the theme knows about.
    let bar = web::bar(
        &city.buildings[0].progress,
        false,
        web::Subject::Plan,
        web::Lang::En,
    );
    assert_eq!(bar.filled, Some(web::per_mille_of(1, 4)));
    assert!(link.is_live());
}

#[test]
fn a_frame_before_the_welcome_is_treated_as_the_mismatch_it_is() {
    let mut link = Link::new(None);
    link.connect();
    link.advance(LinkEvent::Opened);
    let early = round_trip(&ServerFrame::Event(Box::new(record(
        1,
        EventKind::RunStarted,
        RunId::CITY,
    ))));
    let LinkAction::Report(err) = link.advance(LinkEvent::Received(early)) else {
        panic!("a server that streams before welcoming is not speaking this protocol");
    };
    assert!(!err.recovery().is_empty());
}
