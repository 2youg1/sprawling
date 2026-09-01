// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The listening end. Humble Object (ARCHITECTURE section 7): the two
//! judgements that matter - may we bind this address, may we accept this
//! peer - are pure functions with exhaustive verdicts, tested without a
//! socket. The async shell around them owns three jobs and no policy:
//! serve the client bundle, upgrade a WebSocket, accept an upload, and
//! take a credential from a caller on this machine.
//!
//! Asset lookup is a pure function too ([`ClientAssets::lookup`]): which
//! bytes answer which path, whether they are gzipped, and why a miss is a
//! miss are all decided without HTTP, so the handler is three lines.
//!
//! Binding is loopback by default. Exposing the port demands a pairing
//! token, and a missing token refuses the *start*, not the connection:
//! "data stays on your team" is a judgement or it is decoration.

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

use crate::auth;
use crate::wire::{
    Answer, ClientFrame, Command, Hello, ServerFrame, UploadId, WIRE_V, Welcome, WireCommand,
};
use crate::wire::{Query, schema_hash};

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

/// One file of the embedded client: the path a browser requests and the
/// gzip bytes the build wrote into the binary.
#[derive(Debug, Clone, Copy)]
pub struct EmbeddedFile {
    /// Request path relative to the root, forward slashes, no leading
    /// slash: `index.html`, `web.js`, `snippets/<crate>/inline0.js`.
    pub path: &'static str,
    /// The file, gzip-compressed at build time with a zeroed timestamp,
    /// so the same source bytes embed identically on every build.
    pub gz: &'static [u8],
}

/// The client the browser downloads, from whichever source this process
/// was given. Two sources, one contract: the release binary carries the
/// bundle inside itself; a development loop points at the directory the
/// wasm build writes, read per request so an edit shows on refresh.
#[derive(Debug)]
pub enum ClientAssets {
    /// The bundle inside the binary, one gzip per file.
    Embedded(&'static [EmbeddedFile]),
    /// A directory on disk, read per request. Loopback development only;
    /// the release path never constructs this arm.
    Disk(std::path::PathBuf),
}

/// What one asset request gets back.
#[derive(Debug)]
pub enum AssetReply {
    Found {
        bytes: Vec<u8>,
        content_type: &'static str,
        /// Whether `bytes` are gzip and need `Content-Encoding: gzip`.
        gzipped: bool,
    },
    Miss(AxError),
}

impl ClientAssets {
    /// Answers one request path. Pure over the embedded arm; the disk arm
    /// reads exactly the file the sanitised path names.
    ///
    /// `""` and `"/"` mean `index.html`. A path that steps outside the
    /// bundle (`..`, empty segments, drive letters, leading dots) is a
    /// miss, not an error: the bundle is the whole world this route knows.
    #[must_use]
    pub fn lookup(&self, request_path: &str) -> AssetReply {
        let Some(rel) = sanitize_asset_path(request_path) else {
            return AssetReply::Miss(
                AxError::failure(
                    AxCode::InvalidArgs,
                    "serve a client asset",
                    format!("the path {request_path} steps outside the client bundle"),
                )
                .with_recovery("request a bundle-relative path such as /web.js"),
            );
        };
        match self {
            ClientAssets::Embedded(files) => {
                for file in *files {
                    if file.path == rel {
                        return AssetReply::Found {
                            bytes: file.gz.to_vec(),
                            content_type: content_type_of(&rel),
                            gzipped: true,
                        };
                    }
                }
                AssetReply::Miss(missing_asset(&rel, "this binary"))
            }
            ClientAssets::Disk(root) => {
                let full = root.join(&rel);
                match std::fs::read(&full) {
                    Ok(bytes) => AssetReply::Found {
                        bytes,
                        content_type: content_type_of(&rel),
                        gzipped: false,
                    },
                    Err(_) => AssetReply::Miss(missing_asset(&rel, "the --web-dir directory")),
                }
            }
        }
    }
}

fn missing_asset(rel: &str, source: &str) -> AxError {
    AxError::failure(
        AxCode::InvalidArgs,
        "serve a client asset",
        format!("{source} does not carry {rel}"),
    )
    .with_recovery("rebuild the client (`just build-web`), then rebuild or restart the server")
}

/// Normalises a request path to a bundle-relative one, or refuses.
/// Rejects `..`, empty segments, backslashes, drive colons and segments
/// that start with a dot; empty input means the index page.
fn sanitize_asset_path(raw: &str) -> Option<String> {
    let trimmed = raw.trim_start_matches('/');
    if trimmed.is_empty() {
        return Some("index.html".to_owned());
    }
    if trimmed.contains('\\') || trimmed.contains(':') {
        return None;
    }
    let mut segments = Vec::new();
    for segment in trimmed.split('/') {
        if segment.is_empty() || segment.starts_with('.') {
            return None;
        }
        segments.push(segment);
    }
    Some(segments.join("/"))
}

/// The content type a browser needs to run the client. `application/wasm`
/// is load-bearing: `WebAssembly.instantiateStreaming` refuses anything
/// else and the loader falls back to a slower path with a console warning.
fn content_type_of(rel: &str) -> &'static str {
    let suffix = rel.rsplit('.').next().unwrap_or_default();
    match suffix {
        "html" => "text/html; charset=utf-8",
        "js" | "mjs" => "text/javascript; charset=utf-8",
        "wasm" => "application/wasm",
        "css" => "text/css; charset=utf-8",
        "json" | "map" => "application/json",
        "svg" => "image/svg+xml",
        _ => "application/octet-stream",
    }
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

