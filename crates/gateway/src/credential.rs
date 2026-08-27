// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! Credential custody. Plaintext exists in two places
//! only: the vault, and the last register before the wire. This module
//! owns the effect half of Custody — capture (replace plaintext with
//! `secret:` literals), redemption (resolve per operation, never cached),
//! describe (state without value), and the startup persistence probe that
//! never silently falls back.
//!
//! The inner seam `Vault` stays `pub(crate)`: two sentences of interface,
//! backends and their politics hidden. We never write our own encrypted
//! files — the platform credential service or session memory, nothing
//! between.

use std::collections::BTreeMap;

use kernel::{AxCode, AxError, Payload, Sealed, SecretRef, scan};
use serde_json::{Map, Value};
use zeroize::Zeroizing;

/// The inner seam: store, fetch, delete. Nothing else leaves the crate.
pub(crate) trait Vault {
    fn put(&mut self, reference: &SecretRef, value: Zeroizing<String>) -> Result<(), AxError>;
    fn get(&self, reference: &SecretRef) -> Result<Option<Zeroizing<String>>, AxError>;
    fn delete(&mut self, reference: &SecretRef) -> Result<(), AxError>;
}

/// How long the active backend keeps a value.
#[non_exhaustive]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Persistence {
    AcrossReboots,
    ThisBoot,
    ThisProcess,
}

impl Persistence {
    fn as_str(self) -> &'static str {
        match self {
            Persistence::AcrossReboots => "across_reboots",
            Persistence::ThisBoot => "this_boot",
            Persistence::ThisProcess => "this_process",
        }
    }
}

/// `describe`'s answer: state you can render, value you cannot get.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Described {
    pub configured: bool,
    pub source: String,
    pub persistence: Persistence,
    pub writable: bool,
}

/// Platform credential service via the keyring crate.
struct KeyringVault;

fn keyring_entry(reference: &SecretRef) -> Result<keyring::Entry, AxError> {
    keyring::Entry::new(
        &format!("sprawling/{}", reference.realm()),
        reference.name(),
    )
    .map_err(|err| {
        AxError::failure(
            AxCode::ConfigInvalid,
            "open credential entry",
            err.to_string(),
        )
    })
}

impl Vault for KeyringVault {
    fn put(&mut self, reference: &SecretRef, value: Zeroizing<String>) -> Result<(), AxError> {
        keyring_entry(reference)?
            .set_password(&value)
            .map_err(|err| {
                AxError::failure(AxCode::ConfigInvalid, "store credential", err.to_string())
            })
    }

    fn get(&self, reference: &SecretRef) -> Result<Option<Zeroizing<String>>, AxError> {
        match keyring_entry(reference)?.get_password() {
            Ok(value) => Ok(Some(Zeroizing::new(value))),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(err) => Err(AxError::failure(
                AxCode::ConfigInvalid,
                "fetch credential",
                err.to_string(),
            )),
        }
    }

    fn delete(&mut self, reference: &SecretRef) -> Result<(), AxError> {
        match keyring_entry(reference)?.delete_credential() {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(err) => Err(AxError::failure(
                AxCode::ConfigInvalid,
                "delete credential",
                err.to_string(),
            )),
        }
    }
}

/// Session-memory fallback: honest about its persistence grade.
#[derive(Default)]
struct MemoryVault {
    values: BTreeMap<String, Zeroizing<String>>,
}

impl Vault for MemoryVault {
    fn put(&mut self, reference: &SecretRef, value: Zeroizing<String>) -> Result<(), AxError> {
        self.values.insert(reference.to_string(), value);
        Ok(())
    }

    fn get(&self, reference: &SecretRef) -> Result<Option<Zeroizing<String>>, AxError> {
        Ok(self.values.get(&reference.to_string()).cloned())
    }

    fn delete(&mut self, reference: &SecretRef) -> Result<(), AxError> {
        self.values.remove(&reference.to_string());
        Ok(())
    }
}

/// Reads the read-only source (process environment). Injected so tests
/// can shade without touching the real environment (set_var is unsafe in
/// edition 2024, and tests never mutate shared process state).
pub type EnvReader = Box<dyn Fn(&str) -> Option<String> + Send>;

