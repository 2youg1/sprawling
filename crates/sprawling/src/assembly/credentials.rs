// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! What this city can sign in as, and what it may call.

use std::sync::Arc;

use kernel::{AxCode, AxError, EventKind};
use kernel::{Model, Payload};

use crate::serving::random_token;

use super::{RunWorker, now_ms};

/// The name the environment-configured endpoint is attached under, so a
/// person reading the settings page can see where it came from.
pub(super) const ENVIRONMENT_ENDPOINT: &str = "environment";

/// What a person entered to reach a provider.
///
/// Five values that are read together and never chosen independently:
/// probing an endpoint, attaching it and describing it to the ledger are
/// three readings of one form, and passing them side by side gave three
/// chances for the probe and the attachment to disagree about what they
/// were talking to.
pub(super) struct Entered {
    pub(super) name: String,
    pub(super) base_url: String,
    pub(super) dialect: kernel::DialectKind,
    /// A `secret:realm/name` reference, never plaintext.
    pub(super) secret: Option<String>,
    /// The header the credential travels in, when the provider wants one
    /// that is not `Authorization: Bearer`.
    pub(super) auth_header: Option<String>,
}

/// Which model, at which endpoint, for which of the city's roles.
pub(super) struct Chosen {
    pub(super) endpoint: String,
    pub(super) model: String,
    pub(super) tag: kernel::ModelTag,
}

/// The two ceilings a model row states.
///
/// Zero in either field means "take the catalogue's figure": a person
/// choosing a model on a form has no business typing a context window,
/// and the pair travels together because a context window without an
/// output ceiling describes no model that can be called.
pub(super) struct Ceilings {
    pub(super) context_tokens: u64,
    pub(super) max_output_tokens: u64,
}

/// How long a probe may take. Short: a person is watching the settings
/// page, and an endpoint that cannot answer in this time is one they
/// want to hear about rather than wait for.
pub(super) const PROBE_TIMEOUT_MS: u64 = 15_000;

/// How long one model call may take.
pub(super) const CALL_TIMEOUT_MS: u64 = 120_000;

/// The headers a dialect requires beyond the credential.
pub(super) fn dialect_headers(dialect: kernel::DialectKind) -> Vec<(String, String)> {
    match dialect {
        kernel::DialectKind::Anthropic => {
            vec![("anthropic-version".to_owned(), ANTHROPIC_VERSION.to_owned())]
        }
        _ => Vec::new(),
    }
}

/// The Messages API version this build speaks. Pinned rather than
/// omitted: the provider treats a missing version as an error, and a
/// floating one would change the wire under a replay.
/// <https://platform.claude.com/docs/en/api/messages>
pub(super) const ANTHROPIC_VERSION: &str = "2023-06-01";

pub(super) fn poisoned_vault() -> AxError {
    AxError::failure(
        AxCode::StorageFatal,
        "reach the vault",
        "the vault lock is poisoned",
    )
    .with_recovery("restart the server; enrolled credentials are unaffected")
}

pub(super) fn dialect_of(provider: &str) -> Result<kernel::DialectKind, AxError> {
    match provider {
        "anthropic" => Ok(kernel::DialectKind::Anthropic),
        "openai" => Ok(kernel::DialectKind::OpenAi),
        other => Err(AxError::failure(
            AxCode::ConfigInvalid,
            "choose a dialect for a provider",
            other.to_owned(),
        )
        .with_recovery("attach this provider by hand and state its dialect")),
    }
}

/// The catalog's `local` row under the name a local server serves it.
pub(super) fn local_model_facts(model: &str) -> Result<gateway::ModelEntry, AxError> {
    let market = gateway::MarketSnapshot::builtin();
    let local = market.lookup("local").ok_or_else(|| {
        AxError::failure(
            AxCode::ConfigInvalid,
            "read the model catalog",
            "the pinned catalog has no local row",
        )
    })?;
    Ok(gateway::ModelEntry {
        id: model.to_owned(),
        ..local.clone()
    })
}

