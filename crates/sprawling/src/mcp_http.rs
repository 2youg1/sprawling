// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! An MCP server reached over HTTP: one request carries one message and
//! its answer comes back in the response body.
//!
//! The second transport, and the reason the seam earns its name. What
//! differs from the child process is only where the bytes go: framing,
//! deadlines and refusals keep the same shapes, so nothing above this
//! module knows which kind of server it is talking to.
//!
//! A hosted server may answer as an event stream rather than as one JSON
//! body. This reads the first data line of such a stream and refuses a
//! body it cannot read as one message, rather than guessing at a
//! concatenation - a wrong guess here would be a tool result assembled
//! out of two answers.
//!
//! **The session lives here**, because the specification puts it in the
//! transport rather than in the protocol: a server *may* answer
//! `initialize` with an `Mcp-Session-Id`, and a client that receives one
//! MUST send it back on every later request. A 404 means the server
//! ended the session, and the answer to that is a new handshake rather
//! than a refusal.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use kernel::{AxCode, AxError, Sealed, TimeoutMs};

/// A header a building configured, in the two shapes it may take.
///
/// A paid server's key belongs in the vault like any other credential,
/// so a configured header may name one instead of carrying it. The
/// reference is redeemed in [`HttpServer::post`] and nowhere earlier -
/// the last slot before the wire, which is the same rule the provider
/// endpoint follows.
enum HeaderValue {
    /// Written out in the configuration: an account name, a fixed tag,
    /// anything whose disclosure costs nothing.
    Plain(String),
    /// A `secret:realm/name` reference. Shared rather than copied on
    /// clone, because one server is one credential.
    Sealed(Arc<Sealed<String>>),
}

/// What the far end told us about itself, and what has to travel back.
#[derive(Debug, Default)]
struct Session {
    /// Present only when the server chose to have one.
    id: Option<String>,
    /// The negotiated revision, learned from the answer that carried it.
    protocol_version: Option<String>,
}

/// A connection to one HTTP server.
///
/// Cloning shares the session, because one server is one session however
/// many of its tools a run holds: two clones sending two session ids
/// would be two conversations with one server, and the server is
/// entitled to know only about the one it opened.
#[derive(Clone)]
pub(crate) struct HttpServer {
    url: String,
    header: Option<(String, HeaderValue)>,
    client: reqwest::blocking::Client,
    session: Arc<Mutex<Session>>,
}

impl Clone for HeaderValue {
    fn clone(&self) -> HeaderValue {
        match self {
            HeaderValue::Plain(text) => HeaderValue::Plain(text.clone()),
            HeaderValue::Sealed(held) => HeaderValue::Sealed(Arc::clone(held)),
        }
    }
}

impl std::fmt::Debug for HttpServer {
    /// Names the header but never its value: a configured header may be
    /// a redeemed credential, and a `Debug` that printed it would be
    /// the leak `Sealed` exists to make unspellable.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HttpServer")
            .field("url", &self.url)
            .field("header", &self.header.as_ref().map(|(name, _)| name))
            .finish_non_exhaustive()
    }
}

impl HttpServer {
    /// # Errors
    /// Refuses a header this version cannot split into a name and a
    /// value, and a client this machine cannot build.
    pub(crate) fn open(
        url: &str,
        header: Option<&str>,
        resolve: &gateway::SecretResolver,
    ) -> Result<HttpServer, AxError> {
        let header = match header {
            None => None,
            Some(raw) => {
                let (name, value) = raw.split_once(':').ok_or_else(|| {
                    AxError::failure(
                        AxCode::ConfigInvalid,
                        "reach an mcp server",
                        format!("{raw}: a header is `Name: value`"),
                    )
                    .with_recovery("write the header as `Name: value`, or leave it out")
                })?;
                let value = value.trim();
                // A reference is redeemed from the vault; anything else
                // is what the building wrote. Refused here rather than
                // at the first call, because a key the vault does not
                // hold is a configuration error and not a server that
                // happens to be down.
                let held = match kernel::SecretRef::parse(value) {
                    Ok(reference) => HeaderValue::Sealed(Arc::new(resolve(&reference)?)),
                    Err(_) => HeaderValue::Plain(value.to_owned()),
                };
                Some((name.trim().to_owned(), held))
            }
        };
        let client = reqwest::blocking::Client::builder()
            // Named, because a hosted server sitting behind a content
            // delivery network refuses a client that will not say what
            // it is: reaching Exa's endpoint without this answers 403
            // `browser_signature_banned` before any MCP message is read.
            .user_agent(concat!("sprawling/", env!("CARGO_PKG_VERSION")))
            .build()
            .map_err(|err| {
                AxError::failure(AxCode::ConfigInvalid, "build http client", err.to_string())
            })?;
        Ok(HttpServer {
            url: url.to_owned(),
            header,
            client,
            session: Arc::new(Mutex::new(Session::default())),
        })
    }