fn env_key(reference: &SecretRef) -> String {
    let sanitize = |s: &str| -> String {
        s.chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() {
                    c.to_ascii_uppercase()
                } else {
                    '_'
                }
            })
            .collect()
    };
    format!(
        "SPRAWLING_SECRET_{}_{}",
        sanitize(reference.realm()),
        sanitize(reference.name())
    )
}

/// The custody face. One per process; hands out payloads, never values.
pub struct Custodian {
    backend: Box<dyn Vault + Send>,
    source: &'static str,
    persistence: Persistence,
    env: EnvReader,
    captures: u64,
}

/// What `capture` returns: the bytes with plaintext replaced by
/// `secret:` literals, plus the `secret_captured` payloads the caller
/// appends (realm/name/origin/span length only — no plaintext, no hash
/// prefix).
pub struct Captured {
    pub replaced: Vec<u8>,
    pub events: Vec<Payload>,
}

impl Custodian {
    /// Startup probe: write-read-delete against the platform service;
    /// all candidates failing falls back to session memory and returns
    /// the `provider_degraded` payload for the ledger. Never silent.
    pub fn probe() -> (Custodian, Option<Payload>) {
        let probe_ref = match SecretRef::parse("secret:sprawling/startup-probe") {
            Ok(reference) => reference,
            Err(_) => {
                return (
                    Custodian::with_backend(
                        Box::new(MemoryVault::default()),
                        "session-memory",
                        Persistence::ThisProcess,
                    ),
                    degraded_payload("probe reference unparsable"),
                );
            }
        };
        let mut candidate = KeyringVault;
        let round_trip = candidate
            .put(&probe_ref, Zeroizing::new("probe".to_owned()))
            .and_then(|()| candidate.get(&probe_ref))
            .and_then(|read| {
                candidate.delete(&probe_ref)?;
                Ok(read)
            });
        match round_trip {
            Ok(Some(read)) if read.as_str() == "probe" => (
                Custodian::with_backend(
                    Box::new(KeyringVault),
                    "platform-credential-service",
                    Persistence::AcrossReboots,
                ),
                None,
            ),
            Ok(_) => (
                Custodian::with_backend(
                    Box::new(MemoryVault::default()),
                    "session-memory",
                    Persistence::ThisProcess,
                ),
                degraded_payload("platform service returned a different value"),
            ),
            Err(err) => (
                Custodian::with_backend(
                    Box::new(MemoryVault::default()),
                    "session-memory",
                    Persistence::ThisProcess,
                ),
                degraded_payload(err.subject()),
            ),
        }
    }

    /// Session-memory custodian (tests, headless fallback by choice).
    pub fn in_memory() -> Custodian {
        Custodian::with_backend(
            Box::new(MemoryVault::default()),
            "session-memory",
            Persistence::ThisProcess,
        )
    }

    fn with_backend(
        backend: Box<dyn Vault + Send>,
        source: &'static str,
        persistence: Persistence,
    ) -> Custodian {
        Custodian {
            backend,
            source,
            persistence,
            env: Box::new(|key| std::env::var(key).ok()),
            captures: 0,
        }
    }

    /// Test seam: replace the read-only source reader.
    pub fn with_env_reader(mut self, env: EnvReader) -> Custodian {
        self.env = env;
        self
    }

    /// Custody's effect half: spans from `kernel::secret::scan`, values
    /// into the vault, `secret:` literals into the bytes. Deterministic
    /// names: `cap-<n>` under the shape's provider realm.
    pub fn capture(&mut self, bytes: &[u8], origin: &str) -> Result<Captured, AxError> {
        let spans = scan(bytes);
        let mut replaced = bytes.to_vec();
        let mut events = Vec::new();
        // Reverse order keeps earlier offsets valid while splicing.
        for span in spans.iter().rev() {
            let end = span.start.saturating_add(span.len);
            let Some(slice) = bytes.get(span.start..end) else {
                continue;
            };
            let plaintext = Zeroizing::new(String::from_utf8_lossy(slice).into_owned());
            self.captures = self.captures.saturating_add(1);
            let realm = span.provider.unwrap_or("detected");
            let name = format!("cap-{}", self.captures);
            let reference = SecretRef::parse(&format!("secret:{realm}/{name}"))?;
            self.backend.put(&reference, plaintext)?;
            let literal = reference.to_string();
            replaced.splice(span.start..end, literal.bytes());
            let mut event = Map::new();
            event.insert("realm".to_owned(), Value::String(realm.to_owned()));
            event.insert("name".to_owned(), Value::String(name));
            event.insert("origin".to_owned(), Value::String(origin.to_owned()));
            event.insert(
                "span_len".to_owned(),
                Value::Number(u64::try_from(span.len).unwrap_or(0).into()),
            );
            events.push(Payload::new(event)?);
        }
        events.reverse();
        Ok(Captured { replaced, events })
    }

