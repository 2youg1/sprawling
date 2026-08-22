// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Subscription-login intelligence: data only, zero
//! branches. The flow code lives in `credential`; this table tracks each
//! provider's endpoints, scopes and client ids — sourced from actively
//! maintained open harnesses, refreshed as a weekly task, never a CI
//! gate ("is this endpoint stale" is not machine-decidable).
//!
//! Empty fields are deliberate: better absent than wrong; `oauth_begin`
//! fails closed on them.

#[derive(Debug, Clone, Copy)]
pub struct OauthProfile {
    pub provider: &'static str,
    /// Where this provider's API lives, so a finished login can attach
    /// an endpoint without a person retyping a URL they never chose.
    /// Empty means the same as an empty endpoint: fail closed.
    pub api_base: &'static str,
    pub auth_endpoint: &'static str,
    pub token_endpoint: &'static str,
    pub scopes: &'static [&'static str],
    pub client_id: &'static str,
    pub redirect_uri: &'static str,
    pub headers: &'static [(&'static str, &'static str)],
}

/// The table. Anthropic values are the publicly documented ones used by
/// open-source harnesses (2026-08 recheck); OpenAI's subscription flow
/// intelligence is pending and stays empty (fail-closed).
pub const OAUTH_PROFILES: [OauthProfile; 2] = [
    OauthProfile {
        provider: "anthropic",
        api_base: "https://api.anthropic.com",
        auth_endpoint: "https://claude.ai/oauth/authorize",
        token_endpoint: "https://console.anthropic.com/v1/oauth/token",
        scopes: &["org:create_api_key", "user:profile", "user:inference"],
        client_id: "9d1c250a-e61b-44d9-88ed-5944d1962f5e",
        redirect_uri: "https://console.anthropic.com/oauth/code/callback",
        headers: &[],
    },
    OauthProfile {
        provider: "openai",
        api_base: "",
        auth_endpoint: "",
        token_endpoint: "",
        scopes: &[],
        client_id: "",
        redirect_uri: "",
        headers: &[],
    },
];

pub fn profile(provider: &str) -> Option<&'static OauthProfile> {
    OAUTH_PROFILES.iter().find(|p| p.provider == provider)
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
    fn the_table_is_data_and_lookup_finds_rows() {
        assert!(profile("anthropic").is_some());
        assert!(profile("openai").is_some());
        assert!(profile("nonexistent").is_none());
        // Zero branches: nothing here computes; emptiness is a value.
        assert!(profile("openai").unwrap().auth_endpoint.is_empty());
    }
}
