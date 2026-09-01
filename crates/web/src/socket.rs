// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The only place in this crate that talks to the server.
//!
//! Everything about *when* to connect, what to send first, and what to do
//! after a drop is a pure state machine here; the browser call that actually
//! opens a socket is a few lines behind a target gate. That split is what
//! lets the reconnect behaviour be tested by a native test with no browser
//! and no clock.
//!
//! Two rules the machine exists to hold:
//!
//! - **A schema mismatch is terminal.** Retrying against a server we cannot
//!   speak to would spin forever and show a spinner instead of the one
//!   sentence that fixes it: reload the page. `Refused` is a resting state.
//! - **Backoff is deterministic, never jittered.** Same reason
//!   `gateway::admission` gives: random jitter is probabilistic, so several
//!   clients waking together can all draw a low value and stampede anyway.
//!   A fixed ladder caps the rate by construction.

use channels::{
    Address, AxCode, AxError, EventRecord, Hello, Seq, ServerFrame, Welcome, schema_hash,
};

/// The backoff ladder in milliseconds. Ends flat rather than growing without
/// bound: a person who left the laptop closed should find the interface live
/// within a minute of opening it, not on the far side of an hour-long wait.
const BACKOFF_LADDER_MS: [u64; 6] = [250, 500, 1_000, 2_000, 5_000, 10_000];

/// Where the link is.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkState {
    /// Nothing attempted yet.
    Idle,
    /// A socket is opening.
    Opening,
    /// Open, `Hello` sent, waiting for the server's answer.
    Handshaking,
    /// Serving frames.
    Live { resume_from: Option<Seq> },
    /// Waiting out a failed attempt. The attempt *number* is not here: it
    /// survives the trip through `Opening` and so belongs to the link, not
    /// to a momentary phase (see [`Link::consecutive_failures`]).
    Backoff,
    /// Stopped on purpose. Not a failure to retry - a fact to report.
    Refused(Box<AxError>),
    /// Nobody is looking at this tab. Not a failure and not a refusal:
    /// the link is closed because there is no reader, and it comes back
    /// when there is one. Carrying `resume_from` across is what makes
    /// coming back cheap - the server replays a tail rather than a
    /// history.
    Suspended { resume_from: Option<Seq> },
}

/// What happened to the link.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkEvent {
    Opened,
    Received(Box<ServerFrame>),
    Closed,
    /// The transport failed to open or died mid-flight.
    TransportFailed,
    /// The backoff wait elapsed.
    WaitElapsed,
    /// The tab went out of view.
    Backgrounded,
    /// The tab came back.
    Foregrounded,
}

/// What the shell should do next. Exhaustive, so the shell cannot invent an
/// action the machine never authorised.
#[derive(Debug, Clone, PartialEq)]
pub enum LinkAction {
    Nothing,
    OpenSocket,
    Send(Box<Hello>),
    /// Hand an event to `app::Snapshot::apply`.
    Deliver(Box<EventRecord>),
    /// Hand a query answer to whichever view asked for it.
    Answered(Box<channels::Answer>),
    /// Hand text a model is still saying to the page showing that run.
    ///
    /// Never `Deliver`: an increment has no sequence number and is never
    /// written down, so folding it into the snapshot would put something
    /// on screen that no replay could produce.
    Saying(channels::Delta),
    /// Sleep this long, then feed back `WaitElapsed`.
    WaitMs(u64),
    /// Show this and stop. The interface renders the three-part refusal.
    Report(Box<AxError>),
    /// Close the socket and do nothing further until asked. Distinct
    /// from `WaitMs`: a slowed tab still holds a connection, still wakes
    /// the machine, and still spends something nobody agreed to spend.
    CloseSocket,
}

/// One connection, as a value.
#[derive(Debug, Clone, PartialEq)]
pub struct Link {
    state: LinkState,
    token: Option<String>,
    /// Failures since the last time frames flowed. Held on the link rather
    /// than inside `LinkState::Backoff`, because every retry passes through
    /// `Opening` on its way back to `Backoff` - a counter living in the
    /// phase is reset by its own retry loop and the ladder never climbs.
    consecutive_failures: u32,
    /// The city named in the welcome, once one has arrived.
    city: Option<Address>,
}