    /// Stores a value. Empty is not a configuration; a shaded reference
    /// (read-only source active) refuses and names the shader. The input
    /// is `Zeroizing`, not `Sealed`: `Sealed` unseals only at the two
    /// wire redemption points, and custody is a store, not a sink —
    /// callers holding a `Sealed` keep it sealed all the way to the wire
    /// (the S4 command face converts inside its own boundary).
    pub fn set(&mut self, reference: &SecretRef, value: Zeroizing<String>) -> Result<(), AxError> {
        if value.is_empty() {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "store credential",
                "empty value is not a configuration",
            ));
        }
        let key = env_key(reference);
        if (self.env)(&key).is_some_and(|v| !v.is_empty()) {
            return Err(AxError::failure(
                AxCode::ConfigInvalid,
                "store credential",
                format!("{reference} is shaded by the environment variable {key}"),
            )
            .with_recovery("unset the environment variable, then store again"));
        }
        self.backend.put(reference, value)
    }

    /// Redemption: resolve per operation. Rotation works because no
    /// second copy survives between operations.
    pub fn resolve(&self, reference: &SecretRef) -> Result<Sealed<String>, AxError> {
        let key = env_key(reference);
        if let Some(value) = (self.env)(&key)
            && !value.is_empty()
        {
            return Ok(Sealed::new(Box::new(value)));
        }
        match self.backend.get(reference)? {
            Some(value) if !value.is_empty() => {
                Ok(Sealed::new(Box::new(value.as_str().to_owned())))
            }
            _ => Err(AxError::failure(
                AxCode::CredentialMissing,
                "resolve credential",
                reference.to_string(),
            )
            .with_recovery("store the credential, or set its environment variable")),
        }
    }

    /// State you can render; the value stays unreachable.
    pub fn describe(&self, reference: &SecretRef) -> Described {
        let key = env_key(reference);
        if (self.env)(&key).is_some_and(|v| !v.is_empty()) {
            return Described {
                configured: true,
                source: format!("environment ({key})"),
                persistence: Persistence::ThisProcess,
                writable: false,
            };
        }
        let configured = matches!(self.backend.get(reference), Ok(Some(v)) if !v.is_empty());
        Described {
            configured,
            source: self.source.to_owned(),
            persistence: self.persistence,
            writable: true,
        }
    }

    pub fn persistence(&self) -> Persistence {
        self.persistence
    }
}

/// PKCE begin (RFC 7636, S256): pure construction — the browser visit
/// and the token POST are the caller's I/O. The verifier arrives from
/// the assembly's seeded randomness (kernel never samples).
pub struct OauthPending {
    pub auth_url: String,
    pub state: String,
    code_verifier: Zeroizing<String>,
}

/// The token-endpoint POST, ready to send: url plus JSON body.
pub struct TokenRequest {
    pub url: String,
    pub body: String,
}

fn base64url_alphabet() -> [u8; 64] {
    // Assembled at runtime: a 64-byte mixed-alphabet literal is exactly
    // the shape the secret scanner hunts (its self-test discipline).
    let mut table = [0u8; 64];
    let mut i = 0usize;
    for range in [b'A'..=b'Z', b'a'..=b'z', b'0'..=b'9'] {
        for c in range {
            if let Some(slot) = table.get_mut(i) {
                *slot = c;
            }
            i = i.saturating_add(1);
        }
    }
    if let Some(slot) = table.get_mut(62) {
        *slot = b'-';
    }
    if let Some(slot) = table.get_mut(63) {
        *slot = b'_';
    }
    table
}