impl RunWorker {
    /// Renews a subscription credential that is about to stop working.
    ///
    /// Called before the credential is used rather than after a call
    /// fails: a 401 costs a whole turn to discover, and the expiry the
    /// provider stated is a fact this city already wrote down. A
    /// provider with no recorded expiry is left alone - not knowing when
    /// something expires is not a reason to renew it every time.
    ///
    /// # Errors
    /// Propagates the token endpoint's refusal. A refused refresh means
    /// the login is over, and saying so beats retrying what will fail
    /// again.
    pub(super) fn renew_if_stale(&mut self, provider: &str) -> Result<(), AxError> {
        let Some(expires_at) = self.expiries.get(provider).copied() else {
            return Ok(());
        };
        // A minute of margin: a call started now must still be holding a
        // working credential when it reaches the far end.
        if now_ms()?.value().saturating_add(60_000) < expires_at {
            return Ok(());
        }
        let Some(profile) = gateway::profile(provider) else {
            return Ok(());
        };
        let stored = kernel::SecretRef::parse(&format!("secret:{provider}/oauth-refresh"))?;
        let refresh = {
            let vault = self.vault.lock().map_err(|_| poisoned_vault())?;
            vault.resolve(&stored)?
        };
        let tokens = gateway::oauth_refresh(profile, &refresh, PROBE_TIMEOUT_MS)?;
        let access = kernel::SecretRef::parse(&format!("secret:{provider}/oauth"))?;
        {
            let mut vault = self.vault.lock().map_err(|_| poisoned_vault())?;
            vault.set(&access, tokens.access)?;
            if let Some(next) = tokens.refresh {
                vault.set(&stored, next)?;
            }
        }
        let mut map = serde_json::Map::new();
        map.insert(
            "ref".to_owned(),
            serde_json::Value::String(access.to_string()),
        );
        map.insert(
            "origin".to_owned(),
            serde_json::Value::String(format!("{provider}-renewal")),
        );
        if let Some(seconds) = tokens.expires_in_s {
            let at = now_ms()?
                .value()
                .saturating_add(seconds.saturating_mul(1_000));
            map.insert(
                "expires_at".to_owned(),
                serde_json::Value::Number(at.into()),
            );
            self.expiries.insert(provider.to_owned(), at);
        }
        self.record(EventKind::SecretCaptured, Payload::new(map)?)
    }

    /// One step of a subscription login.
    ///
    /// Two steps rather than one because a person stands between them:
    /// the provider shows them a code after they approve, and they bring
    /// it back. Nothing listens on a port for it — the profile's own
    /// redirect is the provider's page, so a listener would be a second
    /// way in that nobody uses.
    pub(super) fn login(
        &mut self,
        provider: &str,
        step: channels::LoginStep,
    ) -> Result<(), AxError> {
        let profile = *gateway::profile(provider).ok_or_else(|| {
            AxError::failure(
                AxCode::ConfigInvalid,
                "begin a subscription login",
                provider.to_owned(),
            )
            .with_recovery(
                "this build knows the subscription flow of: anthropic; \
                 other providers attach with an API key",
            )
        })?;
        self.login_with(&profile, provider, step)
    }

