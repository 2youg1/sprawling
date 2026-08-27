// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! The book of attached endpoints and the models chosen from them
//! (shape 7): a value rebuilt from the ledger, never a store. Deleting
//! it and replaying the city produces the same book.
//!
//! Two questions live here and nowhere else. *What did the person
//! attach* — a base URL, a dialect, a credential reference, and the
//! model ids the endpoint reported. *Which model answers for a tag* —
//! the choice a request needs, with the facts no probe returns.
//!
//! Duty pools and multi-axis grading are deliberately absent: with one
//! agent per run there is no consumer for a pool, and a pool without a
//! consumer is an authority nobody reads.

use std::collections::BTreeMap;

use kernel::{
    AxCode, AxError, BuildingPolicy, DialectKind, EventKind, EventRecord, ModelTag, Payload,
    SecretRef, UsdMicros,
};
use serde_json::{Map, Value};

use crate::endpoint::AuthSpec;
use crate::market::ModelEntry;
use crate::native::is_loopback;

/// One endpoint the person attached, as the book holds it.
#[derive(Debug, Clone)]
pub struct AttachedEndpoint {
    pub name: String,
    /// The base URL the person entered, without a path of its own; the
    /// dialect knows which paths hang off it.
    pub base_url: String,
    pub dialect: DialectKind,
    pub auth: AuthSpec,
    /// What the endpoint said it serves. Ids only: no provider returns
    /// prices or limits from its model list, and a number we invented
    /// would outrank the one the provider actually bills.
    pub models: Vec<String>,
}

impl AttachedEndpoint {
    /// Whether calls to this endpoint stay on this machine. The answer
    /// comes from the same test the local adapter applies, so "local"
    /// means one thing city-wide.
    #[must_use]
    pub fn is_local(&self) -> bool {
        is_loopback(&self.base_url)
    }

    /// Whether a credential was enrolled for it. The only question about
    /// a credential this side of the vault can answer, and the only one
    /// a settings page needs.
    #[must_use]
    pub fn has_credential(&self) -> bool {
        auth_reference(&self.auth).is_some()
    }

    /// Where a chat request goes for this dialect. The person enters a
    /// base URL because that is what a provider's documentation prints;
    /// the path belongs to the dialect, which is the only party that
    /// knows it.
    #[must_use]
    pub fn chat_url(&self) -> String {
        join(&self.base_url, chat_path(self.dialect))
    }

    /// Where the model list lives for this dialect.
    #[must_use]
    pub fn models_url(&self) -> String {
        join(&self.base_url, "models")
    }
}

/// Both dialects list models at the same path; they differ in the chat
/// path and in the shape of what comes back.
fn chat_path(dialect: DialectKind) -> &'static str {
    match dialect {
        DialectKind::Anthropic => "messages",
        // Anything not spelled here is served by the OpenAI-compatible
        // shape, which is what an unknown local server almost always is.
        _ => "chat/completions",
    }
}

fn join(base: &str, path: &str) -> String {
    format!("{}/{}", base.trim_end_matches('/'), path)
}

/// The model that answers for one tag, and the endpoint it lives behind.
#[derive(Debug, Clone, Copy)]
pub struct Chosen<'b> {
    pub endpoint: &'b AttachedEndpoint,
    pub entry: &'b ModelEntry,
}

#[derive(Debug, Clone)]
struct Choice {
    endpoint: String,
    entry: ModelEntry,
}

/// Every endpoint and every choice, rebuilt from the event stream.
#[derive(Debug, Clone, Default)]
pub struct EndpointBook {
    endpoints: BTreeMap<String, AttachedEndpoint>,
    chosen: BTreeMap<ModelTag, Choice>,
}

impl EndpointBook {
    #[must_use]
    pub fn new() -> EndpointBook {
        EndpointBook::default()
    }

    /// Folds one record. Records of other kinds pass through untouched,
    /// so a caller may hand it the whole stream.
    ///
    /// # Errors
    /// A payload of a kind this book owns that it cannot read is an
    /// error rather than a skipped record: a book that quietly drops a
    /// registration would send calls to an endpoint the person retired.
    pub fn apply(&mut self, record: &EventRecord) -> Result<(), AxError> {
        self.apply_payload(record.kind(), record.data())
    }