fn base64url_nopad(bytes: &[u8]) -> String {
    let alphabet = base64url_alphabet();
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk.first().copied().unwrap_or(0));
        let b1 = u32::from(chunk.get(1).copied().unwrap_or(0));
        let b2 = u32::from(chunk.get(2).copied().unwrap_or(0));
        let triple = (b0 << 16) | (b1 << 8) | b2;
        let take = match chunk.len() {
            1 => 2,
            2 => 3,
            _ => 4,
        };
        for i in 0..take {
            let i: usize = i;
            let shift = 18usize.saturating_sub(i.saturating_mul(6));
            let index = usize::try_from((triple >> shift) & 0x3f).unwrap_or(0);
            out.push(char::from(*alphabet.get(index).unwrap_or(&b'A')));
        }
    }
    out
}

fn percent_encode(raw: &str) -> String {
    let mut out = String::new();
    for byte in raw.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'.' | b'_' | b'~' => {
                out.push(char::from(byte));
            }
            other => {
                out.push('%');
                out.push_str(&format!("{other:02X}"));
            }
        }
    }
    out
}

/// Encodes caller-supplied entropy into the characters a PKCE verifier
/// and a login state may contain.
///
/// The bytes arrive from the caller because entropy is a host fact and
/// this crate samples nothing. The alphabet lives here because the flow
/// that consumes it lives here, and a second copy of it elsewhere would
/// be a second answer to what a verifier may look like.
#[must_use]
pub fn oauth_random(entropy: &[u8]) -> String {
    base64url_nopad(entropy)
}

/// Fails closed on empty intelligence fields and out-of-range verifiers
/// (RFC 7636: 43..=128 chars).
pub fn oauth_begin(
    profile: &crate::oauth_profiles::OauthProfile,
    code_verifier: String,
    state: String,
) -> Result<OauthPending, AxError> {
    if profile.auth_endpoint.is_empty()
        || profile.token_endpoint.is_empty()
        || profile.client_id.is_empty()
    {
        return Err(AxError::failure(
            AxCode::ConfigInvalid,
            "begin oauth",
            format!("{}: profile intelligence incomplete", profile.provider),
        )
        .with_recovery("fill the oauth_profiles row for this provider"));
    }
    if !(43..=128).contains(&code_verifier.len()) {
        return Err(AxError::failure(
            AxCode::InvalidArgs,
            "begin oauth",
            "code verifier length outside 43..=128",
        ));
    }
    // The two values answer different questions: the verifier proves the
    // client that redeems is the client that asked, and the state proves
    // the redirect came back from the request this process started.
    // Reusing one as the other collapses both proofs into one, and at
    // least one provider now rejects it outright. Refused here rather
    // than trusted to whoever wires the call, because this is exactly
    // the shape an implementation copied from elsewhere arrives in.
    if state == code_verifier {
        return Err(AxError::failure(
            AxCode::InvalidArgs,
            "begin oauth",
            "state and code verifier are the same value",
        )
        .with_recovery(
            "draw the state from randomness of its own; it proves the redirect, not the client",
        ));
    }
    let digest = <sha2::Sha256 as sha2::Digest>::digest(code_verifier.as_bytes());
    let challenge = base64url_nopad(&digest);
    let scope = profile.scopes.join(" ");
    let auth_url = format!(
        "{}?code=true&client_id={}&response_type=code&redirect_uri={}&scope={}&code_challenge={}&code_challenge_method=S256&state={}",
        profile.auth_endpoint,
        percent_encode(profile.client_id),
        percent_encode(profile.redirect_uri),
        percent_encode(&scope),
        challenge,
        percent_encode(&state),
    );
    Ok(OauthPending {
        auth_url,
        state,
        code_verifier: Zeroizing::new(code_verifier),
    })
}

/// What a token endpoint hands back. The two secrets stay wrapped from
/// the moment they are parsed: nothing here logs, formats or returns
/// them by value.
pub struct OauthTokens {
    pub access: Zeroizing<String>,
    /// Absent when the provider issues no refresh token, which is a
    /// fact about that provider rather than a failure.
    pub refresh: Option<Zeroizing<String>>,
    /// Seconds from now, as the provider states them.
    pub expires_in_s: Option<u64>,
}

impl std::fmt::Debug for OauthTokens {
    /// States what was returned, never what it was. `Zeroizing`'s own
    /// `Debug` prints the string, so a derive here would put a live
    /// token into the first panic message that formats one.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "OauthTokens {{ access: <redacted>, refresh: {}, expires_in_s: {:?} }}",
            if self.refresh.is_some() {
                "<redacted>"
            } else {
                "none"
            },
            self.expires_in_s
        )
    }
}