    /// The same login against a profile the caller supplies. The lookup
    /// is the only thing this does not do, which is what lets a test
    /// point the flow at a server it controls without the production
    /// path growing an override nobody in production would set.
    pub(super) fn login_with(
        &mut self,
        profile: &gateway::OauthProfile,
        provider: &str,
        step: channels::LoginStep,
    ) -> Result<(), AxError> {
        match step {
            channels::LoginStep::Begin => {
                // Two independent draws. The verifier proves the client
                // that redeems is the client that asked; the state
                // proves the redirect answers this request. One value
                // doing both jobs proves neither, and `oauth_begin`
                // refuses it.
                let pending = gateway::oauth_begin(profile, random_token(48)?, random_token(24)?)?;
                let mut map = serde_json::Map::new();
                map.insert(
                    "provider".to_owned(),
                    serde_json::Value::String(provider.to_owned()),
                );
                // The URL carries a PKCE challenge and a state, both of
                // which are public by design; no credential exists yet.
                map.insert(
                    "auth_url".to_owned(),
                    serde_json::Value::String(pending.auth_url.clone()),
                );
                self.logins.insert(provider.to_owned(), pending);
                self.record(EventKind::LoginStarted, Payload::new(map)?)
            }
            channels::LoginStep::Code { code } => {
                let pending = self.logins.remove(provider).ok_or_else(|| {
                    AxError::failure(
                        AxCode::CredentialMissing,
                        "redeem an authorization code",
                        provider.to_owned(),
                    )
                    .with_recovery(
                        "start the login first; the code answers a request this process made",
                    )
                })?;
                let tokens = gateway::oauth_redeem(profile, &pending, &code, PROBE_TIMEOUT_MS)?;
                let access = kernel::SecretRef::parse(&format!("secret:{provider}/oauth"))?;
                {
                    let mut vault = self.vault.lock().map_err(|_| poisoned_vault())?;
                    vault.set(&access, tokens.access)?;
                    if let Some(refresh) = tokens.refresh {
                        let reference =
                            kernel::SecretRef::parse(&format!("secret:{provider}/oauth-refresh"))?;
                        vault.set(&reference, refresh)?;
                    }
                }
                let mut map = serde_json::Map::new();
                map.insert(
                    "ref".to_owned(),
                    serde_json::Value::String(access.to_string()),
                );
                map.insert(
                    "origin".to_owned(),
                    serde_json::Value::String(format!("{provider}-subscription")),
                );
                // When it stops working, in the city's own clock. Not a
                // secret, and the one fact that decides whether the next
                // call must renew first.
                if let Some(seconds) = tokens.expires_in_s {
                    let at = now_ms()?
                        .value()
                        .saturating_add(seconds.saturating_mul(1_000));
                    map.insert(
                        "expires_at".to_owned(),
                        serde_json::Value::Number(at.into()),
                    );
                    self.expiries.insert(provider.to_owned(), at);
                }
                self.record(EventKind::SecretCaptured, Payload::new(map)?)?;
                if profile.api_base.is_empty() {
                    return Err(AxError::failure(
                        AxCode::ConfigInvalid,
                        "attach the endpoint this login is for",
                        provider.to_owned(),
                    )
                    .with_recovery(
                        "the token is in the vault; attach the endpoint by hand until this \
                         provider's api base is known",
                    ));
                }
                // The person logged in to use it, so the endpoint they
                // logged into is attached here rather than left as a
                // second thing to remember.
                self.attach_endpoint(
                    Entered {
                        name: provider.to_owned(),
                        base_url: profile.api_base.to_owned(),
                        dialect: dialect_of(provider)?,
                        secret: Some(access.to_string()),
                        auth_header: None,
                    },
                    &[],
                )
            }
        }
    }

    /// Puts one credential in the vault. Nothing about it reaches the
    /// ledger but the fact that it happened.
    pub(super) fn put_secret(
        &mut self,
        realm: String,
        name: String,
        value: kernel::Sealed<String>,
    ) -> Result<(), AxError> {
        let reference = kernel::SecretRef::parse(&format!("secret:{realm}/{name}"))?;
        {
            let mut vault = self.vault.lock().map_err(|_| poisoned_vault())?;
            vault.set(&reference, value.into_vault_value())?;
        }
        let mut map = serde_json::Map::new();
        map.insert(
            "ref".to_owned(),
            serde_json::Value::String(reference.to_string()),
        );
        map.insert(
            "origin".to_owned(),
            serde_json::Value::String("enrolment".to_owned()),
        );
        self.record(EventKind::SecretCaptured, Payload::new(map)?)
    }

    /// The redemption closure the adapters take: one resolve per call,
    /// nothing cached, the lock held only while the vault is read.
    pub(super) fn resolver(&self) -> gateway::SecretResolver {
        let vault = Arc::clone(&self.vault);
        Box::new(move |reference: &kernel::SecretRef| {
            let held = vault.lock().map_err(|_| poisoned_vault())?;
            held.resolve(reference)
        })
    }