    /// The same fold, for the writer that has the payload in hand and
    /// not yet a record. Both entry points read the payload with one
    /// reader, so what the writer believes and what a rebuild produces
    /// cannot drift.
    ///
    /// # Errors
    /// As [`EndpointBook::apply`].
    pub fn apply_payload(&mut self, kind: EventKind, data: &Payload) -> Result<(), AxError> {
        match kind {
            EventKind::EndpointAttached => {
                let attached = read_attached(data)?;
                self.endpoints.insert(attached.name.clone(), attached);
                Ok(())
            }
            EventKind::EndpointLost => {
                let name = text(data, "name")?;
                self.endpoints.remove(&name);
                self.chosen.retain(|_, choice| choice.endpoint != name);
                Ok(())
            }
            EventKind::ModelSelected => {
                let (tag, choice) = read_choice(data)?;
                self.chosen.insert(tag, choice);
                Ok(())
            }
            _ => Ok(()),
        }
    }

    /// The model for one tag under one building's policy.
    ///
    /// # Errors
    /// Refuses when nothing is chosen for the tag, when the endpoint
    /// behind the choice is gone, and when a confidential building's
    /// choice would leave this machine.
    pub fn select(&self, tag: ModelTag, policy: &BuildingPolicy) -> Result<Chosen<'_>, AxError> {
        let choice = self.chosen.get(&tag).ok_or_else(|| {
            AxError::failure(
                AxCode::ConfigInvalid,
                format!("choose the {tag} model"),
                "no model is chosen for this tag",
            )
            .with_recovery("attach a provider on the settings page and pick a model for this tag")
        })?;
        let endpoint = self.endpoints.get(&choice.endpoint).ok_or_else(|| {
            AxError::failure(
                AxCode::ConfigInvalid,
                format!("choose the {tag} model"),
                format!("the endpoint {} is no longer attached", choice.endpoint),
            )
            .with_recovery("pick a model from an attached endpoint")
        })?;
        if policy.confidential && !endpoint.is_local() {
            return Err(AxError::failure(
                AxCode::GateDenied,
                format!("choose the {tag} model"),
                format!(
                    "{} is confidential and {} is not on this machine",
                    "this building", endpoint.base_url
                ),
            )
            .with_recovery("attach a loopback inference server and pick a model from it"));
        }
        Ok(Chosen {
            endpoint,
            entry: &choice.entry,
        })
    }

    pub fn endpoints(&self) -> impl Iterator<Item = &AttachedEndpoint> {
        self.endpoints.values()
    }

    /// What is chosen, tag by tag.
    pub fn choices(&self) -> impl Iterator<Item = (ModelTag, &str, &ModelEntry)> {
        self.chosen
            .iter()
            .map(|(tag, choice)| (*tag, choice.endpoint.as_str(), &choice.entry))
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.endpoints.is_empty() && self.chosen.is_empty()
    }
}

/// The one place an `endpoint_attached` payload is formed, so the reader
/// below and every writer agree by construction.
///
/// # Errors
/// Propagates payload construction failure.
pub fn attached_payload(endpoint: &AttachedEndpoint) -> Result<Payload, AxError> {
    let mut map = Map::new();
    map.insert("name".to_owned(), Value::String(endpoint.name.clone()));
    map.insert(
        "base_url".to_owned(),
        Value::String(endpoint.base_url.clone()),
    );
    map.insert(
        "dialect".to_owned(),
        serde_json::to_value(endpoint.dialect).map_err(|err| {
            AxError::failure(AxCode::InvalidArgs, "encode dialect", err.to_string())
        })?,
    );
    // The reference, never the credential: this is the byte that makes
    // "plaintext only enters the vault" true of the ledger as well.
    if let Some(reference) = auth_reference(&endpoint.auth) {
        map.insert("auth".to_owned(), Value::String(reference.to_string()));
        if let AuthSpec::Header { name, .. } = &endpoint.auth {
            map.insert("auth_header".to_owned(), Value::String(name.clone()));
        }
    }
    map.insert(
        "models".to_owned(),
        Value::Array(
            endpoint
                .models
                .iter()
                .map(|id| Value::String(id.clone()))
                .collect(),
        ),
    );
    Payload::new(map)
}

fn auth_reference(auth: &AuthSpec) -> Option<&SecretRef> {
    match auth {
        AuthSpec::Bearer(reference) => Some(reference),
        AuthSpec::Header { value, .. } => Some(value),
        _ => None,
    }
}

