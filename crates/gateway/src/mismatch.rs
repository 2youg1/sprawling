// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Reading a value out of a provider's JSON, and the one word for what
//! it means when the shape is not the one this build asked for.
//!
//! Both dialects need the same four readers and fail the same way, so
//! the readers and the failure live together and each dialect module
//! depends on this one rather than on the other. Every accessor takes
//! the path it is reading, because "expected string" without a path
//! sends a person to read a whole response body to find the field.
//!
//! `E_WIRE_MISMATCH` is a loadtime code: what it means is that the
//! endpoint's declared dialect does not match what answered it, so the
//! recovery says exactly that rather than "try again".

use kernel::{AxCode, AxError, Effort, Payload, Tokens};
use serde_json::Value;

pub(crate) fn mismatch(path: &str, detail: &str) -> AxError {
    AxError::failure(
        AxCode::WireMismatch,
        "translate wire",
        format!("{path}: {detail}"),
    )
    .with_recovery(
        "the provider answered in a shape this dialect does not know; check that the \
         endpoint's dialect matches the provider and that its base url is the one its \
         documentation prints",
    )
}

pub(crate) fn stream_cut(detail: &str) -> AxError {
    AxError::failure(
        AxCode::Provider,
        "read a streamed answer",
        detail.to_owned(),
    )
    .with_recovery("dispatch again; the reply that arrived was not a whole one")
    .retriable()
}

/// One field of a JSON object, or a mismatch naming the path it is
/// missing from.
pub(crate) fn require<'v>(wire: &'v Value, path: &str, key: &str) -> Result<&'v Value, AxError> {
    wire.get(key)
        .ok_or_else(|| mismatch(&format!("{path}.{key}"), "missing"))
}

pub(crate) fn as_str<'v>(value: &'v Value, path: &str) -> Result<&'v str, AxError> {
    value
        .as_str()
        .ok_or_else(|| mismatch(path, "expected string"))
}

fn as_u64(value: &Value, path: &str) -> Result<u64, AxError> {
    value
        .as_u64()
        .ok_or_else(|| mismatch(path, "expected unsigned integer"))
}

pub(crate) fn tokens_or_zero(usage: &Value, key: &str, path: &str) -> Result<Tokens, AxError> {
    match usage.get(key) {
        None | Some(Value::Null) => Ok(Tokens::new(0)),
        Some(value) => Ok(Tokens::new(as_u64(value, &format!("{path}.{key}"))?)),
    }
}

pub(crate) fn payload_from(value: &Value, path: &str) -> Result<Payload, AxError> {
    if kernel::value_has_float(value) {
        return Err(mismatch(path, "float payloads are banned city-wide"));
    }
    serde_json::from_value(value.clone()).map_err(|err| mismatch(path, &err.to_string()))
}

/// A level added to the canonical ladder that this module has not been
/// taught to write. Fail closed rather than send a neighbouring level:
/// "I asked for max" must never silently become "I got high".
pub(crate) fn unspelled_effort(effort: Effort, dialect: &str) -> AxError {
    AxError::failure(
        AxCode::ConfigInvalid,
        format!("put an effort level on the {dialect} wire"),
        format!("{effort:?} has no spelling in this dialect"),
    )
    .with_recovery("teach this dialect the level, or pick one it already writes")
}