    /// One POST, with whatever the session says has to travel with it.
    fn post(&self, line: &str, patience: TimeoutMs) -> Result<Exchange, AxError> {
        let mut request = self
            .client
            .post(&self.url)
            .timeout(Duration::from_millis(patience.0))
            .header("content-type", "application/json")
            // Both shapes are declared as acceptable because the
            // specification requires a client to support either, and a
            // server that answers a shape it was never offered is a
            // server this city would refuse for a reason of its making.
            .header("accept", "application/json, text/event-stream")
            .body(line.to_owned());
        if let Some((name, value)) = &self.header {
            // The redemption point: plaintext exists for the length of
            // this call and never in the configuration, in a log, or in
            // this type's `Debug`.
            request = match value {
                HeaderValue::Plain(text) => request.header(name, text),
                HeaderValue::Sealed(held) => request.header(name, held.expose().as_str()),
            };
        }
        if let Ok(session) = self.session.lock() {
            if let Some(id) = &session.id {
                request = request.header("mcp-session-id", id);
            }
            if let Some(version) = &session.protocol_version {
                request = request.header("mcp-protocol-version", version);
            }
        }
        let response = request.send().map_err(|err| self.unreachable(&err))?;
        let status = response.status().as_u16();
        let handed = response
            .headers()
            .get("mcp-session-id")
            .and_then(|value| value.to_str().ok())
            .map(str::to_owned);
        let body = response.text().map_err(|err| self.unreachable(&err))?;
        Ok(Exchange {
            status,
            handed,
            body,
        })
    }

    /// Records what an exchange taught the session.
    ///
    /// The protocol version is taken from the answer that carries one,
    /// which by the lifecycle is the answer to `initialize`: it is the
    /// first message of a connection, so no earlier answer can hold the
    /// field.
    fn learn(&self, exchange: &Exchange) {
        let Ok(mut session) = self.session.lock() else {
            return;
        };
        if let Some(id) = &exchange.handed {
            session.id = Some(id.clone());
        }
        if session.protocol_version.is_none()
            && let Ok(value) = serde_json::from_str::<serde_json::Value>(&exchange.body)
            && let Some(agreed) = value
                .pointer("/result/protocolVersion")
                .and_then(serde_json::Value::as_str)
        {
            session.protocol_version = Some(agreed.to_owned());
        }
    }

    /// Forgets a session the server says is gone.
    ///
    /// The specification's answer to a 404 on a request carrying a
    /// session id is to open a new session, so the id is dropped here
    /// and the next handshake starts without one.
    fn forget(&self) {
        if let Ok(mut session) = self.session.lock() {
            session.id = None;
        }
    }

    /// Turns a status the server chose into the refusal a person reads.
    fn refused(&self, status: u16) -> AxError {
        if status == 404 {
            self.forget();
            return AxError::failure(
                AxCode::ToolUnavailable,
                "call an mcp server",
                format!("{}: the server ended this session", self.url),
            )
            .with_recovery("the session was dropped; dispatch again to open a new one")
            .retriable();
        }
        // The body is not quoted: a server's error page is other
        // people's text and this refusal is read by a person.
        AxError::failure(
            AxCode::ToolUnavailable,
            "call an mcp server",
            format!("{}: the server answered {status}", self.url),
        )
        .with_recovery("check the url and the header this building configured")
    }
}

/// One request and what came back, before anything is decided about it.
struct Exchange {
    status: u16,
    /// The `Mcp-Session-Id` the server handed out, if it opened one.
    handed: Option<String>,
    body: String,
}

impl protocol::Outbound for HttpServer {
    fn call(&mut self, line: &str, patience: TimeoutMs) -> Result<String, AxError> {
        let exchange = self.post(line, patience)?;
        if !(200..300).contains(&exchange.status) {
            return Err(self.refused(exchange.status));
        }
        self.learn(&exchange);
        one_message(&exchange.body).ok_or_else(|| {
            AxError::failure(
                AxCode::WireMismatch,
                "read an mcp answer",
                format!("{}: the answer is not one message", self.url),
            )
            .with_recovery("this version reads one JSON body, or the first data line of a stream")
        })
    }