impl Link {
    /// A link that has not tried yet. `token` is present only when the
    /// operator paired this browser with a non-loopback host.
    #[must_use]
    pub fn new(token: Option<String>) -> Self {
        Self {
            state: LinkState::Idle,
            token,
            consecutive_failures: 0,
            city: None,
        }
    }

    /// Failures since frames last flowed. Zero whenever the link is live.
    #[must_use]
    pub fn consecutive_failures(&self) -> u32 {
        self.consecutive_failures
    }

    #[must_use]
    pub fn state(&self) -> &LinkState {
        &self.state
    }

    /// The city this link reached, once it has been welcomed.
    #[must_use]
    pub fn city(&self) -> Option<&Address> {
        self.city.as_ref()
    }

    /// Whether frames are flowing. The interface dims itself when not.
    #[must_use]
    pub fn is_live(&self) -> bool {
        matches!(self.state, LinkState::Live { .. })
    }

    /// Starts, or restarts after a `Refused` was cleared by the operator.
    pub fn connect(&mut self) -> LinkAction {
        self.state = LinkState::Opening;
        LinkAction::OpenSocket
    }

    /// Advances the machine.
    ///
    /// `resume_from` is carried across a reconnect so the server can replay
    /// the tail rather than the whole history; `app::Snapshot` drops any
    /// overlap for free, so the cut does not need to be exact.
    pub fn advance(&mut self, event: LinkEvent) -> LinkAction {
        match (&self.state, event) {
            (LinkState::Refused(_), _) => LinkAction::Nothing,

            (LinkState::Opening, LinkEvent::Opened) => {
                self.state = LinkState::Handshaking;
                LinkAction::Send(Box::new(Hello {
                    wire_v: channels::WIRE_V,
                    schema: schema_hash(),
                    token: self.token.clone(),
                }))
            }

            (LinkState::Handshaking, LinkEvent::Received(frame)) => match *frame {
                ServerFrame::Welcome(welcome) => self.welcomed(&welcome),
                ServerFrame::Refusal(err) => self.refuse(*err),
                // A server that streams or answers before welcoming is not
                // speaking this protocol; treat it as the mismatch it is.
                ServerFrame::Event(_) | ServerFrame::Answer(_) | ServerFrame::Delta(_) => {
                    self.refuse(out_of_order())
                }
            },

            (LinkState::Live { resume_from }, LinkEvent::Received(frame)) => {
                let resume_from = *resume_from;
                match *frame {
                    ServerFrame::Event(event) => {
                        self.state = LinkState::Live {
                            resume_from: Some(event.seq()),
                        };
                        LinkAction::Deliver(event)
                    }
                    ServerFrame::Answer(answer) => {
                        self.state = LinkState::Live { resume_from };
                        LinkAction::Answered(answer)
                    }
                    // Text arriving mid-call. It does not advance
                    // `resume_from`: nothing here is recoverable from the
                    // ledger, because nothing here is in it.
                    ServerFrame::Delta(delta) => LinkAction::Saying(delta),
                    ServerFrame::Refusal(err) => LinkAction::Report(err),
                    ServerFrame::Welcome(_) => {
                        self.state = LinkState::Live { resume_from };
                        LinkAction::Nothing
                    }
                }
            }

            (LinkState::Backoff, LinkEvent::WaitElapsed) => {
                self.state = LinkState::Opening;
                LinkAction::OpenSocket
            }

            // Going out of view stops the link from wherever it was. A
            // refusal is already caught by the first arm above: it is a
            // fact to report, and it does not become less true because
            // somebody switched tabs.
            (state, LinkEvent::Backgrounded) => {
                let resume_from = match state {
                    LinkState::Live { resume_from } => *resume_from,
                    _ => None,
                };
                self.state = LinkState::Suspended { resume_from };
                LinkAction::CloseSocket
            }
            (LinkState::Suspended { .. }, LinkEvent::Foregrounded) => {
                // Back from the bottom of the ladder: time away is not
                // evidence that the server is unwell.
                self.consecutive_failures = 0;
                self.state = LinkState::Opening;
                LinkAction::OpenSocket
            }
            // Nothing reaches a suspended link. A socket that was closing
            // while the tab went away will report it, and that report
            // must not restart anything.
            (LinkState::Suspended { .. }, _) => LinkAction::Nothing,

            (_, LinkEvent::Closed | LinkEvent::TransportFailed) => self.retreat(),

            // Anything else is a frame arriving in a state that did not ask
            // for one: ignore rather than crash, because the link's job is
            // to keep the interface honest, not to police the server.
            _ => LinkAction::Nothing,
        }
    }

