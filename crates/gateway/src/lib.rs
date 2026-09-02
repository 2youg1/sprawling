// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! gateway — duty routing, dialects, credentials and cost for model calls.
//! Adapters implement the `kernel::model` seam; the dialect face is pure
//! translation (deliberately not a trait).

mod admission;
mod anthropic;
mod cost;
mod credential;
mod dialect;
mod endpoint;
mod market;
mod mismatch;
mod native;
mod oauth_profiles;
mod openai;
mod router;

pub use admission::{AdmissionState, AdmissionVerdict, ProviderOutcome};
pub use cost::{CallCost, CostSource, settle};
pub use credential::oauth_refresh;
pub use credential::{Captured, Custodian, Described, EnvReader, Persistence};
pub use credential::{OauthPending, OauthTokens, TokenRequest, oauth_begin};
pub use credential::{oauth_random, oauth_redeem, oauth_redeem_request};
pub use dialect::{increment_of, request_wire, response_from_wire};
pub use dialect::{response_wire, settled_from_stream};
pub use endpoint::{AuthSpec, Endpoint, EndpointConfig, SecretResolver};
pub use market::{MarketSnapshot, ModelEntry};
pub use native::{Native, NativeConfig};
pub use oauth_profiles::{OAUTH_PROFILES, OauthProfile, profile};
pub use router::{AttachedEndpoint, Chosen, EndpointBook, attached_payload, selected_payload};
