// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The external-provider adapter: a self-written wire-format client over
//! a plain HTTP transport. One call, one HTTP
//! round trip: retries and failover are the watchdog's and admission's
//! decisions, never a hidden loop here.
//!
//! Credentials resolve at the last moment: the `Sealed` value is exposed
//! only while the auth header is written, then dropped (zeroized).

use std::time::Duration;

use kernel::{
    AxCode, AxError, DialectKind, Model, ModelRequest, ModelReturn, Sealed, SecretRef, UsdMicros,
};
use serde_json::Value;

use crate::dialect;

use crate::cost;
use crate::dialect::{request_wire, response_from_wire};
use crate::market::ModelEntry;

/// How the request authenticates. The secret stays a reference until the
/// header is written.
#[non_exhaustive]
#[derive(Debug, Clone)]
pub enum AuthSpec {
    Bearer(SecretRef),
    Header { name: String, value: SecretRef },
    None,
}

/// The redemption face `credential` provides (S3.03): resolve a
/// reference into a sealed value, per operation, never cached.
pub type SecretResolver = Box<dyn Fn(&SecretRef) -> Result<Sealed<String>, AxError> + Send>;

/// Everything one endpoint needs, field by field — no provider
/// abstraction layer eats any of it.
#[derive(Debug, Clone)]
pub struct EndpointConfig {
    /// The full URL of the chat endpoint (e.g. `.../v1/messages`).
    pub base_url: String,
    pub dialect: DialectKind,
    /// Provider-side model name; overrides the duty name from the
    /// canonical request.
    pub model: String,
    pub auth: AuthSpec,
    /// Plaintext headers (never credentials — those go through `auth`).
    pub extra_headers: Vec<(String, String)>,
    /// Field-by-field request overrides: JSON pointer to value, applied
    /// last, later entries win. Missing object paths are created.
    pub overrides: Vec<(String, Value)>,
    pub timeout_ms: u64,
    /// Price-sheet row for settlement; `None` settles nothing (billed
    /// stays empty and attribution sees usage only).
    pub pricing: Option<ModelEntry>,
}

pub struct Endpoint {
    config: EndpointConfig,
    client: reqwest::blocking::Client,
    resolver: SecretResolver,
}

/// A transport failure as its whole chain states it.
///
/// `reqwest`'s own `Display` says only that sending failed; whether it
/// was a refused connection, an unresolvable name or a passed deadline
/// lives one link further down - and that link is the entire difference
/// between "the provider is down" and "the provider is slow" for the
/// person reading the refusal.
fn transport_detail(err: &reqwest::Error) -> String {
    let mut out = err.to_string();
    let mut cause = std::error::Error::source(err);
    while let Some(link) = cause {
        out.push_str(": ");
        out.push_str(&link.to_string());
        cause = link.source();
    }
    out
}

fn provider_err(action: &str, subject: impl Into<String>) -> AxError {
    AxError::failure(AxCode::Provider, action, subject)
        .with_recovery("the watchdog decides retry or failover; admission widens the interval")
}

/// Applies one JSON-pointer override, creating missing object segments.
fn apply_override(root: &mut Value, pointer: &str, value: &Value) -> Result<(), AxError> {
    let path: Vec<&str> = pointer.split('/').filter(|s| !s.is_empty()).collect();
    if path.is_empty() {
        return Err(AxError::failure(
            AxCode::ConfigInvalid,
            "apply request override",
            "empty JSON pointer",
        ));
    }
    let mut cursor = root;
    for (i, segment) in path.iter().enumerate() {
        let last = i == path.len().saturating_sub(1);
        if last {
            match cursor {
                Value::Object(map) => {
                    map.insert((*segment).to_owned(), value.clone());
                    return Ok(());
                }
                _ => {
                    return Err(AxError::failure(
                        AxCode::ConfigInvalid,
                        "apply request override",
                        format!("{pointer}: parent is not an object"),
                    ));
                }
            }
        }
        cursor = match cursor {
            Value::Object(map) => map
                .entry((*segment).to_owned())
                .or_insert_with(|| Value::Object(serde_json::Map::new())),
            _ => {
                return Err(AxError::failure(
                    AxCode::ConfigInvalid,
                    "apply request override",
                    format!("{pointer}: crossed a non-object"),
                ));
            }
        };
    }
    Ok(())
}