    fn welcomed(&mut self, welcome: &Welcome) -> LinkAction {
        if welcome.wire_v != channels::WIRE_V || welcome.schema != schema_hash() {
            return self.refuse(mismatch(welcome));
        }
        // Which city answered. Kept here because the handshake is where it
        // was said, and the interface reads it from the link rather than
        // waiting for an event that will never come again.
        self.city.clone_from(&welcome.city);
        self.state = LinkState::Live {
            resume_from: welcome.resume_from,
        };
        // Frames flow again: the next outage starts at the bottom of the
        // ladder rather than inheriting an old grudge.
        self.consecutive_failures = 0;
        LinkAction::Nothing
    }

    fn refuse(&mut self, err: AxError) -> LinkAction {
        let boxed = Box::new(err);
        self.state = LinkState::Refused(boxed.clone());
        LinkAction::Report(boxed)
    }

    fn retreat(&mut self) -> LinkAction {
        let attempt = self.consecutive_failures;
        self.consecutive_failures = attempt.saturating_add(1);
        self.state = LinkState::Backoff;
        LinkAction::WaitMs(backoff_ms(attempt))
    }
}

/// The wait for one attempt number, clamped to the end of the ladder.
#[must_use]
pub fn backoff_ms(attempt: u32) -> u64 {
    let last = BACKOFF_LADDER_MS.len().saturating_sub(1);
    let index = usize::try_from(attempt).unwrap_or(last).min(last);
    BACKOFF_LADDER_MS.get(index).copied().unwrap_or(10_000)
}

fn mismatch(welcome: &Welcome) -> AxError {
    AxError::failure(
        AxCode::WireMismatch,
        "join this city's control surface",
        format!(
            "this page speaks wire v{} and the server speaks v{}",
            channels::WIRE_V,
            welcome.wire_v
        ),
    )
    .with_recovery("reload the page to fetch the client this server was built with")
}

fn out_of_order() -> AxError {
    AxError::failure(
        AxCode::WireMismatch,
        "join this city's control surface",
        "the server streamed events before completing the handshake",
    )
    .with_recovery("reload the page; if it repeats, the address is not a sprawling server")
}

/// Parses one text frame the way the client must read it. A frame this
/// build cannot parse is read as a closed link rather than skipped: the
/// two ends disagree about the wire, and the machine already knows what
/// to do about that.
#[must_use]
pub fn read_frame(text: &str) -> LinkEvent {
    match serde_json::from_str::<ServerFrame>(text) {
        Ok(frame) => LinkEvent::Received(Box::new(frame)),
        Err(_) => LinkEvent::Closed,
    }
}

/// The pairing code the host put on the URL that opened this page.
///
/// Pure over the query string, so the judgement is testable off the
/// browser and the browser half below holds none of it. The value is
/// taken as written: the codes this city mints come from an alphabet of
/// digits and lower-case letters, which needs no unescaping, and a
/// configured token carrying reserved characters fails visibly at the
/// handshake with "the pairing token does not match" rather than
/// silently connecting as somebody else.
///
/// An empty value is not a value. A peer that sends `token=` would be
/// refused anyway, and answering `None` keeps the refusal at the one
/// place that decides it.
#[must_use]
pub fn token_in(search: &str) -> Option<String> {
    search
        .trim_start_matches('?')
        .split('&')
        .filter_map(|pair| pair.split_once('='))
        .find(|(name, _)| *name == "token")
        .map(|(_, value)| value.to_owned())
        .filter(|value| !value.is_empty())
}

/// The pairing code this page was opened with, read from where the host
/// put it.
///
/// One call and no judgement: everything that could be decided is in
/// [`token_in`]. Browser-only for the same reason [`socket_url`] below
/// is - off the browser there is no location to read, and its one caller
/// is unreachable without one.
#[cfg(target_arch = "wasm32")]
#[must_use]
pub fn pairing_token() -> Option<String> {
    token_in(&web_sys::window()?.location().search().ok()?)
}