/// The one place a `model_selected` payload is formed.
///
/// # Errors
/// Propagates payload construction failure.
pub fn selected_payload(
    tag: ModelTag,
    endpoint: &str,
    entry: &ModelEntry,
) -> Result<Payload, AxError> {
    let mut map = Map::new();
    map.insert("tag".to_owned(), Value::String(tag.as_str().to_owned()));
    map.insert("endpoint".to_owned(), Value::String(endpoint.to_owned()));
    map.insert("model".to_owned(), Value::String(entry.id.clone()));
    map.insert(
        "context_tokens".to_owned(),
        Value::Number(entry.context_tokens.into()),
    );
    map.insert(
        "max_output_tokens".to_owned(),
        Value::Number(entry.max_output_tokens.into()),
    );
    for (key, price) in [
        ("input_price", entry.input_price),
        ("output_price", entry.output_price),
        ("cache_read_price", entry.cache_read_price),
        ("cache_write_price", entry.cache_write_price),
    ] {
        map.insert(key.to_owned(), Value::Number(price.get().into()));
    }
    Payload::new(map)
}

fn invalid(subject: impl Into<String>) -> AxError {
    AxError::failure(AxCode::WireMismatch, "read an endpoint record", subject)
        .with_recovery("replay with the build that wrote this record")
}

fn text(payload: &Payload, key: &str) -> Result<String, AxError> {
    payload
        .as_map()
        .get(key)
        .and_then(Value::as_str)
        .map(str::to_owned)
        .ok_or_else(|| invalid(format!("{key} is missing or not a string")))
}

fn count(payload: &Payload, key: &str) -> Result<u64, AxError> {
    payload
        .as_map()
        .get(key)
        .and_then(Value::as_u64)
        .ok_or_else(|| invalid(format!("{key} is missing or not a count")))
}

fn read_attached(payload: &Payload) -> Result<AttachedEndpoint, AxError> {
    let dialect_value = payload
        .as_map()
        .get("dialect")
        .ok_or_else(|| invalid("dialect is missing"))?;
    let dialect: DialectKind = serde_json::from_value(dialect_value.clone())
        .map_err(|err| invalid(format!("dialect: {err}")))?;
    let auth = match payload.as_map().get("auth").and_then(Value::as_str) {
        None => AuthSpec::None,
        Some(raw) => {
            let reference = SecretRef::parse(raw)?;
            match payload.as_map().get("auth_header").and_then(Value::as_str) {
                Some(header) => AuthSpec::Header {
                    name: header.to_owned(),
                    value: reference,
                },
                None => AuthSpec::Bearer(reference),
            }
        }
    };
    let models = payload
        .as_map()
        .get("models")
        .and_then(Value::as_array)
        .ok_or_else(|| invalid("models is missing or not an array"))?
        .iter()
        .map(|value| {
            value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| invalid("a model id is not a string"))
        })
        .collect::<Result<Vec<String>, AxError>>()?;
    Ok(AttachedEndpoint {
        name: text(payload, "name")?,
        base_url: text(payload, "base_url")?,
        dialect,
        auth,
        models,
    })
}