    /// A notification is answered with 202 and no body, so nothing is
    /// read back. A session id may still arrive here, and is kept.
    fn notify(&mut self, line: &str, patience: TimeoutMs) -> Result<(), AxError> {
        let exchange = self.post(line, patience)?;
        if !(200..300).contains(&exchange.status) {
            return Err(self.refused(exchange.status));
        }
        self.learn(&exchange);
        Ok(())
    }
}

impl HttpServer {
    fn unreachable(&self, err: &reqwest::Error) -> AxError {
        let mut detail = err.to_string();
        let mut cause = std::error::Error::source(err);
        while let Some(link) = cause {
            detail.push_str(": ");
            detail.push_str(&link.to_string());
            cause = link.source();
        }
        AxError::failure(
            AxCode::ToolUnavailable,
            "call an mcp server",
            format!("{}: {detail}", self.url),
        )
        .with_recovery("the server is not answering; this run continues without it")
    }
}

/// The one message a body carries, whether it arrived as a JSON body or
/// as the first data line of an event stream.
fn one_message(body: &str) -> Option<String> {
    let trimmed = body.trim();
    if trimmed.starts_with('{') {
        return Some(trimmed.to_owned());
    }
    trimmed
        .lines()
        .find_map(|line| line.strip_prefix("data:"))
        .map(|line| line.trim().to_owned())
        .filter(|line| line.starts_with('{'))
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
    use protocol::Outbound as _;

    /// A vault holding one credential, so a configured header that names
    /// one can be redeemed the way a real building's would be.
    fn vault() -> gateway::SecretResolver {
        Box::new(|reference: &kernel::SecretRef| {
            Ok(kernel::Sealed::new(Box::new(format!(
                "held-{}",
                reference.name()
            ))))
        })
    }

    /// A server that answers one POST with `status` and `body`.
    fn fake_server(status: u16, body: String) -> (String, std::thread::JoinHandle<String>) {
        use std::io::{Read as _, Write as _};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = vec![0u8; 65536];
            let read = stream.read(&mut buf).unwrap();
            let seen = String::from_utf8_lossy(&buf[..read]).into_owned();
            let head = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(body.as_bytes()).unwrap();
            seen
        });
        (format!("http://{addr}/mcp"), handle)
    }

    #[test]
    fn a_hosted_server_answers_and_the_configured_header_travels() {
        let (url, server) = fake_server(
            200,
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"tools\":[]}}".to_owned(),
        );
        let mut held = HttpServer::open(&url, Some("X-Desk-Key: opaque-value"), &vault()).unwrap();
        let answer = held
            .call(
                &protocol::Rpc::new().list_tools(),
                protocol::EXTERNAL_CALL_PATIENCE,
            )
            .unwrap();
        assert!(protocol::Rpc::read(&answer).is_ok());
        let sent = server.join().unwrap().to_ascii_lowercase();
        assert!(sent.contains("x-desk-key: opaque-value"));
        assert!(sent.contains("accept: application/json, text/event-stream"));
    }

    #[test]
    fn an_answer_that_arrives_as_a_stream_is_read_as_one_message() {
        assert_eq!(
            one_message("event: message\ndata: {\"result\":{}}\n\n").as_deref(),
            Some("{\"result\":{}}")
        );
        assert_eq!(
            one_message(" {\"result\":{}} ").as_deref(),
            Some("{\"result\":{}}")
        );
        assert!(
            one_message("event: ping\n\n").is_none(),
            "a stream carrying no message is refused rather than joined into one"
        );
    }

    #[test]
    fn a_refusing_server_states_the_status_without_quoting_its_page() {
        let (url, server) = fake_server(403, "{\"error\":\"account suspended\"}".to_owned());
        let mut held = HttpServer::open(&url, None, &vault()).unwrap();
        let err = held
            .call("{\"id\":1}", protocol::EXTERNAL_CALL_PATIENCE)
            .unwrap_err();
        assert_eq!(err.code(), &AxCode::ToolUnavailable);
        assert!(err.subject().contains("403"));
        assert!(!err.subject().contains("suspended"));
        let _ = server.join();
    }

    /// A server that answers `rounds` requests, each with `body`, and
    /// hands out `session` on the first. Returns everything it was sent.
    fn sessioned_server(
        rounds: usize,
        session: &'static str,
        body: &'static str,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{Read as _, Write as _};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for round in 0..rounds {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = vec![0u8; 65536];
                let read = stream.read(&mut buf).unwrap();
                seen.push(String::from_utf8_lossy(&buf[..read]).into_owned());
                let handed = if round == 0 {
                    format!("mcp-session-id: {session}\r\n")
                } else {
                    String::new()
                };
                let head = format!(
                    "HTTP/1.1 200 X\r\ncontent-type: application/json\r\n{handed}\
                     content-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                stream.write_all(head.as_bytes()).unwrap();
                stream.write_all(body.as_bytes()).unwrap();
            }
            seen
        });
        (format!("http://{addr}/mcp"), handle)
    }

    /// The whole reason this card exists: a session id handed out at
    /// initialization has to travel on every later request, and so does
    /// the negotiated protocol version. Without them a server that
    /// keeps state answers 400 to everything after the handshake.
    #[test]
    fn a_session_handed_out_at_initialization_travels_on_every_later_request() {
        const OPENED: &str = "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"protocolVersion\":\"2025-03-26\",\"capabilities\":{},\"serverInfo\":{\"name\":\"hosted\",\"version\":\"1\"},\"tools\":[]}}";
        let (url, server) = sessioned_server(3, "session-abc", OPENED);
        let mut held = HttpServer::open(&url, None, &vault()).unwrap();
        let mut rpc = protocol::Rpc::new();
        let opened =
            protocol::handshake(&mut held, &mut rpc, protocol::EXTERNAL_CALL_PATIENCE).unwrap();
        assert_eq!(opened.protocol_version, "2025-03-26");
        held.call(&rpc.list_tools(), protocol::EXTERNAL_CALL_PATIENCE)
            .unwrap();

        let sent = server.join().unwrap();
        let first = sent[0].to_ascii_lowercase();
        assert!(first.contains("\"method\":\"initialize\""), "{first}");
        assert!(
            !first.contains("mcp-session-id"),
            "there is no session to name before one is handed out"
        );
        for later in &sent[1..] {
            let later = later.to_ascii_lowercase();
            assert!(later.contains("mcp-session-id: session-abc"), "{later}");
            assert!(
                later.contains("mcp-protocol-version: 2025-03-26"),
                "{later}"
            );
        }
        assert!(sent[1].contains("notifications/initialized"));
    }

    /// A 404 on a request carrying a session id means the server ended
    /// the session. The id is dropped so the next handshake opens a new
    /// one, which is what the specification asks for; keeping it would
    /// make every later request fail the same way for ever.
    #[test]
    fn a_session_the_server_ended_is_forgotten_rather_than_kept() {
        let (url, server) = fake_server(404, "{}".to_owned());
        let mut held = HttpServer::open(&url, None, &vault()).unwrap();
        if let Ok(mut session) = held.session.lock() {
            session.id = Some("stale".to_owned());
        }
        let err = held
            .call("{\"id\":1}", protocol::EXTERNAL_CALL_PATIENCE)
            .unwrap_err();
        assert!(err.subject().contains("ended this session"));
        assert_eq!(
            held.session.lock().unwrap().id,
            None,
            "a session the server disowned is not sent back to it"
        );
        let _ = server.join();
    }

    /// A paid server's key belongs in the vault, not in a building's
    /// `CONFIG.toml`. `xtask secret` cannot see a city's configuration,
    /// because a city is not in this repository - so nothing but this
    /// would have caught a key written out there in plaintext.
    #[test]
    fn a_header_naming_a_credential_carries_the_key_and_never_the_reference() {
        let (url, server) = fake_server(
            200,
            "{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{}}".to_owned(),
        );
        let mut held = HttpServer::open(&url, Some("X-Api-Key: secret:exa/api"), &vault()).unwrap();
        held.call("{\"id\":1}", protocol::EXTERNAL_CALL_PATIENCE)
            .unwrap();
        let sent = server.join().unwrap();
        assert!(sent.contains("x-api-key: held-api"), "{sent}");
        assert!(
            !sent.contains("secret:"),
            "the reference is redeemed, not forwarded"
        );
    }

    /// The value is never in `Debug` either: a diagnostic line is the
    /// easiest place for a credential to escape to.
    #[test]
    fn the_debug_face_names_the_header_and_never_its_value() {
        let held = HttpServer::open(
            "http://127.0.0.1:1/mcp",
            Some("X-Api-Key: secret:exa/api"),
            &vault(),
        )
        .unwrap();
        let drawn = format!("{held:?}");
        assert!(drawn.contains("X-Api-Key"));
        assert!(!drawn.contains("held-api"));
    }

    #[test]
    fn a_header_that_is_not_a_header_is_refused_before_any_request() {
        let err = HttpServer::open("http://127.0.0.1:1/mcp", Some("no-colon-here"), &vault())
            .unwrap_err();
        assert_eq!(err.code(), &AxCode::ConfigInvalid);
        assert!(err.recovery().contains("Name: value"));
    }
}
