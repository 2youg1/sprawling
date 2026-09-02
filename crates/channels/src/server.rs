// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The listening end, and the humble half of it (ARCHITECTURE section
//! 9). Every branch here is a send, a receive, or the end of a session;
//! the judgements it applies are `channels::reception`'s and the bytes
//! it serves are `channels::assets`'.
//!
//! Five jobs and no policy: serve the client bundle, upgrade a
//! WebSocket, accept an upload, take a credential from a caller on this
//! machine, and let an outside editor drive the city.
//!
//! A refusal made minutes later has no way home, which is why a command
//! carries the [`Reply`] address of whoever sent it.

use std::net::SocketAddr;
use std::sync::Arc;

use axum::Json;
use axum::Router;
use axum::body::Bytes;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::extract::{ConnectInfo, State};
use axum::http::{StatusCode, header};
use axum::response::{IntoResponse, Response};
use axum::routing::{get, post};
use kernel::{Address, AxCode, AxError, B3Hash, EventKind, EventRecord, Sealed};
use serde::{Deserialize, Serialize};
use tokio::sync::broadcast;

use crate::answer::Answer;
use crate::assets::{AssetReply, ClientAssets};
use crate::auth;
use crate::carried_name::UploadId;
use crate::command::{Command, WireCommand};
use crate::reception::{
    BindVerdict, EnrollVerdict, SessionState, SessionStep, decide_bind, decide_enroll, decide_frame,
};
use crate::wire::Query;
use crate::wire::{ClientFrame, ServerFrame};

/// Where an enrolled credential goes. Named because the sink's own
/// shape is the point: it takes the Command set that has no byte form.
///
/// It carries a [`Reply`] for the same reason every other command does:
/// the worker refuses minutes later on another thread, and a refusal
/// with no address reaches nobody.
pub type SecretSink =
    Arc<dyn Fn(Command<Sealed<String>>, Reply) -> Result<(), AxError> + Send + Sync>;

/// How long the enrolment route waits for the worker to say what
/// happened.
///
/// Bounded because the worker may be inside a dispatch that runs for
/// minutes, and an HTTP request that waited for it would look like a
/// hang. Waiting longer would not help in that case and hurts in every
/// other: what is being waited for is a queue hop and a vault write,
/// both of which are milliseconds.
const ENROLMENT_PATIENCE: std::time::Duration = std::time::Duration::from_secs(2);

/// Everything the shell needs that it must not decide for itself.
///
/// `upload_sink` is injected as a closure rather than a trait: `channels`
/// declares no `pub trait` (it is not on the seam list, ARCHITECTURE
/// section 3), and one implementation is not a seam.
pub struct ServeConfig {
    pub addr: SocketAddr,
    /// Digest of the pairing token, never the token. `None` means no token
    /// is configured, which [`decide_bind`] turns into a refusal for any
    /// address reachable beyond this machine.
    pub token_digest: Option<B3Hash>,
    /// The client bundle, handed in by the assembly layer so this crate
    /// never learns where build artifacts live.
    pub client: Arc<ClientAssets>,
    /// Where `Attach` bytes go. Returns the handle a later Command names.
    ///
    /// Takes `Vec<u8>` rather than the transport's own buffer type: the
    /// assembly layer must not have to name axum to hand us a sink, and a
    /// public signature that leaks the HTTP library would make replacing it
    /// a breaking change for every caller.
    pub upload_sink: Arc<dyn Fn(Vec<u8>) -> Result<UploadId, AxError> + Send + Sync>,
    /// Where a request from an outside editor goes. Separate from
    /// `commands` because it arrives over its own route, carries its own
    /// authentication, and gets an answer rather than an event stream.
    pub acp: AcpSink,
    /// Where an accepted Command goes. **Accepting is not running it**: a
    /// dispatch may take hours, and awaiting it inside the socket task
    /// would tie the work to the lifetime of one browser tab. The sink
    /// takes the command, returns, and the progress comes back as events.
    ///
    /// The [`Reply`] travels with the command because the answer arrives
    /// long after this call returned, and a refusal belongs to the peer
    /// that caused it.
    pub commands: Arc<dyn Fn(WireCommand, Reply) -> Result<(), AxError> + Send + Sync>,
    /// The event fan-out. This crate only subscribes; the writer is
    /// whoever owns the Ledger, because the ledger line is the event.
    pub events: broadcast::Sender<EventRecord>,
    /// What a model is saying, while it is still saying it.
    ///
    /// A second channel rather than a second variant on the first,
    /// because the two carry different kinds of thing and lag
    /// differently: an increment a slow client missed is nothing, and an
    /// event it missed is history it must recover. Sharing one channel
    /// would let a burst of increments push records out of a reader's
    /// window.
    pub deltas: broadcast::Sender<crate::wire::Delta>,
    /// Answers a query from the city's derived views. Synchronous: a
    /// query reads a projection, and a projection that needed to block
    /// would be a query pretending to be a command.
    pub queries: Arc<dyn Fn(Query) -> Result<Answer, AxError> + Send + Sync>,
    /// Where an enrolled credential goes. Takes the full [`Command`] and
    /// not [`WireCommand`]: this is the one sink whose input has no byte
    /// form, which is what keeps enrolment a local act.
    pub secrets: SecretSink,
    /// Which city this server is. Told at the handshake, because a client
    /// that only hears what happens next cannot know the name of a city
    /// that was initialised last month.
    pub city: Option<Address>,
}