impl Endpoint {
    pub fn new(config: EndpointConfig, resolver: SecretResolver) -> Result<Endpoint, AxError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_millis(config.timeout_ms))
            .build()
            .map_err(|err| {
                AxError::failure(AxCode::ConfigInvalid, "build http client", err.to_string())
            })?;
        Ok(Endpoint {
            config,
            client,
            resolver,
        })
    }

    /// What the far side says it serves.
    ///
    /// Both dialects answer `GET .../models` with `{"data":[{"id":..}]}`,
    /// and neither returns prices or token limits there — those are
    /// facts a person confirms at registration, not numbers to guess.
    ///
    /// # Errors
    /// Transport failure, a non-success status, or a body without a
    /// readable `data` array; each carries the URL that was asked.
    pub fn list_models(&self, url: &str) -> Result<Vec<String>, AxError> {
        let mut request = self.client.get(url);
        for (name, value) in &self.config.extra_headers {
            request = request.header(name, value);
        }
        request = self.authorize(request)?;
        let response = request
            .send()
            .map_err(|err| provider_err("list models", transport_detail(&err)))?;
        let status = response.status();
        if !status.is_success() {
            return Err(provider_err(
                "list models",
                format!("{url} answered {}", status.as_u16()),
            ));
        }
        let body: Value = response
            .json()
            .map_err(|err| provider_err("read the model list", err.to_string()))?;
        let rows = body
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| provider_err("read the model list", format!("{url}: no data array")))?;
        let mut ids = Vec::new();
        for row in rows {
            let id = row
                .get("id")
                .and_then(Value::as_str)
                .ok_or_else(|| provider_err("read the model list", "a row has no id"))?;
            ids.push(id.to_owned());
        }
        ids.sort();
        ids.dedup();
        Ok(ids)
    }

    /// Redemption: resolve now, expose only while the header is written,
    /// drop (zeroize) immediately after.
    fn authorize(
        &self,
        request: reqwest::blocking::RequestBuilder,
    ) -> Result<reqwest::blocking::RequestBuilder, AxError> {
        Ok(match &self.config.auth {
            AuthSpec::Bearer(reference) => {
                let sealed = (self.resolver)(reference)?;
                request.header("authorization", format!("Bearer {}", sealed.expose()))
            }
            AuthSpec::Header { name, value } => {
                let sealed = (self.resolver)(value)?;
                request.header(name, sealed.expose().as_str())
            }
            _ => request,
        })
    }

    fn wire_request(&self, req: &ModelRequest) -> Result<Value, AxError> {
        let mut chat = req.chat.clone();
        chat.model = self.config.model.clone();
        let mut wire = request_wire(self.config.dialect, &chat)?;
        for (pointer, value) in &self.config.overrides {
            apply_override(&mut wire, pointer, value)?;
        }
        Ok(wire)
    }
}