    /// Asks a base URL what it serves, and attaches nothing.
    ///
    /// The list is recorded rather than returned: a query would have to
    /// make this blocking call on the socket's own task, and the answer
    /// is a fact about what this city can reach - which is the kind of
    /// thing the ledger holds.
    pub(super) fn probe_endpoint(&mut self, entered: Entered) -> Result<(), AxError> {
        let endpoint = self.endpoint_of(entered)?;
        let models = self.probe(&endpoint)?;
        let mut map = serde_json::Map::new();
        map.insert(
            "name".to_owned(),
            serde_json::Value::String(endpoint.name.clone()),
        );
        map.insert(
            "base_url".to_owned(),
            serde_json::Value::String(endpoint.base_url.clone()),
        );
        map.insert(
            "models".to_owned(),
            serde_json::Value::Array(
                models
                    .iter()
                    .map(|id| serde_json::Value::String(id.clone()))
                    .collect(),
            ),
        );
        self.record(EventKind::EndpointProbed, Payload::new(map)?)
    }

    /// The endpoint a form describes, before anybody has asked it
    /// anything. One reading of the four fields, so a probe and the
    /// attachment that follows it cannot disagree about what they are
    /// talking to.
    fn endpoint_of(&self, entered: Entered) -> Result<gateway::AttachedEndpoint, AxError> {
        let Entered {
            name,
            base_url,
            dialect,
            secret,
            auth_header,
        } = entered;
        let auth = match secret {
            None => gateway::AuthSpec::None,
            Some(raw) => {
                let reference = kernel::SecretRef::parse(&raw)?;
                match auth_header {
                    Some(header) => gateway::AuthSpec::Header {
                        name: header,
                        value: reference,
                    },
                    None => gateway::AuthSpec::Bearer(reference),
                }
            }
        };
        Ok(gateway::AttachedEndpoint {
            name,
            base_url,
            dialect,
            auth,
            models: Vec::new(),
        })
    }

    /// Registers what the person entered, after asking the endpoint what
    /// it serves. The probe happens before the record: an endpoint that
    /// cannot be reached is not attached, so the book never advertises a
    /// model nobody can call.
    ///
    /// `admit` narrows what is registered to the models the person
    /// ticked. An empty list admits everything the endpoint serves,
    /// which is what somebody who never asked for the list meant; a name
    /// on the list that the endpoint does not serve is left out rather
    /// than promised, the same answer a reading room gives a skill that
    /// is not on the shelves.
    pub(super) fn attach_endpoint(
        &mut self,
        entered: Entered,
        admit: &[String],
    ) -> Result<(), AxError> {
        let mut endpoint = self.endpoint_of(entered)?;
        let served = self.probe(&endpoint)?;
        endpoint.models = if admit.is_empty() {
            served
        } else {
            served
                .into_iter()
                .filter(|id| admit.iter().any(|wanted| wanted == id))
                .collect()
        };
        self.note(
            runtime::diagnostics::Level::Effect,
            "gateway::router",
            &format!(
                "{} at {} serves {} model(s)",
                endpoint.name,
                endpoint.base_url,
                endpoint.models.len()
            ),
        );
        let payload = gateway::attached_payload(&endpoint)?;
        self.record(EventKind::EndpointAttached, payload)
    }

    fn probe(&self, endpoint: &gateway::AttachedEndpoint) -> Result<Vec<String>, AxError> {
        let probe = gateway::Endpoint::new(
            gateway::EndpointConfig {
                base_url: endpoint.chat_url(),
                dialect: endpoint.dialect,
                model: String::new(),
                auth: endpoint.auth.clone(),
                extra_headers: dialect_headers(endpoint.dialect),
                overrides: Vec::new(),
                timeout_ms: PROBE_TIMEOUT_MS,
                pricing: None,
            },
            self.resolver(),
        )?;
        probe.list_models(&endpoint.models_url())
    }