fn read_choice(payload: &Payload) -> Result<(ModelTag, Choice), AxError> {
    let raw = text(payload, "tag")?;
    let tag = ModelTag::ALL
        .into_iter()
        .find(|candidate| candidate.as_str() == raw)
        .ok_or_else(|| invalid(format!("{raw} is not a tag this build knows")))?;
    let entry = ModelEntry {
        id: text(payload, "model")?,
        context_tokens: count(payload, "context_tokens")?,
        max_output_tokens: count(payload, "max_output_tokens")?,
        input_price: UsdMicros::new(count(payload, "input_price")?),
        output_price: UsdMicros::new(count(payload, "output_price")?),
        cache_read_price: UsdMicros::new(count(payload, "cache_read_price")?),
        cache_write_price: UsdMicros::new(count(payload, "cache_write_price")?),
    };
    Ok((
        tag,
        Choice {
            endpoint: text(payload, "endpoint")?,
            entry,
        },
    ))
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
    use kernel::{EventDraft, GENESIS_PREV, RunId, Seq, TimeMs};

    fn record(kind: EventKind, data: Payload) -> EventRecord {
        EventRecord::from_draft(
            EventDraft {
                run: RunId::CITY,
                t: TimeMs::new(1),
                who: "owner".to_owned(),
                addr: None,
                kind,
                data,
                ig: false,
            },
            Seq::FIRST,
            GENESIS_PREV,
        )
    }

    fn attached(name: &str, base_url: &str) -> AttachedEndpoint {
        AttachedEndpoint {
            name: name.to_owned(),
            base_url: base_url.to_owned(),
            dialect: DialectKind::OpenAi,
            auth: AuthSpec::Bearer(SecretRef::parse("secret:provider/key").unwrap()),
            models: vec!["m-small".to_owned(), "m-large".to_owned()],
        }
    }

    fn entry(id: &str) -> ModelEntry {
        ModelEntry {
            id: id.to_owned(),
            context_tokens: 128_000,
            max_output_tokens: 8_192,
            input_price: UsdMicros::new(1_000_000),
            output_price: UsdMicros::new(2_000_000),
            cache_read_price: UsdMicros::new(0),
            cache_write_price: UsdMicros::new(0),
        }
    }

    fn book_with(base_url: &str) -> EndpointBook {
        let mut book = EndpointBook::new();
        let endpoint = attached("house", base_url);
        book.apply(&record(
            EventKind::EndpointAttached,
            attached_payload(&endpoint).unwrap(),
        ))
        .unwrap();
        book.apply(&record(
            EventKind::ModelSelected,
            selected_payload(ModelTag::Main, "house", &entry("m-large")).unwrap(),
        ))
        .unwrap();
        book
    }

    #[test]
    fn an_attachment_survives_the_payload_round_trip() {
        let endpoint = attached("house", "https://api.example.test/v1");
        let mut book = EndpointBook::new();
        book.apply(&record(
            EventKind::EndpointAttached,
            attached_payload(&endpoint).unwrap(),
        ))
        .unwrap();
        let held = book.endpoints().next().unwrap();
        assert_eq!(held.name, endpoint.name);
        assert_eq!(held.base_url, endpoint.base_url);
        assert_eq!(held.models, endpoint.models);
    }

    #[test]
    fn the_payload_carries_a_reference_and_never_a_credential() {
        let endpoint = attached("house", "https://api.example.test/v1");
        let payload = attached_payload(&endpoint).unwrap();
        let text = serde_json::to_string(&payload).unwrap();
        assert!(text.contains("secret:provider/key"));
        assert!(
            !text.contains("Bearer"),
            "the ledger records where a credential lives, not what it is"
        );
    }

    #[test]
    fn a_tag_with_no_choice_says_what_to_do_about_it() {
        let err = EndpointBook::new()
            .select(ModelTag::Digest, &BuildingPolicy::default())
            .unwrap_err();
        assert_eq!(*err.code(), AxCode::ConfigInvalid);
        assert!(err.recovery().contains("settings page"));
    }

    #[test]
    fn a_confidential_building_cannot_choose_a_model_off_this_machine() {
        let remote = book_with("https://api.example.test/v1");
        assert!(
            remote
                .select(ModelTag::Main, &BuildingPolicy::default())
                .is_ok()
        );
        let err = remote
            .select(ModelTag::Main, &BuildingPolicy::new(true))
            .unwrap_err();
        assert_eq!(*err.code(), AxCode::GateDenied);

        let local = book_with("http://127.0.0.1:11434/v1");
        assert!(
            local
                .select(ModelTag::Main, &BuildingPolicy::new(true))
                .is_ok(),
            "a loopback endpoint is what a confidential building is allowed to reach"
        );
    }

    #[test]
    fn losing_an_endpoint_takes_its_choices_with_it() {
        let mut book = book_with("https://api.example.test/v1");
        let mut gone = Map::new();
        gone.insert("name".to_owned(), Value::String("house".to_owned()));
        book.apply(&record(
            EventKind::EndpointLost,
            Payload::new(gone).unwrap(),
        ))
        .unwrap();
        assert!(book.is_empty());
        assert!(
            book.select(ModelTag::Main, &BuildingPolicy::default())
                .is_err()
        );
    }

    #[test]
    fn the_dialect_owns_the_path_the_person_did_not_type() {
        let openai = attached("house", "https://api.example.test/v1/");
        assert_eq!(
            openai.chat_url(),
            "https://api.example.test/v1/chat/completions"
        );
        assert_eq!(openai.models_url(), "https://api.example.test/v1/models");
        let mut anthropic = attached("house", "https://api.example.test/v1");
        anthropic.dialect = DialectKind::Anthropic;
        assert_eq!(anthropic.chat_url(), "https://api.example.test/v1/messages");
    }

    #[test]
    fn a_record_this_book_does_not_own_passes_through() {
        let mut book = EndpointBook::new();
        book.apply(&record(EventKind::RunStarted, Payload::empty()))
            .unwrap();
        assert!(book.is_empty());
    }

    #[test]
    fn an_unreadable_registration_is_an_error_not_a_skip() {
        let mut book = EndpointBook::new();
        let mut half = Map::new();
        half.insert("name".to_owned(), Value::String("house".to_owned()));
        let err = book
            .apply(&record(
                EventKind::EndpointAttached,
                Payload::new(half).unwrap(),
            ))
            .unwrap_err();
        assert_eq!(*err.code(), AxCode::WireMismatch);
    }
}