/// Exchanges a refresh token for a fresh pair.
///
/// Same endpoint, same reading, different grant - so it shares the
/// sending and the parsing rather than growing a second copy of them.
///
/// # Errors
/// As [`oauth_redeem`]. A refused refresh means the login is over: this
/// returns the refusal rather than retrying, because a provider that
/// rejects a refresh token will reject it again.
pub fn oauth_refresh(
    profile: &crate::oauth_profiles::OauthProfile,
    refresh: &Sealed<String>,
    timeout_ms: u64,
) -> Result<OauthTokens, AxError> {
    // The one place the stored token is in plain text, and it is the
    // slot before the wire - the same shape as redemption, which is why
    // this file is on the expose whitelist rather than the caller.
    let body = serde_json::json!({
        "grant_type": "refresh_token",
        "refresh_token": refresh.expose(),
        "client_id": profile.client_id,
    });
    send_token_request(profile.token_endpoint, body.to_string(), timeout_ms)
}

/// Sends the redeem POST and reads the tokens out of the answer.
///
/// The send lives here rather than in `endpoint` because this module is
/// the whole OAuth flow: splitting "build the request" from "send it"
/// across two modules would put one exchange under two authorities.
///
/// # Errors
/// Transport failure, a non-success status, a body that is not JSON, and
/// a body without an access token. None of them quote the body: a token
/// endpoint's error page can contain the code that was just sent.
pub fn oauth_redeem(
    profile: &crate::oauth_profiles::OauthProfile,
    pending: &OauthPending,
    code: &str,
    timeout_ms: u64,
) -> Result<OauthTokens, AxError> {
    let request = oauth_redeem_request(profile, pending, code)?;
    send_token_request(&request.url, request.body, timeout_ms)
}

/// The one exchange with a token endpoint: send, read, refuse without
/// quoting. Both grants use it, so neither can drift.
fn send_token_request(url: &str, body: String, timeout_ms: u64) -> Result<OauthTokens, AxError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_millis(timeout_ms))
        .build()
        .map_err(|err| {
            AxError::failure(AxCode::ConfigInvalid, "build http client", err.to_string())
        })?;
    let response = client
        .post(url)
        .header("content-type", "application/json")
        .body(body)
        .send()
        .map_err(|err| {
            AxError::failure(
                AxCode::Provider,
                "redeem an authorization code",
                err.to_string(),
            )
            .with_recovery("check the network and start the login again")
        })?;
    let status = response.status();
    let body: Value = response.json().map_err(|err| {
        AxError::failure(
            AxCode::Provider,
            "read a token answer",
            format!("{}: {err}", status.as_u16()),
        )
        .with_recovery("the provider answered something this version cannot read")
    })?;
    if !status.is_success() {
        // The provider's own words are not quoted: this body is the one
        // place a just-used code can appear in plain text.
        return Err(AxError::failure(
            AxCode::Provider,
            "redeem an authorization code",
            format!("the provider answered {}", status.as_u16()),
        )
        .with_recovery("start the login again; a code can be redeemed once and expires quickly"));
    }
    let access = body
        .get("access_token")
        .and_then(Value::as_str)
        .ok_or_else(|| {
            AxError::failure(
                AxCode::Provider,
                "read a token answer",
                "no access_token in the answer",
            )
            .with_recovery("the provider answered something this version cannot read")
        })?;
    Ok(OauthTokens {
        access: Zeroizing::new(access.to_owned()),
        refresh: body
            .get("refresh_token")
            .and_then(Value::as_str)
            .map(|token| Zeroizing::new(token.to_owned())),
        expires_in_s: body.get("expires_in").and_then(Value::as_u64),
    })
}

/// The redeem POST for the authorization code; the response's token goes
/// straight into the vault via `Custodian::set`.
pub fn oauth_redeem_request(
    profile: &crate::oauth_profiles::OauthProfile,
    pending: &OauthPending,
    code: &str,
) -> Result<TokenRequest, AxError> {
    let body = serde_json::json!({
        "grant_type": "authorization_code",
        "code": code,
        "redirect_uri": profile.redirect_uri,
        "client_id": profile.client_id,
        "code_verifier": pending.code_verifier.as_str(),
        "state": pending.state,
    });
    Ok(TokenRequest {
        url: profile.token_endpoint.to_owned(),
        body: body.to_string(),
    })
}