    /// Points one tag at one model. The two token counts come from the
    /// person because no provider's model list carries them, and a
    /// number invented here would outrank the one that bills.
    pub(super) fn select_model(
        &mut self,
        chosen: Chosen,
        ceilings: Ceilings,
    ) -> Result<(), AxError> {
        let Chosen {
            endpoint,
            model,
            tag,
        } = chosen;
        let Ceilings {
            context_tokens,
            max_output_tokens,
        } = ceilings;
        let known = self
            .book
            .endpoints()
            .find(|candidate| candidate.name == endpoint)
            .ok_or_else(|| {
                AxError::failure(
                    AxCode::ConfigInvalid,
                    "choose a model",
                    format!("{endpoint} is not attached"),
                )
                .with_recovery("attach the endpoint first, then choose one of the models it lists")
            })?;
        if !known.models.contains(&model) {
            return Err(AxError::failure(
                AxCode::ConfigInvalid,
                "choose a model",
                format!("{endpoint} does not serve {model}"),
            )
            .with_recovery("choose one of the models the endpoint listed")
            .with_nearby(known.models.clone()));
        }
        let priced = gateway::MarketSnapshot::builtin().lookup(&model).cloned();
        // Zero means "take the catalogue's figure". A person choosing a
        // model in the settings page has no business typing a context
        // window: the ceiling is a fact about the model, and a number
        // invented on a form would end runs for a reason that appears
        // nowhere in the account.
        let context_tokens = match context_tokens {
            0 => priced.as_ref().map_or(0, |row| row.context_tokens),
            stated => stated,
        };
        let max_output_tokens = match max_output_tokens {
            0 => priced.as_ref().map_or(0, |row| row.max_output_tokens),
            stated => stated,
        };
        let entry = gateway::ModelEntry {
            id: model,
            context_tokens,
            max_output_tokens,
            // Prices come from the pinned catalog when it knows the
            // model and are zero when it does not: an unpriced call is
            // reported as unpriced rather than as free-looking guesswork.
            input_price: priced
                .as_ref()
                .map(|row| row.input_price)
                .unwrap_or_default(),
            output_price: priced
                .as_ref()
                .map(|row| row.output_price)
                .unwrap_or_default(),
            cache_read_price: priced
                .as_ref()
                .map(|row| row.cache_read_price)
                .unwrap_or_default(),
            cache_write_price: priced.map(|row| row.cache_write_price).unwrap_or_default(),
        };
        let payload = gateway::selected_payload(tag, &endpoint, &entry)?;
        self.record(EventKind::ModelSelected, payload)
    }

    fn seed_from_environment(&mut self, base_url: &str, model: &str) -> Result<(), AxError> {
        let facts = local_model_facts(model)?;
        self.attach_endpoint(
            Entered {
                name: ENVIRONMENT_ENDPOINT.to_owned(),
                base_url: base_url.to_owned(),
                dialect: kernel::DialectKind::OpenAi,
                secret: None,
                auth_header: None,
            },
            &[],
        )?;
        self.select_model(
            Chosen {
                endpoint: ENVIRONMENT_ENDPOINT.to_owned(),
                model: model.to_owned(),
                tag: kernel::ModelTag::Main,
            },
            Ceilings {
                context_tokens: facts.context_tokens,
                max_output_tokens: facts.max_output_tokens,
            },
        )
    }