struct ShellState {
    client: Arc<ClientAssets>,
    upload_sink: Arc<dyn Fn(Vec<u8>) -> Result<UploadId, AxError> + Send + Sync>,
    commands: Arc<dyn Fn(WireCommand, Reply) -> Result<(), AxError> + Send + Sync>,
    events: broadcast::Sender<EventRecord>,
    deltas: broadcast::Sender<crate::wire::Delta>,
    queries: Arc<dyn Fn(Query) -> Result<Answer, AxError> + Send + Sync>,
    secrets: SecretSink,
    acp: AcpSink,
    token_digest: Option<B3Hash>,
    city: Option<Address>,
}

/// One request from an outside editor driving this city as an agent.
///
/// The shape is the protocol's, not this crate's; what this crate adds
/// is that the token is compared here, where the pairing token already
/// lives, and only the verdict travels inward.
#[derive(Debug, Deserialize)]
pub struct AcpBody {
    pub token: String,
    pub addr: String,
    pub task: String,
    pub goal: String,
}

/// What an accepted request gets back: the run it became, and nothing
/// else. Progress is what an editor may see; the city's history is not
/// published through this door.
#[derive(Debug, Serialize)]
pub struct AcpProgress {
    pub run: String,
    pub turns: u32,
    pub finished: bool,
}

/// Where an outside request goes once the token has been judged.
///
/// The boolean is carried rather than acted on here: the refusal for an
/// unauthenticated request is `protocol::admit`'s to word, and it words
/// it so that a stranger learns exactly one bit.
pub type AcpSink = Arc<dyn Fn(AcpBody, bool) -> Result<AcpProgress, AxError> + Send + Sync>;

/// The enrolment body: a realm, a name, and the value that will never be
/// seen again outside the vault.
#[derive(Debug, Deserialize)]
pub struct EnrollBody {
    pub realm: String,
    pub name: String,
    pub value: String,
}

/// Builds the route table. Split from `serve` so a test can exercise the
/// routes over an in-process transport without owning a port.
pub fn router(config: &ServeConfig) -> Router {
    let state = Arc::new(ShellState {
        client: Arc::clone(&config.client),
        upload_sink: Arc::clone(&config.upload_sink),
        commands: Arc::clone(&config.commands),
        events: config.events.clone(),
        deltas: config.deltas.clone(),
        queries: Arc::clone(&config.queries),
        secrets: Arc::clone(&config.secrets),
        acp: Arc::clone(&config.acp),
        token_digest: config.token_digest,
        city: config.city.clone(),
    });
    Router::new()
        .route("/", get(serve_index))
        .route("/ws", get(upgrade))
        .route("/upload", post(accept_upload))
        .route("/enroll", post(accept_enrolment))
        .route("/acp", post(accept_acp))
        .route("/{*asset}", get(serve_asset))
        .with_state(state)
}