/// The address of this city's socket, derived from the page's own origin.
/// A client that is served by the city it talks to needs no configured
/// endpoint, and cannot be pointed at a second one by accident.
#[cfg(target_arch = "wasm32")]
#[must_use]
pub fn socket_url() -> Option<String> {
    let location = web_sys::window()?.location();
    let host = location.host().ok()?;
    let scheme = match location.protocol().ok()?.as_str() {
        "https:" => "wss",
        _ => "ws",
    };
    Some(format!("{scheme}://{host}/ws"))
}

/// What the enrolment route answered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Enrolment {
    /// The reference to put in the attach form. The credential itself
    /// is now in the vault and will not be seen again.
    Stored { reference: String },
    /// The server refused, in its own words. Shown rather than
    /// paraphrased: a refusal from a tunnelled session names the one
    /// machine that can do this instead.
    Refused { reason: String },
}

/// Sends one credential to this city's enrolment route.
///
/// It goes over HTTP rather than the socket because the socket's frame
/// type cannot spell a credential. Both halves are needed: the frame is
/// unspellable, and this route only answers a caller on the machine
/// running the city.
///
/// The value is never held by this module beyond the send, and never
/// enters a frame, a snapshot, or a log line.
#[cfg(target_arch = "wasm32")]
pub fn enrol(realm: &str, name: &str, value: &str, on_done: impl FnOnce(Enrolment) + 'static) {
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    let Some(url) = enrol_url() else {
        on_done(Enrolment::Refused {
            reason: "this page has no origin to enrol against".to_owned(),
        });
        return;
    };
    let Ok(request) = web_sys::XmlHttpRequest::new() else {
        on_done(Enrolment::Refused {
            reason: "this browser refused to make the request".to_owned(),
        });
        return;
    };
    if request.open_with_async("POST", &url, true).is_err() {
        on_done(Enrolment::Refused {
            reason: format!("{url} could not be opened"),
        });
        return;
    }
    let body = serde_json::json!({ "realm": realm, "name": name, "value": value }).to_string();
    let reference = format!("secret:{realm}/{name}");
    let handle = request.clone();
    let mut finish = Some(on_done);
    let settled = Closure::<dyn FnMut()>::new(move || {
        if handle.ready_state() != web_sys::XmlHttpRequest::DONE {
            return;
        }
        let Some(finish) = finish.take() else {
            return;
        };
        let answer = match handle.status() {
            Ok(201) => Enrolment::Stored {
                reference: reference.clone(),
            },
            _ => Enrolment::Refused {
                reason: handle
                    .response_text()
                    .ok()
                    .flatten()
                    .unwrap_or_else(|| "the city did not say why".to_owned()),
            },
        };
        finish(answer);
    });
    request.set_onreadystatechange(Some(settled.as_ref().unchecked_ref()));
    settled.forget();
    if request.send_with_opt_str(Some(&body)).is_err() {
        // The callback above will not fire, so the caller hears it here.
        request.set_onreadystatechange(None);
    }
}

/// The enrolment route on this page's own origin.
#[cfg(target_arch = "wasm32")]
fn enrol_url() -> Option<String> {
    let location = web_sys::window()?.location();
    Some(format!(
        "{}//{}/enroll",
        location.protocol().ok()?,
        location.host().ok()?
    ))
}

/// Off the browser there is nowhere to send it, and saying so is the
/// honest answer: the page shows this refusal rather than appearing to
/// have stored something.
#[cfg(not(target_arch = "wasm32"))]
pub fn enrol(_realm: &str, _name: &str, _value: &str, on_done: impl FnOnce(Enrolment) + 'static) {
    on_done(Enrolment::Refused {
        reason: "this client is not running in a browser".to_owned(),
    });
}