impl Endpoint {
    /// One call, with the body read as it arrives.
    ///
    /// **The answer is still the whole answer.** This asks the provider
    /// to stream, reports the text as it lands, and then reads the
    /// settled response out of the terminal frame — so what comes back
    /// is what a non-streaming call would have returned, and a stream cut
    /// halfway is a body read error rather than a shortened reply. The
    /// increments are a thing to watch; they are never the record.
    ///
    /// Two dialects carry a settled answer at the end of their stream in
    /// two shapes, and neither is worth a second parser: the request asks
    /// for a stream only when the caller wants increments, and the reply
    /// is reassembled through the same `response_from_wire` a blocking
    /// call uses.
    fn stream(
        &mut self,
        req: &ModelRequest,
        onto: kernel::Increments<'_>,
    ) -> Result<ModelReturn, AxError> {
        let mut wire = self.wire_request(req)?;
        if let Some(map) = wire.as_object_mut() {
            map.insert("stream".to_owned(), Value::Bool(true));
        }
        let mut request = self
            .client
            .post(&self.config.base_url)
            .header("content-type", "application/json")
            .header("accept", "text/event-stream");
        for (name, value) in &self.config.extra_headers {
            request = request.header(name, value);
        }
        request = self.authorize(request)?;
        let response = request
            .json(&wire)
            .send()
            .map_err(|err| provider_err("call provider", transport_detail(&err)))?;
        let status = response.status();
        if !status.is_success() {
            return Err(provider_err(
                "call provider",
                format!("{} answered {}", self.config.base_url, status.as_u16()),
            ));
        }
        let body = response
            .text()
            .map_err(|err| provider_err("read provider response", transport_detail(&err)))?;
        let mut frames = Vec::new();
        for line in body.lines() {
            let Some(payload) = line.strip_prefix("data:") else {
                continue;
            };
            let payload = payload.trim();
            // The sentinel one dialect ends with. It is not JSON, and
            // treating it as an unreadable frame would turn every
            // successful stream into a warning.
            if payload.is_empty() || payload == "[DONE]" {
                continue;
            }
            // A frame this build cannot read is skipped rather than
            // fatal: providers add event types, and a person watching
            // text arrive must not lose a call because one of them was
            // new. What cannot be skipped is the settled answer, and
            // that is checked below.
            let Ok(frame) = serde_json::from_str::<Value>(payload) else {
                continue;
            };
            if let Some(text) = dialect::increment_of(self.config.dialect, &frame) {
                onto(&text);
            }
            frames.push(frame);
        }
        let settled = dialect::settled_from_stream(self.config.dialect, &frames)?;
        let resp = dialect::response_from_wire(self.config.dialect, &settled)?;
        let billed: Option<UsdMicros> = match &self.config.pricing {
            Some(entry) => Some(cost::settle(&resp.usage, None, entry)?.billed),
            None => None,
        };
        ModelReturn::from_response(resp, billed)
    }
}

impl Model for Endpoint {
    fn call_streaming(
        &mut self,
        req: &ModelRequest,
        onto: kernel::Increments<'_>,
    ) -> Result<ModelReturn, AxError> {
        if req.policy.confidential {
            return Err(self.confidential_refusal());
        }
        self.stream(req, onto)
    }

    fn call(&mut self, req: &ModelRequest) -> Result<ModelReturn, AxError> {
        // A confidential building's bytes do not leave the machine, and
        // this type is the way off it. The refusal is here rather than
        // only at the routing layer because a backstop that lives where
        // the leak would happen survives a routing mistake (P1.08).
        if req.policy.confidential {
            return Err(self.confidential_refusal());
        }
        let wire = self.wire_request(req)?;
        let mut request = self
            .client
            .post(&self.config.base_url)
            .header("content-type", "application/json");
        for (name, value) in &self.config.extra_headers {
            request = request.header(name, value);
        }
        request = self.authorize(request)?;
        let response = request
            .json(&wire)
            .send()
            .map_err(|err| provider_err("call provider", transport_detail(&err)))?;
        let status = response.status();
        if !status.is_success() {
            return Err(provider_err(
                "call provider",
                format!("{} answered {}", self.config.base_url, status.as_u16()),
            ));
        }
        // A cut stream surfaces here as a body read error — no partial
        // ModelReturn is ever fabricated.
        let body: Value = response
            .json()
            .map_err(|err| provider_err("read provider response", transport_detail(&err)))?;
        let resp = response_from_wire(self.config.dialect, &body)?;
        let billed: Option<UsdMicros> = match &self.config.pricing {
            Some(entry) => Some(cost::settle(&resp.usage, None, entry)?.billed),
            None => None,
        };
        ModelReturn::from_response(resp, billed)
    }
}

impl Endpoint {
    /// The one sentence both doors say when a confidential building asks
    /// to leave this machine. Written once because two copies of a
    /// security refusal are two chances for one of them to soften.
    fn confidential_refusal(&self) -> AxError {
        AxError::failure(
            AxCode::GateDenied,
            "call a remote provider for a confidential building",
            self.config.base_url.clone(),
        )
        .with_recovery(
            "configure a local model for this building, or drop `confidential: true` \
             from its BUILDING.md and record why",
        )
    }
}

#[cfg(test)]
#[allow(
    clippy::float_arithmetic,
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice,
    clippy::arithmetic_side_effects,
    reason = "test code"
)]
mod tests {
    use super::*;
    use kernel::{B3Hash, BuildingPolicy, ChatRequest};
    use std::io::{Read, Write};
    use std::net::TcpListener;