/// The one route that carries a credential. It exists as HTTP rather
/// than as a socket frame because the socket's Command type cannot spell
/// a secret; here the same rule is enforced against the peer address.
async fn accept_enrolment(
    State(state): State<Arc<ShellState>>,
    ConnectInfo(peer): ConnectInfo<SocketAddr>,
    body: Bytes,
) -> Response {
    if let EnrollVerdict::Refuse(err) = decide_enroll(&peer) {
        return (StatusCode::FORBIDDEN, refusal_text(&err)).into_response();
    }
    let Ok(enrolment) = serde_json::from_slice::<EnrollBody>(&body) else {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            "send {\"realm\":..,\"name\":..,\"value\":..}",
        )
            .into_response();
    };
    let EnrollBody { realm, name, value } = enrolment;
    let reference = format!("secret:{realm}/{name}");
    let command = Command::PutSecret {
        realm,
        name,
        value: Sealed::new(Box::new(value)),
    };
    // Subscribed before the command is posted: a worker that finished
    // while this task was still setting up would otherwise write the one
    // record this request is waiting for into a stream nobody is reading.
    let mut records = state.events.subscribe();
    let (refused, mut refusals) = tokio::sync::mpsc::unbounded_channel::<AxError>();
    let reply = Reply::to(move |err: AxError| match refused.send(err) {
        Ok(()) => Delivered::ToThePeer,
        Err(_) => Delivered::PeerGone,
    });
    if let Err(err) = (state.secrets)(command, reply) {
        return (StatusCode::UNPROCESSABLE_ENTITY, refusal_text(&err)).into_response();
    }
    let wanted = reference.clone();
    let waited = tokio::time::timeout(ENROLMENT_PATIENCE, async move {
        // The reply address is dropped when the worker finishes without
        // refusing, so a closed refusal channel says only that no
        // refusal is coming - never that the credential was stored. The
        // guard stops the closed branch from spinning; the event is
        // still what a 201 waits for.
        let mut refusal_possible = true;
        loop {
            tokio::select! {
                refusal = refusals.recv(), if refusal_possible => match refusal {
                    Some(err) => return Some(Enrolled::Refused(err)),
                    None => refusal_possible = false,
                },
                record = records.recv() => match record {
                    Ok(record) => {
                        if record.kind() == EventKind::SecretCaptured
                            && record
                                .data()
                                .as_map()
                                .get("ref")
                                .and_then(serde_json::Value::as_str)
                                == Some(wanted.as_str())
                        {
                            return Some(Enrolled::Stored);
                        }
                    }
                    // Lagged means this task missed records, not that the
                    // enrolment failed; the loop keeps waiting and the
                    // timeout below is what ends it.
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return None,
                },
            }
        }
    })
    .await;
    match waited {
        Ok(Some(Enrolled::Stored)) => (StatusCode::CREATED, reference).into_response(),
        Ok(Some(Enrolled::Refused(err))) => {
            (StatusCode::UNPROCESSABLE_ENTITY, refusal_text(&err)).into_response()
        }
        // Neither arrived. Two-oh-two says so in the one word HTTP has
        // for it, and the body says why rather than leaving the caller
        // to read a status code as an outcome.
        Ok(None) | Err(_) => (
            StatusCode::ACCEPTED,
            format!(
                "{reference} was handed to the city and it has not answered within {}s; the                  worker may be inside a dispatch. Check whether the reference resolves before                  sending the credential again",
                ENROLMENT_PATIENCE.as_secs()
            ),
        )
            .into_response(),
    }
}

/// What the city said about one enrolment. Two arms and a timeout, which
/// is three answers, and the route gives each of them its own status.
enum Enrolled {
    Stored,
    Refused(AxError),
}