/// The browser half: one socket, three listeners, and no decisions.
///
/// Every judgement belongs to [`Link`]. This opens the connection the
/// machine asked for and turns browser callbacks into [`LinkEvent`]s.
/// Reconnect timing stays with the caller, because a timer belongs to
/// whoever owns the frame loop rather than to a transport.
///
/// # Errors
/// Refuses an address the browser will not open, naming it.
#[cfg(target_arch = "wasm32")]
pub fn open(
    url: &str,
    on_event: impl FnMut(LinkEvent) + 'static,
) -> Result<web_sys::WebSocket, AxError> {
    use std::cell::RefCell;
    use std::rc::Rc;
    use wasm_bindgen::JsCast;
    use wasm_bindgen::closure::Closure;

    let socket = web_sys::WebSocket::new(url).map_err(|_| {
        AxError::failure(
            AxCode::WireMismatch,
            "open the control surface socket",
            url.to_owned(),
        )
        .with_recovery("reload the page; this client is served by the city it talks to")
    })?;

    type Sink = Rc<RefCell<Box<dyn FnMut(LinkEvent)>>>;
    let sink: Sink = Rc::new(RefCell::new(Box::new(on_event)));
    let deliver = move |sink: &Sink, event: LinkEvent| {
        // A listener that fires while the machine is mid-step drops its
        // event rather than reentering it: the socket will report the
        // same condition again, and a half-applied transition would not.
        if let Ok(mut hold) = sink.try_borrow_mut() {
            hold(event);
        }
    };

    let opened = Closure::<dyn FnMut()>::new({
        let sink = Rc::clone(&sink);
        move || deliver(&sink, LinkEvent::Opened)
    });
    socket.set_onopen(Some(opened.as_ref().unchecked_ref()));
    opened.forget();

    let message = Closure::<dyn FnMut(web_sys::MessageEvent)>::new({
        let sink = Rc::clone(&sink);
        move |event: web_sys::MessageEvent| {
            if let Some(text) = event.data().as_string() {
                deliver(&sink, read_frame(&text));
            }
        }
    });
    socket.set_onmessage(Some(message.as_ref().unchecked_ref()));
    message.forget();

    let closed = Closure::<dyn FnMut(web_sys::CloseEvent)>::new({
        let sink = Rc::clone(&sink);
        move |_event: web_sys::CloseEvent| deliver(&sink, LinkEvent::Closed)
    });
    socket.set_onclose(Some(closed.as_ref().unchecked_ref()));
    closed.forget();

    let errored = Closure::<dyn FnMut(web_sys::Event)>::new({
        let sink = Rc::clone(&sink);
        move |_event: web_sys::Event| deliver(&sink, LinkEvent::Closed)
    });
    socket.set_onerror(Some(errored.as_ref().unchecked_ref()));
    errored.forget();

    Ok(socket)
}