fn degraded_payload(reason: &str) -> Option<Payload> {
    let mut map = Map::new();
    map.insert("component".to_owned(), Value::String("vault".to_owned()));
    map.insert(
        "fallback".to_owned(),
        Value::String("session-memory".to_owned()),
    );
    map.insert(
        "persistence".to_owned(),
        Value::String(Persistence::ThisProcess.as_str().to_owned()),
    );
    map.insert("reason".to_owned(), Value::String(reason.to_owned()));
    Payload::new(map).ok()
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

    fn sample_token() -> String {
        // Runtime-assembled: the repository never holds a complete
        // high-entropy literal at rest (xtask secret discipline).
        ["sk-ant-api03-", "Zx9yQ2mK4pL7", "vB1nC5tR8sD3"].concat()
    }

    /// A token endpoint that answers one POST with `status` and `body`,
    /// and reports what it was sent.
    fn token_endpoint(status: u16, body: String) -> (String, std::thread::JoinHandle<String>) {
        use std::io::{Read as _, Write as _};
        use std::net::TcpListener;

        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = vec![0u8; 65536];
            let n = stream.read(&mut buf).unwrap();
            let seen = String::from_utf8_lossy(&buf[..n]).into_owned();
            let head = format!(
                "HTTP/1.1 {status} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(body.as_bytes()).unwrap();
            seen
        });
        (format!("http://{addr}/v1/oauth/token"), handle)
    }

    fn profile_at(token_url: &str) -> crate::oauth_profiles::OauthProfile {
        let mut profile = *crate::oauth_profiles::profile("anthropic").unwrap();
        profile.token_endpoint = Box::leak(token_url.to_owned().into_boxed_str());
        profile
    }

    #[test]
    fn a_redeemed_code_becomes_tokens_that_stay_wrapped() {
        let refresh = ["rt-", "K3nD7pQ2", "mX8vB4tL"].concat();
        let body = serde_json::json!({
            "access_token": sample_token(),
            "refresh_token": refresh,
            "expires_in": 3600,
        })
        .to_string();
        let (url, server) = token_endpoint(200, body);
        let profile = profile_at(&url);
        let pending = oauth_begin(
            &profile,
            "v".repeat(64),
            "state-of-its-own-randomness".to_owned(),
        )
        .unwrap();
        let tokens = oauth_redeem(&profile, &pending, "the-code", 5_000).unwrap();
        assert_eq!(tokens.access.as_str(), sample_token());
        assert!(tokens.refresh.is_some());
        assert_eq!(tokens.expires_in_s, Some(3600));

        let sent = server.join().unwrap();
        assert!(sent.contains("the-code"), "the code travels in the body");
        assert!(
            sent.contains(&"v".repeat(64)),
            "and so does the verifier that proves this client asked"
        );
    }

    #[test]
    fn a_refresh_exchanges_the_stored_token_for_a_fresh_pair() {
        let access = ["sk-ant-oat01-", "N7pQ2mK4", "vB1nC5tR"].concat();
        let body = serde_json::json!({ "access_token": access, "expires_in": 3600 }).to_string();
        let (url, server) = token_endpoint(200, body);
        let profile = profile_at(&url);
        let held = Sealed::new(Box::new("the-refresh-token".to_owned()));
        let tokens = oauth_refresh(&profile, &held, 5_000).unwrap();
        assert_eq!(tokens.expires_in_s, Some(3600));
        assert!(
            tokens.refresh.is_none(),
            "a provider that issues no new refresh token is a fact, not a failure"
        );
        let sent = server.join().unwrap();
        assert!(sent.contains("refresh_token"));
        assert!(sent.contains("the-refresh-token"));
    }

    #[test]
    fn a_refused_redeem_says_what_to_do_without_quoting_the_answer() {
        let (url, server) = token_endpoint(
            400,
            serde_json::json!({ "error": "invalid_grant", "code": "the-code" }).to_string(),
        );
        let profile = profile_at(&url);
        let pending = oauth_begin(&profile, "v".repeat(64), "state-value".to_owned()).unwrap();
        let err = oauth_redeem(&profile, &pending, "the-code", 5_000).unwrap_err();
        assert_eq!(err.code(), &AxCode::Provider);
        assert!(err.subject().contains("400"));
        assert!(
            !err.subject().contains("the-code") && !err.recovery().contains("the-code"),
            "a refusal must not carry the code back out: {} / {}",
            err.subject(),
            err.recovery()
        );
        assert!(err.recovery().contains("start the login again"));
        let _ = server.join();
    }

    #[test]
    fn a13_capture_leaves_references_not_plaintext() {
        let mut custodian = Custodian::in_memory();
        let paste = format!("my key is {} thanks", sample_token());
        let captured = custodian.capture(paste.as_bytes(), "paste").unwrap();
        let replaced = String::from_utf8(captured.replaced.clone()).unwrap();
        assert!(!replaced.contains(&sample_token()), "no plaintext left");
        assert!(replaced.contains("secret:anthropic/cap-1"), "{replaced}");
        // The payload names realm/name/origin/len — nothing else.
        let event = serde_json::to_value(&captured.events[0]).unwrap();
        assert_eq!(event["realm"], "anthropic");
        assert_eq!(event["origin"], "paste");
        assert!(event.get("value").is_none());
        assert!(!event.to_string().contains("Zx9yQ2mK4pL7"));
        // Redemption succeeds and stays sealed; describe reports state,
        // never the value. (Value correctness is proven on the wire in
        // `a13_redeemed_value_reaches_the_wire_verbatim` — no expose
        // call exists outside the two redemption points.)
        let reference = SecretRef::parse("secret:anthropic/cap-1").unwrap();
        assert!(custodian.resolve(&reference).is_ok());
        let described = custodian.describe(&reference);
        assert!(described.configured);
        assert_eq!(described.persistence, Persistence::ThisProcess);
    }

    #[test]
    fn a13_redeemed_value_reaches_the_wire_verbatim() {
        use crate::endpoint::{AuthSpec, Endpoint, EndpointConfig};
        use kernel::DialectKind;
        use kernel::{B3Hash, BuildingPolicy, ChatRequest, Model, ModelRequest};
        use std::io::{Read, Write};
        use std::net::TcpListener;

        // Capture the pasted token, then let the endpoint redeem it.
        let mut custodian = Custodian::in_memory();
        let paste = format!("key: {}", sample_token());
        custodian.capture(paste.as_bytes(), "paste").unwrap();
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let server = std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buf = vec![0u8; 65536];
            let n = stream.read(&mut buf).unwrap();
            let seen = String::from_utf8_lossy(&buf[..n]).into_owned();
            let body = serde_json::json!({
                "content": [], "stop_reason": "end_turn",
                "usage": { "input_tokens": 0, "output_tokens": 0 },
            })
            .to_string();
            let head = format!(
                "HTTP/1.1 200 X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                body.len()
            );
            stream.write_all(head.as_bytes()).unwrap();
            stream.write_all(body.as_bytes()).unwrap();
            seen
        });
        let custodian = std::sync::Arc::new(std::sync::Mutex::new(custodian));
        let resolver_handle = std::sync::Arc::clone(&custodian);
        let mut endpoint = Endpoint::new(
            EndpointConfig {
                base_url: format!("http://{addr}/v1/messages"),
                dialect: DialectKind::Anthropic,
                model: "m".to_owned(),
                auth: AuthSpec::Header {
                    name: "x-api-key".to_owned(),
                    value: SecretRef::parse("secret:anthropic/cap-1").unwrap(),
                },
                extra_headers: vec![],
                overrides: vec![],
                timeout_ms: 5_000,
                pricing: None,
            },
            Box::new(move |reference| {
                resolver_handle
                    .lock()
                    .map_err(|_| {
                        AxError::failure(AxCode::CredentialMissing, "resolve credential", "lock")
                    })?
                    .resolve(reference)
            }),
        )
        .unwrap();
        endpoint
            .call(&ModelRequest {
                policy: BuildingPolicy::default(),
                segments: [B3Hash::digest(b"s"); 4],
                chat: ChatRequest::empty("m", 8),
            })
            .unwrap();
        let seen = server.join().unwrap();
        assert!(
            seen.contains(&format!("x-api-key: {}", sample_token())),
            "the vaulted value reaches the wire verbatim"
        );
    }

    #[test]
    fn missing_and_empty_read_as_not_configured() {
        let mut custodian = Custodian::in_memory();
        let reference = SecretRef::parse("secret:acme/key").unwrap();
        let err = match custodian.resolve(&reference) {
            Err(err) => err,
            Ok(_) => panic!("missing credential must not resolve"),
        };
        assert_eq!(*err.code(), AxCode::CredentialMissing);
        let err = custodian
            .set(&reference, Zeroizing::new(String::new()))
            .unwrap_err();
        assert_eq!(*err.code(), AxCode::InvalidArgs);
        assert!(!custodian.describe(&reference).configured);
    }

    #[test]
    fn the_environment_shades_and_set_refuses_naming_the_shader() {
        let mut custodian = Custodian::in_memory().with_env_reader(Box::new(|key| {
            (key == "SPRAWLING_SECRET_ACME_KEY").then(|| "from-env".to_owned())
        }));
        let reference = SecretRef::parse("secret:acme/key").unwrap();
        // Resolve serves the read-only source (sealed).
        assert!(custodian.resolve(&reference).is_ok());
        // Describe shows read-only.
        let described = custodian.describe(&reference);
        assert!(described.configured);
        assert!(!described.writable);
        assert!(described.source.contains("SPRAWLING_SECRET_ACME_KEY"));
        // Set refuses: it would look successful and change nothing.
        let err = custodian
            .set(&reference, Zeroizing::new("new".to_owned()))
            .unwrap_err();
        assert!(err.subject().contains("SPRAWLING_SECRET_ACME_KEY"));
    }

    #[test]
    fn pkce_matches_the_rfc_7636_vector_and_fails_closed() {
        let profile = crate::oauth_profiles::profile("anthropic").unwrap();
        // RFC 7636 Appendix B vector, assembled at runtime (C13: no
        // complete high-entropy literal at rest in the repository).
        let verifier = ["dBjftJeZ4CVP-mB92", "K27uhbUJU1p1r_", "wW1gFWFOEjXk"].concat();
        let expected_challenge = ["E9Melhoa2OwvFrEMTJ", "guCHaoeK1t8URW", "buGJSstw-cM"].concat();
        let pending = oauth_begin(profile, verifier.clone(), "st".to_owned()).unwrap();
        assert!(
            pending
                .auth_url
                .contains(&format!("code_challenge={expected_challenge}")),
            "{}",
            pending.auth_url
        );
        assert!(pending.auth_url.contains("code_challenge_method=S256"));
        let redeem = oauth_redeem_request(profile, &pending, "authcode").unwrap();
        assert_eq!(redeem.url, profile.token_endpoint);
        assert!(redeem.body.contains("authorization_code"));
        // The defect the upstream intelligence sources carry: a state
        // equal to the verifier proves one thing twice and the other
        // thing not at all.
        let reused = oauth_begin(profile, verifier.clone(), verifier.clone())
            .err()
            .expect("state equal to the verifier is refused");
        assert_eq!(reused.code(), &AxCode::InvalidArgs);
        assert!(
            reused.recovery().contains("randomness of its own"),
            "the refusal says what the state is for: {}",
            reused.recovery()
        );

        // Empty intelligence fails closed; short verifiers are refused.
        let openai = crate::oauth_profiles::profile("openai").unwrap();
        assert!(oauth_begin(openai, "x".repeat(50), "s".to_owned()).is_err());
        assert!(oauth_begin(profile, "short".to_owned(), "s".to_owned()).is_err());
    }

    #[test]
    fn rotation_is_next_operation_effective_because_nothing_caches() {
        let mut custodian = Custodian::in_memory();
        let reference = SecretRef::parse("secret:acme/rotating").unwrap();
        custodian
            .set(&reference, Zeroizing::new("one".to_owned()))
            .unwrap();
        assert!(custodian.resolve(&reference).is_ok());
        custodian
            .set(&reference, Zeroizing::new("two".to_owned()))
            .unwrap();
        // No copy survives between operations: the next resolve reads the
        // backend, so the rotated value is what the wire test would see.
        assert!(custodian.resolve(&reference).is_ok());
    }
}