fn refusal_text(err: &AxError) -> String {
    format!("{}: {}", err.action(), err.recovery())
}

/// Where a refusal ended up.
///
/// Three states rather than a `Result`, because "nobody asked" and "the
/// one who asked has gone" are different facts: the first is the
/// schedule working normally, and the second is worth a diagnostic line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[must_use]
pub enum Delivered {
    ToThePeer,
    NobodyAsked,
    PeerGone,
}

/// Where a refusal goes back to.
///
/// A command travels socket to desk to worker thread, and only events
/// come back, so a refusal made minutes later has no way home. Making
/// it an event instead would tell everyone watching about one person's
/// mistyped URL; the city's history is not anybody's error log. So the
/// command carries the address of whoever sent it.
///
/// Holds a function rather than a channel so that this crate's public
/// signature does not name a transport the assembly layer would then
/// have to name too.
#[derive(Clone)]
pub struct Reply(Option<Arc<dyn Fn(AxError) -> Delivered + Send + Sync>>);

impl Reply {
    /// A reply address that reaches the peer that sent the command.
    pub fn to(sink: impl Fn(AxError) -> Delivered + Send + Sync + 'static) -> Reply {
        Reply(Some(Arc::new(sink)))
    }

    /// No peer asked. The schedule starts work by itself, and so does
    /// the startup scan; a refusal there has nobody to be handed to.
    pub fn nowhere() -> Reply {
        Reply(None)
    }

    /// Hands the refusal back, and says where it ended up.
    pub fn refuse(&self, error: AxError) -> Delivered {
        match &self.0 {
            Some(sink) => sink(error),
            None => Delivered::NobodyAsked,
        }
    }
}

impl std::fmt::Debug for Reply {
    /// Says whether there is somebody to answer, and never what was
    /// said: the payload is an `AxError` on its way to one peer.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let face = if self.0.is_some() {
            "Reply(a peer)"
        } else {
            "Reply(nowhere)"
        };
        f.write_str(face)
    }
}

async fn upgrade(State(state): State<Arc<ShellState>>, ws: WebSocketUpgrade) -> Response {
    ws.on_upgrade(move |socket| session(socket, state))
}