/// Sends one client frame. Serialization failure is impossible for the
/// frames this crate builds, so it reports the send failure only.
///
/// # Errors
/// Names the frame the socket refused to carry.
#[cfg(target_arch = "wasm32")]
pub fn send(socket: &web_sys::WebSocket, frame: &channels::ClientFrame) -> Result<(), AxError> {
    let text = serde_json::to_string(frame).map_err(|err| {
        AxError::failure(
            AxCode::WireMismatch,
            "encode a client frame",
            err.to_string(),
        )
    })?;
    socket.send_with_str(&text).map_err(|_| {
        AxError::failure(
            AxCode::WireMismatch,
            "send a client frame",
            "the socket is not open",
        )
        .with_recovery("wait for the link to come back; it retries on a fixed ladder")
    })
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

    fn welcome_matching() -> Welcome {
        Welcome {
            wire_v: channels::WIRE_V,
            schema: schema_hash(),
            resume_from: Some(Seq::new(41)),
            city: Address::parse("kiln").ok(),
        }
    }

    /// The four answers a query string has. The city's own codes are
    /// digits and lower-case letters in four hyphenated groups, so this
    /// runs against the shape the host actually produces.
    #[test]
    fn the_pairing_code_is_read_off_the_url_the_host_opened() {
        assert_eq!(
            token_in("?token=hjkmn-pqrtu-vwxyz-23467"),
            Some("hjkmn-pqrtu-vwxyz-23467".to_owned())
        );
        assert_eq!(token_in("?view=city&token=abcde"), Some("abcde".to_owned()));
        assert_eq!(token_in("token=abcde"), Some("abcde".to_owned()));
        assert_eq!(token_in("?token="), None, "an empty value is not a value");
        assert_eq!(token_in(""), None);
        assert_eq!(token_in("?view=city"), None);
        assert_eq!(
            token_in("?tokenish=abcde"),
            None,
            "a name that merely starts like it is not it"
        );
    }

    /// **The defect this closes.** An exposed city asks every peer for a
    /// pairing token, and the page had no way to have one: `app.rs`
    /// built its link with `None`, so the one frame that could have
    /// carried the code went out empty and the server refused its own
    /// client.
    #[test]
    fn the_hello_carries_the_code_the_page_was_opened_with() {
        let mut link = Link::new(token_in("?token=hjkmn-pqrtu-vwxyz-23467"));
        assert_eq!(link.connect(), LinkAction::OpenSocket);
        let LinkAction::Send(hello) = link.advance(LinkEvent::Opened) else {
            panic!("an opened socket says hello");
        };
        assert_eq!(hello.token.as_deref(), Some("hjkmn-pqrtu-vwxyz-23467"));
    }

    #[test]
    fn a_normal_join_opens_says_hello_and_goes_live() {
        let mut link = Link::new(None);
        assert_eq!(link.connect(), LinkAction::OpenSocket);
        assert!(matches!(
            link.advance(LinkEvent::Opened),
            LinkAction::Send(_)
        ));
        assert_eq!(
            link.advance(LinkEvent::Received(Box::new(ServerFrame::Welcome(
                welcome_matching()
            )))),
            LinkAction::Nothing
        );
        assert!(link.is_live());
        assert_eq!(
            link.state(),
            &LinkState::Live {
                resume_from: Some(Seq::new(41))
            }
        );
    }

    #[test]
    fn a_stale_page_is_told_to_reload_and_stops_trying() {
        let mut link = Link::new(None);
        link.connect();
        link.advance(LinkEvent::Opened);
        let stale = Welcome {
            wire_v: channels::WIRE_V.saturating_add(1),
            schema: schema_hash(),
            resume_from: None,
            city: None,
        };
        let LinkAction::Report(err) =
            link.advance(LinkEvent::Received(Box::new(ServerFrame::Welcome(stale))))
        else {
            panic!("a version mismatch must be reported, not retried");
        };
        assert_eq!(*err.code(), AxCode::WireMismatch);
        assert!(err.recovery().contains("reload"));

        // Terminal: further transport noise must not restart the loop.
        assert_eq!(link.advance(LinkEvent::Closed), LinkAction::Nothing);
        assert_eq!(link.advance(LinkEvent::WaitElapsed), LinkAction::Nothing);
        assert!(!link.is_live());
    }

    #[test]
    fn a_dropped_link_climbs_the_ladder_and_then_stays_flat() {
        let mut link = Link::new(None);
        link.connect();
        let mut waits = Vec::new();
        for _ in 0..8 {
            let LinkAction::WaitMs(ms) = link.advance(LinkEvent::Closed) else {
                panic!("a closed socket backs off");
            };
            waits.push(ms);
            link.advance(LinkEvent::WaitElapsed);
            link.advance(LinkEvent::TransportFailed);
            link.advance(LinkEvent::WaitElapsed);
        }
        assert_eq!(waits.first(), Some(&250));
        assert!(
            waits.windows(2).all(|pair| pair[1] >= pair[0]),
            "the ladder never goes back down: {waits:?}"
        );
        assert_eq!(waits.last(), Some(&10_000), "and it stops climbing");
    }

    #[test]
    fn going_live_again_forgives_the_outage_that_preceded_it() {
        let mut link = Link::new(None);
        link.connect();
        for _ in 0..4 {
            link.advance(LinkEvent::Closed);
            link.advance(LinkEvent::WaitElapsed);
        }
        assert!(link.consecutive_failures() > 0);

        link.advance(LinkEvent::Opened);
        link.advance(LinkEvent::Received(Box::new(ServerFrame::Welcome(
            welcome_matching(),
        ))));
        assert!(link.is_live());
        assert_eq!(link.consecutive_failures(), 0);

        // The next outage starts at the bottom of the ladder.
        assert_eq!(link.advance(LinkEvent::Closed), LinkAction::WaitMs(250));
    }

    #[test]
    fn the_ladder_is_a_total_function_of_the_attempt_number() {
        assert_eq!(backoff_ms(0), 250);
        assert_eq!(backoff_ms(5), 10_000);
        assert_eq!(
            backoff_ms(u32::MAX),
            10_000,
            "no index can escape the table"
        );
    }

    #[test]
    fn events_advance_the_resume_cursor_so_a_reconnect_asks_for_less() {
        let mut link = Link::new(None);
        link.connect();
        link.advance(LinkEvent::Opened);
        link.advance(LinkEvent::Received(Box::new(ServerFrame::Welcome(
            welcome_matching(),
        ))));
        let event = channels::EventRecord::from_draft(
            channels::EventDraft {
                run: channels::RunId::from_bytes([1u8; 16]),
                t: channels::TimeMs::new(1),
                who: "server".to_owned(),
                addr: None,
                kind: channels::EventKind::RunStarted,
                data: channels::Payload::empty(),
                ig: false,
            },
            Seq::new(77),
            channels::B3Hash::digest(b"prev"),
        );
        assert!(matches!(
            link.advance(LinkEvent::Received(Box::new(ServerFrame::Event(Box::new(
                event
            ))))),
            LinkAction::Deliver(_)
        ));
        assert_eq!(
            link.state(),
            &LinkState::Live {
                resume_from: Some(Seq::new(77))
            }
        );
    }

    #[test]
    fn a_server_that_streams_before_welcoming_is_not_this_protocol() {
        let mut link = Link::new(None);
        link.connect();
        link.advance(LinkEvent::Opened);
        let event = channels::EventRecord::from_draft(
            channels::EventDraft {
                run: channels::RunId::from_bytes([1u8; 16]),
                t: channels::TimeMs::new(1),
                who: "server".to_owned(),
                addr: None,
                kind: channels::EventKind::RunStarted,
                data: channels::Payload::empty(),
                ig: false,
            },
            Seq::new(1),
            channels::B3Hash::digest(b"prev"),
        );
        assert!(matches!(
            link.advance(LinkEvent::Received(Box::new(ServerFrame::Event(Box::new(
                event
            ))))),
            LinkAction::Report(_)
        ));
        assert!(matches!(link.state(), LinkState::Refused(_)));
    }

    #[test]
    fn a_tab_nobody_is_looking_at_closes_rather_than_slowing_down() {
        let mut link = Link::new(None);
        assert_eq!(link.connect(), LinkAction::OpenSocket);
        link.advance(LinkEvent::Opened);
        assert_eq!(
            link.advance(LinkEvent::Backgrounded),
            LinkAction::CloseSocket,
            "a slowed tab still holds a socket and still wakes the machine"
        );
        assert!(matches!(link.state(), LinkState::Suspended { .. }));
    }

    #[test]
    fn nothing_reaches_a_suspended_link_and_coming_back_starts_clean() {
        let mut link = Link::new(None);
        link.connect();
        link.advance(LinkEvent::Opened);
        link.advance(LinkEvent::Backgrounded);
        // The socket that was closing reports it; that must not restart
        // anything, and it must not count as a failure either.
        assert_eq!(link.advance(LinkEvent::Closed), LinkAction::Nothing);
        assert_eq!(link.advance(LinkEvent::WaitElapsed), LinkAction::Nothing);
        assert_eq!(link.consecutive_failures(), 0);
        assert_eq!(
            link.advance(LinkEvent::Foregrounded),
            LinkAction::OpenSocket,
            "there is a reader again"
        );
    }

    #[test]
    fn a_refusal_does_not_become_less_true_because_somebody_switched_tabs() {
        let mut link = Link::new(None);
        link.connect();
        link.advance(LinkEvent::Opened);
        let stale = Welcome {
            wire_v: channels::WIRE_V.saturating_add(1),
            schema: schema_hash(),
            resume_from: None,
            city: None,
        };
        assert!(matches!(
            link.advance(LinkEvent::Received(Box::new(ServerFrame::Welcome(stale)))),
            LinkAction::Report(_)
        ));
        assert_eq!(link.advance(LinkEvent::Backgrounded), LinkAction::Nothing);
        assert!(matches!(link.state(), LinkState::Refused(_)));
    }
}
