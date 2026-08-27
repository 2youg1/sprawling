// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! Local inference always goes through here. The S3
//! shape: a fixed client for an OpenAI-compatible server on loopback.
//! Not a pass-through: the policy this type owns is exactly what
//! `Endpoint` refuses to own — loopback-only by construction, no
//! credential, dialect pinned. The egress surface does not exist in the
//! type, so confidential buildings can trust it structurally.

use kernel::{AxCode, AxError, DialectKind, Model, ModelRequest, ModelReturn, SecretRef};

use crate::endpoint::{AuthSpec, Endpoint, EndpointConfig};
use crate::market::ModelEntry;

#[derive(Debug, Clone)]
pub struct NativeConfig {
    /// Full chat endpoint URL; the host must be loopback.
    pub base_url: String,
    pub model: String,
    pub timeout_ms: u64,
    /// Local pricing is normally zero; a metered local pool may price it.
    pub pricing: Option<ModelEntry>,
}

pub struct Native {
    inner: Endpoint,
}

pub(crate) fn is_loopback(url: &str) -> bool {
    let Some(rest) = url.split("://").nth(1) else {
        return false;
    };
    let host_port = rest.split('/').next().unwrap_or("");
    let host = host_port
        .strip_prefix('[')
        .and_then(|h| h.split(']').next())
        .unwrap_or_else(|| host_port.split(':').next().unwrap_or(""));
    matches!(host, "localhost" | "127.0.0.1" | "::1")
}

impl Native {
    /// Fail-closed: a non-loopback URL is a config error, not a warning.
    pub fn new(config: NativeConfig) -> Result<Native, AxError> {
        if !is_loopback(&config.base_url) {
            return Err(AxError::failure(
                AxCode::ConfigInvalid,
                "configure native model",
                format!("{} is not a loopback address", config.base_url),
            )
            .with_recovery("native serves local inference only; use an endpoint for remote"));
        }
        let inner = Endpoint::new(
            EndpointConfig {
                base_url: config.base_url,
                dialect: DialectKind::OpenAi,
                model: config.model,
                auth: AuthSpec::None,
                extra_headers: Vec::new(),
                overrides: Vec::new(),
                timeout_ms: config.timeout_ms,
                pricing: config.pricing,
            },
            Box::new(|_reference: &SecretRef| {
                Err(AxError::failure(
                    AxCode::CredentialMissing,
                    "resolve credential",
                    "native never authenticates",
                ))
            }),
        )?;
        Ok(Native { inner })
    }
}

impl Model for Native {
    fn call(&mut self, req: &ModelRequest) -> Result<ModelReturn, AxError> {
        self.inner.call(req)
    }
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "test code"
)]
mod tests {
    use super::*;

    #[test]
    fn non_loopback_urls_cannot_be_spelled() {
        for bad in [
            "https://api.example.com/v1/chat/completions",
            "http://192.168.1.5:8080/v1/chat/completions",
            "not a url",
        ] {
            let err = match Native::new(NativeConfig {
                base_url: bad.to_owned(),
                model: "local".to_owned(),
                timeout_ms: 1_000,
                pricing: None,
            }) {
                Err(err) => err,
                Ok(_) => panic!("{bad} must be refused"),
            };
            assert_eq!(*err.code(), AxCode::ConfigInvalid, "{bad}");
        }
    }

    #[test]
    fn loopback_spellings_construct() {
        for good in [
            "http://127.0.0.1:8080/v1/chat/completions",
            "http://localhost:11434/v1/chat/completions",
            "http://[::1]:8080/v1/chat/completions",
        ] {
            assert!(
                Native::new(NativeConfig {
                    base_url: good.to_owned(),
                    model: "local".to_owned(),
                    timeout_ms: 1_000,
                    pricing: None,
                })
                .is_ok(),
                "{good}"
            );
        }
    }
}