/// The shell around [`decide_frame`]: it moves bytes and holds no policy.
/// Every judgement here is the pure function's; every branch below is
/// either a send, a receive, or the end of the session.
async fn session(mut socket: WebSocket, state: Arc<ShellState>) {
    let mut phase = SessionState::AwaitingHello;
    let mut events = state.events.subscribe();
    let mut deltas = state.deltas.subscribe();
    // This session's own refusals, which the worker posts into long
    // after the command was accepted. Unbounded because a refusal must
    // not be dropped and because its rate is the rate at which one
    // person makes mistakes, not the rate of the event stream.
    let (refused, mut refusals) = tokio::sync::mpsc::unbounded_channel::<AxError>();
    loop {
        tokio::select! {
            incoming = socket.recv() => {
                let Some(Ok(message)) = incoming else { return };
                let Message::Text(text) = message else { continue };
                let Ok(frame) = serde_json::from_str::<ClientFrame>(&text) else {
                    let refusal = AxError::failure(
                        AxCode::WireMismatch,
                        "decode a client frame",
                        "the frame does not match this wire",
                    )
                    .with_recovery("reload the page to fetch the client this server was built with");
                    let _ = send(&mut socket, &ServerFrame::Refusal(Box::new(refusal))).await;
                    return;
                };
                match decide_frame(phase, frame, state.token_digest.as_ref(), state.city.as_ref()) {
                    SessionStep::Welcome(welcome) => {
                        phase = SessionState::Live;
                        if send(&mut socket, &ServerFrame::Welcome(*welcome)).await.is_err() {
                            return;
                        }
                    }
                    SessionStep::Deliver(command) => {
                        let back = refused.clone();
                        let reply = Reply::to(move |error| match back.send(error) {
                            Ok(()) => Delivered::ToThePeer,
                            Err(_) => Delivered::PeerGone,
                        });
                        if let Err(error) = (state.commands)(*command, reply)
                            && send(&mut socket, &ServerFrame::Refusal(Box::new(error))).await.is_err() {
                            return;
                        }
                    }
                    SessionStep::Answer(query) => {
                        // Answering reads the disk and takes a lock, and
                        // it ran here, inside the task that owns this
                        // socket. One `RunHistory` therefore held a
                        // tokio worker thread for the whole read: this
                        // peer received no pushed event for as long as
                        // it lasted, and enough people opening a session
                        // at once could occupy every worker the runtime
                        // has. The blocking pool is where a synchronous
                        // read belongs, and this stays true however fast
                        // the read becomes.
                        let answering = Arc::clone(&state.queries);
                        let asked = *query;
                        let outcome =
                            tokio::task::spawn_blocking(move || answering(asked)).await;
                        let frame = match outcome {
                            Ok(Ok(answer)) => ServerFrame::Answer(Box::new(answer)),
                            Ok(Err(error)) => ServerFrame::Refusal(Box::new(error)),
                            // The pool dropped the work, which means the
                            // runtime is going down; say so rather than
                            // leave the page waiting on a frame that
                            // will never come.
                            Err(_) => ServerFrame::Refusal(Box::new(
                                AxError::failure(
                                    AxCode::StorageFatal,
                                    "answer a query",
                                    "the answering task did not finish",
                                )
                                .with_recovery("ask again; if it repeats, restart the server"),
                            )),
                        };
                        if send(&mut socket, &frame).await.is_err() {
                            return;
                        }
                    }
                    SessionStep::Refuse { error, close } => {
                        let _ = send(&mut socket, &ServerFrame::Refusal(error)).await;
                        if close {
                            return;
                        }
                    }
                }
            }
            // A refusal the worker made after this socket had already
            // answered. It reaches the peer that caused it and nobody
            // else, which is why it travels here and not as an event.
            late = refusals.recv() => {
                let Some(error) = late else { return };
                if send(&mut socket, &ServerFrame::Refusal(Box::new(error))).await.is_err() {
                    return;
                }
            }
            event = events.recv() => {
                match event {
                    Ok(record) => {
                        if phase == SessionState::Live
                            && send(&mut socket, &ServerFrame::Event(Box::new(record))).await.is_err() {
                            return;
                        }
                    }
                    // A slow client loses the middle of the stream rather
                    // than holding the writer back; it recovers from the
                    // ledger on reconnect.
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => return,
                }
            }
            // Increments, on their own channel. Lag is ignored without a
            // word: a missed increment is a missed frame of an animation,
            // and the settled text arrives as a record either way.
            said = deltas.recv() => {
                match said {
                    Ok(delta) => {
                        if phase == SessionState::Live
                            && send(&mut socket, &ServerFrame::Delta(delta)).await.is_err() {
                            return;
                        }
                    }
                    Err(broadcast::error::RecvError::Lagged(_)) => {}
                    Err(broadcast::error::RecvError::Closed) => {}
                }
            }
        }
    }
}

async fn send(socket: &mut WebSocket, frame: &ServerFrame) -> Result<(), ()> {
    let Ok(text) = serde_json::to_string(frame) else {
        return Err(());
    };
    socket
        .send(Message::Text(text.into()))
        .await
        .map_err(|_| ())
}

async fn serve_index(State(state): State<Arc<ShellState>>) -> Response {
    asset_response(state.client.lookup("index.html"))
}

async fn serve_asset(
    State(state): State<Arc<ShellState>>,
    axum::extract::Path(asset): axum::extract::Path<String>,
) -> Response {
    asset_response(state.client.lookup(&asset))
}