    const BUNDLE: &[EmbeddedFile] = &[
        EmbeddedFile {
            path: "index.html",
            gz: b"gzipped-html",
        },
        EmbeddedFile {
            path: "web.js",
            gz: b"gzipped-js",
        },
        EmbeddedFile {
            path: "web_bg.wasm",
            gz: b"gzipped-wasm",
        },
        EmbeddedFile {
            path: "snippets/dioxus-abc/inline0.js",
            gz: b"gzipped-snippet",
        },
    ];

    #[test]
    fn the_embedded_bundle_answers_the_paths_the_page_asks_for() {
        let assets = ClientAssets::Embedded(BUNDLE);
        for (asked, want_type, want_bytes) in [
            ("", "text/html; charset=utf-8", b"gzipped-html".as_slice()),
            ("/", "text/html; charset=utf-8", b"gzipped-html".as_slice()),
            (
                "web.js",
                "text/javascript; charset=utf-8",
                b"gzipped-js".as_slice(),
            ),
            (
                "/web_bg.wasm",
                "application/wasm",
                b"gzipped-wasm".as_slice(),
            ),
            (
                "snippets/dioxus-abc/inline0.js",
                "text/javascript; charset=utf-8",
                b"gzipped-snippet".as_slice(),
            ),
        ] {
            let AssetReply::Found {
                bytes,
                content_type,
                gzipped,
            } = assets.lookup(asked)
            else {
                panic!("{asked} must be found");
            };
            assert_eq!(bytes, want_bytes, "{asked}");
            assert_eq!(content_type, want_type, "{asked}");
            assert!(gzipped, "embedded files travel gzipped");
        }
    }

    #[test]
    fn a_path_outside_the_bundle_is_a_miss_that_names_the_path() {
        let assets = ClientAssets::Embedded(BUNDLE);
        for hostile in [
            "../Cargo.toml",
            "a/../../secret",
            "a//b.js",
            "C:/windows/system32",
            "snippets\\x\\y.js",
            ".git/config",
        ] {
            let AssetReply::Miss(err) = assets.lookup(hostile) else {
                panic!("{hostile} must miss");
            };
            assert!(!err.recovery().is_empty(), "a miss says what to do next");
        }
    }

    #[test]
    fn an_unknown_file_misses_and_the_recovery_names_the_rebuild() {
        let assets = ClientAssets::Embedded(BUNDLE);
        let AssetReply::Miss(err) = assets.lookup("missing.js") else {
            panic!("an absent file is a miss");
        };
        assert!(err.recovery().contains("just build-web"));
    }

    #[test]
    fn the_disk_arm_reads_per_request_and_never_claims_gzip() {
        let dir = std::env::temp_dir().join(format!("sprawl-assets-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("web.js"), b"fresh from disk").unwrap();
        let assets = ClientAssets::Disk(dir.clone());
        let AssetReply::Found {
            bytes,
            content_type,
            gzipped,
        } = assets.lookup("/web.js")
        else {
            panic!("the file exists");
        };
        assert_eq!(bytes, b"fresh from disk");
        assert_eq!(content_type, "text/javascript; charset=utf-8");
        assert!(!gzipped, "disk bytes are identity-encoded");
        std::fs::write(dir.join("web.js"), b"edited").unwrap();
        let AssetReply::Found { bytes, .. } = assets.lookup("web.js") else {
            panic!("still there");
        };
        assert_eq!(
            bytes, b"edited",
            "a refresh sees the edit without a restart"
        );
        std::fs::remove_dir_all(&dir).ok();
    }
}