    /// The adapter for one chosen model.
    ///
    /// A loopback endpoint speaking the OpenAI shape goes through the
    /// local adapter, which is loopback-only by construction; everything
    /// else goes through the general one, which refuses to carry a
    /// confidential building's bytes off this machine.
    ///
    /// A credential decides the route too: the local adapter has no
    /// authentication surface, so a loopback endpoint that was attached
    /// with a secret (a local proxy, LiteLLM, a corporate gateway) must
    /// take the general path - before this condition existed, the probe
    /// carried the person's key and every real call silently dropped it.
    pub(super) fn adapter_for(
        &self,
        chosen: &gateway::Chosen<'_>,
    ) -> Result<Box<dyn Model + Send>, AxError> {
        let endpoint = chosen.endpoint;
        if endpoint.is_local()
            && matches!(endpoint.dialect, kernel::DialectKind::OpenAi)
            && matches!(endpoint.auth, gateway::AuthSpec::None)
        {
            let native = gateway::Native::new(gateway::NativeConfig {
                base_url: endpoint.chat_url(),
                model: chosen.entry.id.clone(),
                timeout_ms: CALL_TIMEOUT_MS,
                pricing: Some(chosen.entry.clone()),
            })?;
            return Ok(Box::new(native));
        }
        let endpoint = gateway::Endpoint::new(
            gateway::EndpointConfig {
                base_url: endpoint.chat_url(),
                dialect: endpoint.dialect,
                model: chosen.entry.id.clone(),
                auth: endpoint.auth.clone(),
                extra_headers: dialect_headers(endpoint.dialect),
                overrides: Vec::new(),
                timeout_ms: CALL_TIMEOUT_MS,
                pricing: Some(chosen.entry.clone()),
            },
            self.resolver(),
        )?;
        Ok(Box::new(endpoint))
    }

    /// Records what the vault turned out to be, and registers the local
    /// server named in the environment when nothing is registered yet.
    ///
    /// The environment path is a convenience, not a second authority:
    /// it writes the same two records the settings page writes, so the
    /// book stays the only statement of what this city can call. A
    /// failure here is reported and not fatal — a city stays readable
    /// without a provider.
    pub(crate) fn open_for_service(&mut self, vault_notice: Option<Payload>) {
        if let Some(notice) = vault_notice
            && let Err(err) = self.record(EventKind::ProviderDegraded, notice)
        {
            self.note(
                runtime::diagnostics::Level::Refuse,
                "bin::assembly",
                &format!("{err}; {}", err.recovery()),
            );
        }
        if !self.book.is_empty() {
            return;
        }
        let (Ok(base_url), Ok(model)) = (
            std::env::var("SPRAWLING_MODEL_URL"),
            std::env::var("SPRAWLING_MODEL"),
        ) else {
            return;
        };
        if let Err(err) = self.seed_from_environment(&base_url, &model) {
            self.note(
                runtime::diagnostics::Level::Refuse,
                "bin::assembly",
                &format!(
                    "the model named in the environment is not attached: {err}; {}",
                    err.recovery()
                ),
            );
        }
    }
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
    use crate::assembly::fixture::*;
    use crate::assembly::*;