/// The shell around [`ClientAssets::lookup`]: headers on, policy out.
fn asset_response(reply: AssetReply) -> Response {
    match reply {
        AssetReply::Found {
            bytes,
            content_type,
            gzipped: true,
        } => (
            StatusCode::OK,
            [
                (header::CONTENT_TYPE, content_type),
                (header::CONTENT_ENCODING, "gzip"),
            ],
            bytes,
        )
            .into_response(),
        AssetReply::Found {
            bytes,
            content_type,
            gzipped: false,
        } => (
            StatusCode::OK,
            [(header::CONTENT_TYPE, content_type)],
            bytes,
        )
            .into_response(),
        AssetReply::Miss(err) => (StatusCode::NOT_FOUND, refusal_text(&err)).into_response(),
    }
}

/// Large attachments travel over HTTP, not as WebSocket frames: a frame
/// carrying hundreds of megabytes is the wrong shape, and HTTP already
/// answers ranges, resumption and progress.
/// An outside editor's request. The token is judged here and the
/// verdict travels inward; an unauthenticated request still reaches the
/// admission, because what it may learn is that admission's to decide.
async fn accept_acp(State(state): State<Arc<ShellState>>, body: Bytes) -> Response {
    let Ok(request) = serde_json::from_slice::<AcpBody>(&body) else {
        return (
            StatusCode::UNPROCESSABLE_ENTITY,
            "send {\"token\":..,\"addr\":..,\"task\":..,\"goal\":..}",
        )
            .into_response();
    };
    let authentic = match state.token_digest.as_ref() {
        // A city with no pairing token configured is a city on loopback
        // only; the door is open to whoever is already on this machine,
        // which is the same rule the control surface follows.
        None => true,
        Some(digest) => auth::verify(Some(request.token.as_str()), digest),
    };
    match (state.acp)(request, authentic) {
        Ok(progress) => (StatusCode::ACCEPTED, Json(progress)).into_response(),
        Err(err) => (StatusCode::FORBIDDEN, refusal_text(&err)).into_response(),
    }
}

async fn accept_upload(State(state): State<Arc<ShellState>>, body: Bytes) -> Response {
    match (state.upload_sink)(body.to_vec()) {
        Ok(id) => (StatusCode::CREATED, id.as_str().to_owned()).into_response(),
        Err(err) => (
            StatusCode::UNPROCESSABLE_ENTITY,
            format!("{}: {}", err.action(), err.recovery()),
        )
            .into_response(),
    }
}

/// Binds and serves until the future is dropped.
///
/// Returns the refusal from [`decide_bind`] without touching the network
/// when the configuration is not allowed to listen.
///
/// # Errors
/// Refuses an exposed bind without a pairing token; propagates the bind and
/// accept failures the operating system reports.
pub async fn serve(config: ServeConfig) -> Result<(), AxError> {
    let face = match decide_bind(&config.addr, config.token_digest.is_some()) {
        BindVerdict::Serve(face) => face,
        BindVerdict::Refuse(err) => return Err(err),
    };
    let listener = tokio::net::TcpListener::bind(config.addr)
        .await
        .map_err(|source| {
            AxError::failure(
                AxCode::ConfigInvalid,
                "bind the control surface",
                format!("{}: {source}", config.addr),
            )
            .with_recovery("choose a free port, or stop the process already holding it")
        })?;
    let app = router(&config);
    let _ = face;
    // Connect info, because one route's policy is the peer's address:
    // a credential may only be enrolled from this machine.
    let app = app.into_make_service_with_connect_info::<SocketAddr>();
    axum::serve(listener, app).await.map_err(|source| {
        AxError::failure(
            AxCode::StorageFatal,
            "serve the control surface",
            source.to_string(),
        )
        .with_recovery("restart the process; the listener is gone")
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

    /// The two facts the acp door decides before anything inward runs:
    /// a configured token must match, and a city with none is loopback
    /// only, which is the same rule the control surface follows.
    #[test]
    fn the_acp_door_judges_the_token_where_the_token_lives() {
        let digest = B3Hash::digest(b"pair-me-0123456789");
        assert!(auth::verify(Some("pair-me-0123456789"), &digest));
        assert!(!auth::verify(Some("pair-me-9876543210"), &digest));
        assert!(
            !auth::verify(None, &digest),
            "a request carrying no token is not authentic against a configured one"
        );
    }
}