    /// A one-shot loopback provider: answers `responses` in order, then
    /// closes. `cut_mid_body` truncates the JSON body mid-way.
    fn fake_provider(
        responses: Vec<(u16, String)>,
        cut_mid_body: bool,
    ) -> (String, std::thread::JoinHandle<Vec<String>>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            for (status, body) in responses {
                let (mut stream, _) = listener.accept().unwrap();
                let mut buf = vec![0u8; 65536];
                let mut request = String::new();
                loop {
                    let n = stream.read(&mut buf).unwrap();
                    request.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if let Some(head_end) = request.find("\r\n\r\n") {
                        let head = &request[..head_end];
                        let content_length = head
                            .lines()
                            .find_map(|l| {
                                l.to_ascii_lowercase()
                                    .strip_prefix("content-length: ")
                                    .map(|v| v.trim().parse::<usize>().unwrap())
                            })
                            .unwrap_or(0);
                        if request.len() >= head_end + 4 + content_length {
                            break;
                        }
                    }
                }
                seen.push(request.clone());
                let payload = if cut_mid_body {
                    let cut = body.len() / 2;
                    let truncated = &body[..cut];
                    format!(
                        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{truncated}",
                        body.len()
                    )
                } else {
                    format!(
                        "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                        body.len()
                    )
                };
                stream.write_all(payload.as_bytes()).unwrap();
                drop(stream);
            }
            seen
        });
        (format!("http://{addr}/v1/messages"), handle)
    }

    /// A provider that accepts the connection and then says nothing.
    ///
    /// This is the shape of a hang, and it is worse than a refusal: a
    /// refusal is an answer the run can act on, while silence stops the
    /// worker thread that would have acted. The deadline is the only
    /// thing standing between the two, so it gets its own test.
    fn silent_provider() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let held = listener.accept();
            // Hold the connection open without answering, long enough
            // for any deadline under test to pass.
            std::thread::sleep(Duration::from_secs(5));
            drop(held);
        });
        format!("http://{addr}/v1/messages")
    }

    fn config(url: &str) -> EndpointConfig {
        EndpointConfig {
            base_url: url.to_owned(),
            dialect: DialectKind::Anthropic,
            model: "provider-model".to_owned(),
            auth: AuthSpec::Header {
                name: "x-api-key".to_owned(),
                value: SecretRef::parse("secret:anthropic/api").unwrap(),
            },
            extra_headers: vec![("anthropic-version".to_owned(), "2023-06-01".to_owned())],
            overrides: vec![(
                "/metadata/user_id".to_owned(),
                Value::String("city".to_owned()),
            )],
            timeout_ms: 5_000,
            pricing: Some(
                crate::market::MarketSnapshot::builtin()
                    .lookup("claude-sonnet")
                    .unwrap()
                    .clone(),
            ),
        }
    }

    fn resolver() -> SecretResolver {
        Box::new(|_reference: &SecretRef| {
            // Runtime-assembled sample: no complete token literal at rest.
            let token = ["sk-test-", "0123456789"].concat();
            Ok(Sealed::new(Box::new(token)))
        })
    }

    fn request() -> ModelRequest {
        ModelRequest {
            policy: BuildingPolicy::default(),
            segments: [B3Hash::digest(b"seg"); 4],
            chat: ChatRequest::empty("duty", 128),
        }
    }

    #[test]
    fn the_probe_reads_ids_and_carries_the_same_credential_the_call_does() {
        let body = serde_json::json!({
            "data": [
                { "id": "m-large", "object": "model" },
                { "id": "m-small", "object": "model" },
                { "id": "m-large", "object": "model" },
            ],
        })
        .to_string();
        let (url, handle) = fake_provider(vec![(200, body)], false);
        let endpoint = Endpoint::new(config(&url), resolver()).unwrap();
        let ids = endpoint.list_models(&url).unwrap();
        assert_eq!(ids, vec!["m-large".to_owned(), "m-small".to_owned()]);
        let seen = handle.join().unwrap();
        assert!(seen[0].starts_with("GET "));
        assert!(seen[0].to_ascii_lowercase().contains("x-api-key: sk-test-"));
    }

    #[test]
    fn a_model_list_without_ids_is_a_provider_error_not_an_empty_city() {
        let (url, handle) = fake_provider(vec![(200, "{\"object\":\"list\"}".to_owned())], false);
        let endpoint = Endpoint::new(config(&url), resolver()).unwrap();
        let err = endpoint.list_models(&url).unwrap_err();
        assert_eq!(err.code(), &AxCode::Provider);
        assert!(err.subject().contains("no data array"));
        let _ = handle.join();
    }

    #[test]
    fn a_provider_that_never_answers_is_given_up_on_at_the_deadline() {
        let url = silent_provider();
        let mut endpoint = Endpoint::new(
            EndpointConfig {
                timeout_ms: 300,
                ..config(&url)
            },
            resolver(),
        )
        .unwrap();
        let err = endpoint.call(&request()).unwrap_err();
        assert_eq!(
            err.code(),
            &AxCode::Provider,
            "silence is a provider failure, and the watchdog's business"
        );
        // The deadline, not the far end, is what ended this call: the
        // server holds the connection open and answers nothing, so a
        // build whose client ignored `timeout` would sit here instead.
        // Asserted on the reason rather than on elapsed time, because
        // this workspace samples no clock (determinism rule two).
        assert!(
            err.subject().to_ascii_lowercase().contains("timed out"),
            "a provider that says nothing must end as a timeout: {}",
            err.subject()
        );
    }

    #[test]
    fn a_confidential_building_never_reaches_a_remote_provider() {
        // No listener at all: if the refusal were routing-level rather
        // than here, this would fail as a transport error instead.
        let mut endpoint =
            Endpoint::new(config("http://127.0.0.1:1/v1/messages"), resolver()).unwrap();
        let mut confidential = request();
        confidential.policy = BuildingPolicy::new(true);
        let err = endpoint.call(&confidential).unwrap_err();
        assert_eq!(err.code(), &AxCode::GateDenied);
        assert!(err.recovery().contains("local model"));
    }

    #[test]
    fn a_success_round_trip_settles_and_maps_the_wave() {
        let body = serde_json::json!({
            "content": [
                { "type": "text", "text": "on it" },
                { "type": "tool_use", "id": "tu_9", "name": "exec", "input": {"cmd": "ls"} },
            ],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 1_000_000, "output_tokens": 0 },
        })
        .to_string();
        let (url, server) = fake_provider(vec![(200, body)], false);
        let mut endpoint = Endpoint::new(config(&url), resolver()).unwrap();
        let ret = endpoint.call(&request()).unwrap();
        assert_eq!(ret.calls.len(), 1);
        assert_eq!(ret.calls[0].id, "tu_9");
        assert_eq!(ret.billed_usd_micros, Some(UsdMicros::new(3_000_000)));
        assert_eq!(ret.stop, Some(kernel::StopReason::ToolUse));
        let seen = server.join().unwrap();
        // Auth header, extra header, override and model name all landed.
        assert!(seen[0].contains("x-api-key: sk-test-0123456789"));
        assert!(seen[0].contains("anthropic-version: 2023-06-01"));
        assert!(seen[0].contains("\"user_id\":\"city\""));
        assert!(seen[0].contains("\"model\":\"provider-model\""));
    }

    #[test]
    fn provider_status_errors_map_to_e_provider_with_status_only() {
        let (url, server) = fake_provider(vec![(429, "{}".to_owned())], false);
        let mut endpoint = Endpoint::new(config(&url), resolver()).unwrap();
        let err = endpoint.call(&request()).unwrap_err();
        assert_eq!(*err.code(), AxCode::Provider);
        assert!(err.subject().contains("429"));
        drop(server);
    }

    #[test]
    fn a_cut_body_is_e_provider_never_a_partial_return() {
        let body = serde_json::json!({
            "content": [ { "type": "text", "text": "half" } ],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 1, "output_tokens": 1 },
        })
        .to_string();
        let (url, server) = fake_provider(vec![(200, body)], true);
        let mut endpoint = Endpoint::new(config(&url), resolver()).unwrap();
        let err = endpoint.call(&request()).unwrap_err();
        assert_eq!(*err.code(), AxCode::Provider);
        drop(server);
    }

    #[test]
    fn overrides_create_missing_paths_and_refuse_non_objects() {
        let mut root = serde_json::json!({"a": 1});
        apply_override(&mut root, "/b/c", &Value::Bool(true)).unwrap();
        assert_eq!(root["b"]["c"], true);
        let err = apply_override(&mut root, "/a/c", &Value::Bool(true)).unwrap_err();
        assert_eq!(*err.code(), AxCode::ConfigInvalid);
    }
}