    #[test]
    fn a_model_the_endpoint_never_listed_cannot_be_chosen() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        let (base_url, _provider) = fake_openai(&["m-small"], Vec::new());
        let Err(err) = worker_with_provider(dir.path(), &base_url, "m-invented") else {
            panic!("a model the endpoint never listed cannot be chosen");
        };
        assert_eq!(*err.code(), AxCode::ConfigInvalid);
        assert!(err.subject().contains("m-invented"));
    }

    #[test]
    fn an_enrolled_credential_leaves_only_a_reference_in_the_history() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let mut worker = RunWorker::new(
            dir.path(),
            gateway::Custodian::in_memory(),
            runtime::diagnostics::Diagnostics::off(),
        )
        .unwrap();
        // Assembled at runtime: a credential-shaped literal is what the
        // secret gate keeps out of the repository.
        let token = ["sk-live-", "9f2c4a7e1b8d"].concat();
        worker
            .handle(channels::Command::PutSecret {
                realm: "house".to_owned(),
                name: "key".to_owned(),
                value: kernel::Sealed::new(Box::new(token.clone())),
            })
            .unwrap();

        let verified = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
        let history: String = verified
            .raw_lines()
            .iter()
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .collect::<Vec<String>>()
            .join("\n");
        assert!(history.contains("secret:house/key"));
        assert!(
            !history.contains(&token),
            "the ledger records where a credential lives, never what it is"
        );
        // And it is redeemable afterwards, which is the other half: a
        // vault that records the act without keeping the value would
        // fail later, far from here.
        let resolver = worker.resolver();
        let reference = kernel::SecretRef::parse("secret:house/key").unwrap();
        let redeemed = resolver(&reference).unwrap().into_vault_value();
        assert_eq!(redeemed.as_str(), token);
    }

    /// A provider that answers the two requests a finished login makes:
    /// the token POST, then the model list the attach probes for.
    fn fake_oauth_provider() -> (String, std::thread::JoinHandle<Vec<String>>) {
        use std::io::{Read as _, Write as _};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let handle = std::thread::spawn(move || {
            let mut seen = Vec::new();
            let access = ["sk-ant-oat01-", "Qz7mK2p", "L9vB4nC5"].concat();
            let refresh = ["sk-ant-ort01-", "Rt3nD8q", "X2vC6mB1"].concat();
            let tokens = serde_json::json!({
                "access_token": access,
                "refresh_token": refresh,
                "expires_in": 3600,
            })
            .to_string();
            let models = serde_json::json!({ "data": [{ "id": "claude-sonnet-4-6" }] }).to_string();
            for _ in 0..2 {
                let Ok((mut stream, _)) = listener.accept() else {
                    break;
                };
                let mut buf = vec![0u8; 65536];
                let Ok(n) = stream.read(&mut buf) else { break };
                let request = String::from_utf8_lossy(&buf[..n]).into_owned();
                let body = if request.starts_with("POST") {
                    tokens.clone()
                } else {
                    models.clone()
                };
                seen.push(request);
                let head = format!(
                    "HTTP/1.1 200 X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body.as_bytes());
            }
            seen
        });
        (format!("http://{addr}"), handle)
    }

    #[test]
    fn a_subscription_login_ends_with_a_credential_in_the_vault_and_an_endpoint_attached() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let (base, server) = fake_oauth_provider();
        let profile = gateway::OauthProfile {
            provider: "anthropic",
            api_base: Box::leak(base.clone().into_boxed_str()),
            auth_endpoint: "https://example.invalid/oauth/authorize",
            token_endpoint: Box::leak(format!("{base}/v1/oauth/token").into_boxed_str()),
            scopes: &["user:inference"],
            client_id: "test-client",
            redirect_uri: "https://example.invalid/callback",
            headers: &[],
        };
        let mut worker = RunWorker::new(
            dir.path(),
            gateway::Custodian::in_memory(),
            runtime::diagnostics::Diagnostics::off(),
        )
        .unwrap();

        // A code with no request behind it proves nothing, and is
        // refused before any byte is sent.
        let err = worker
            .login_with(
                &profile,
                "anthropic",
                channels::LoginStep::Code {
                    code: "x".to_owned(),
                },
            )
            .unwrap_err();
        assert_eq!(err.code(), &AxCode::CredentialMissing);
        assert!(err.recovery().contains("start the login first"));

        worker
            .login_with(&profile, "anthropic", channels::LoginStep::Begin)
            .unwrap();
        worker
            .login_with(
                &profile,
                "anthropic",
                channels::LoginStep::Code {
                    code: "the-code".to_owned(),
                },
            )
            .unwrap();

        let verified = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
        let history: String = verified
            .raw_lines()
            .iter()
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .collect::<Vec<String>>()
            .join("\n");
        assert!(history.contains("login_started"));
        assert!(
            history.contains("code_challenge_method=S256"),
            "the url a person is asked to open is the one history recorded"
        );
        assert!(
            history.contains("secret:anthropic/oauth"),
            "the credential is a reference in history, never a value"
        );
        assert!(history.contains("endpoint_attached"));

        let sent = server.join().unwrap();
        assert!(
            !history.contains("sk-ant-oat01-"),
            "a token never reaches the ledger"
        );
        assert!(
            sent.iter().any(|request| request.contains("the-code")),
            "the code was redeemed against the provider"
        );
        assert!(
            sent.iter().any(|request| request
                .to_ascii_lowercase()
                .contains("authorization: bearer")),
            "and the attach carries the credential the login just earned"
        );
    }

    #[test]
    fn a_login_for_a_provider_this_build_has_no_flow_for_is_refused_by_name() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        let mut worker = RunWorker::new(
            dir.path(),
            gateway::Custodian::in_memory(),
            runtime::diagnostics::Diagnostics::off(),
        )
        .unwrap();
        let err = worker
            .handle(channels::Command::Login {
                provider: channels::ProviderName::parse("modelscope").unwrap(),
                step: channels::LoginStep::Begin,
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"login"),
            })
            .unwrap_err();
        assert_eq!(err.code(), &AxCode::ConfigInvalid);
        assert!(err.recovery().contains("API key"));

        // The one whose intelligence row is empty fails closed rather
        // than sending a person to an empty URL.
        let err = worker
            .handle(channels::Command::Login {
                provider: channels::ProviderName::parse("openai").unwrap(),
                step: channels::LoginStep::Begin,
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"login"),
            })
            .unwrap_err();
        assert_eq!(err.code(), &AxCode::ConfigInvalid);
        assert!(err.subject().contains("intelligence incomplete"));
    }

    #[test]
    fn a_dispatch_without_a_provider_fails_saying_what_to_configure() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        // Nothing registered: the refusal has to name the act that fixes
        // it, because a person who has not attached a provider yet is
        // exactly the person who does not know that is the missing step.
        let mut worker = RunWorker::new(
            dir.path(),
            gateway::Custodian::in_memory(),
            runtime::diagnostics::Diagnostics::off(),
        )
        .unwrap();
        let err = worker
            .handle(channels::Command::Dispatch {
                addr: Address::parse("lab/room1").unwrap(),
                task: "anything".to_owned(),
                goal: "anything".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"dispatch"),
                session: None,
                effort: None,
            })
            .unwrap_err();
        assert!(
            err.recovery().contains("settings page"),
            "got: {}",
            err.recovery()
        );
    }

    #[test]
    fn a_loopback_endpoint_with_a_credential_sends_it_on_every_call() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        let (base_url, provider) = fake_openai(&["m-key"], vec![completion("done", None)]);
        let mut worker = RunWorker::new(
            dir.path(),
            gateway::Custodian::in_memory(),
            runtime::diagnostics::Diagnostics::off(),
        )
        .unwrap();
        worker
            .handle(channels::Command::PutSecret {
                realm: "proxy".to_owned(),
                name: "key".to_owned(),
                value: kernel::Sealed::new(Box::new("sk-proxy-credential".to_owned())),
            })
            .unwrap();
        worker
            .handle(channels::Command::AttachEndpoint {
                name: channels::ProviderName::parse("proxied").unwrap(),
                base_url,
                dialect: kernel::DialectKind::OpenAi,
                secret: Some("secret:proxy/key".to_owned()),
                auth_header: None,
                admit: Vec::new(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"attach"),
            })
            .unwrap();
        worker
            .handle(channels::Command::SelectModel {
                endpoint: channels::ProviderName::parse("proxied").unwrap(),
                model: "m-key".to_owned(),
                tag: kernel::ModelTag::Main,
                context_tokens: 32_768,
                max_output_tokens: 4_096,
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"select"),
            })
            .unwrap();
        worker
            .handle(channels::Command::Dispatch {
                addr: Address::parse("lab/room1").unwrap(),
                task: "say done".to_owned(),
                goal: "auth on the wire".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"dispatch"),
                session: None,
                effort: None,
            })
            .unwrap();
        let chat = provider
            .exchanges()
            .into_iter()
            .find(|head| head.starts_with("POST"))
            .expect("the dispatch called the model");
        // Before the credential-aware route existed, a loopback endpoint
        // with a secret went through the local adapter and this header
        // was silently absent - the probe authenticated, the calls never.
        assert!(
            chat.to_ascii_lowercase()
                .contains("authorization: bearer sk-proxy-credential"),
            "the chat call must carry the credential; head was:\n{chat}"
        );
    }
}
