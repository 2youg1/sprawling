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

use std::time::Duration;

use kernel::{AxCode, AxError, TimeoutMs};

/// A connection to one HTTP server. Cloning shares nothing but the
/// address: HTTP carries no session, so two tools of one server are two
/// independent requests, and the current revision requires exactly that.
#[derive(Debug, Clone)]
pub(crate) struct HttpServer {
    url: String,
    header: Option<(String, String)>,
    client: reqwest::blocking::Client,
}

impl HttpServer {
    /// # Errors
    /// Refuses a header this version cannot split into a name and a
    /// value, and a client this machine cannot build.
    pub(crate) fn open(url: &str, header: Option<&str>) -> Result<HttpServer, AxError> {
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
                Some((name.trim().to_owned(), value.trim().to_owned()))
            }
        };
        let client = reqwest::blocking::Client::builder()
            .build()
            .map_err(|err| {
                AxError::failure(AxCode::ConfigInvalid, "build http client", err.to_string())
            })?;
        Ok(HttpServer {
            url: url.to_owned(),
            header,
            client,
        })
    }
}

impl protocol::Outbound for HttpServer {
    fn call(&mut self, line: &str, patience: TimeoutMs) -> Result<String, AxError> {
        let mut request = self
            .client
            .post(&self.url)
            .timeout(Duration::from_millis(patience.0))
            .header("content-type", "application/json")
            // Both shapes are declared as acceptable because the current
            // revision lets a server answer either way, and a server
            // that answers a shape it was never offered is a server this
            // city would refuse for a reason of its own making.
            .header("accept", "application/json, text/event-stream")
            .body(line.to_owned());
        if let Some((name, value)) = &self.header {
            request = request.header(name, value);
        }
        let response = request.send().map_err(|err| self.unreachable(&err))?;
        let status = response.status();
        let body = response.text().map_err(|err| self.unreachable(&err))?;
        if !status.is_success() {
            // The body is not quoted: a server's error page is other
            // people's text and this refusal is read by a person.
            return Err(AxError::failure(
                AxCode::ToolUnavailable,
                "call an mcp server",
                format!("{}: the server answered {}", self.url, status.as_u16()),
            )
            .with_recovery("check the url and the header this building configured"));
        }
        one_message(&body).ok_or_else(|| {
            AxError::failure(
                AxCode::WireMismatch,
                "read an mcp answer",
                format!("{}: the answer is not one message", self.url),
            )
            .with_recovery("this version reads one JSON body, or the first data line of a stream")
        })
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
        let mut held = HttpServer::open(&url, Some("X-Desk-Key: opaque-value")).unwrap();
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
        let mut held = HttpServer::open(&url, None).unwrap();
        let err = held
            .call("{\"id\":1}", protocol::EXTERNAL_CALL_PATIENCE)
            .unwrap_err();
        assert_eq!(err.code(), &AxCode::ToolUnavailable);
        assert!(err.subject().contains("403"));
        assert!(!err.subject().contains("suspended"));
        let _ = server.join();
    }

    #[test]
    fn a_header_that_is_not_a_header_is_refused_before_any_request() {
        let err = HttpServer::open("http://127.0.0.1:1/mcp", Some("no-colon-here")).unwrap_err();
        assert_eq!(err.code(), &AxCode::ConfigInvalid);
        assert!(err.recovery().contains("Name: value"));
    }
}
