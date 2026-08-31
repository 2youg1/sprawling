// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Gate: no secret shape anywhere in the repository (C13). The judge is
//! `kernel::secret::scan` — one detector, one home; this gate only walks
//! and reports. Findings carry file + offset + length, never the bytes.
//! There is no inline waiver: a waiver comment would be a hole injected
//! content could drive through. Also enforces the `Sealed::expose` call
//! site whitelist ('s redemption points).

use std::path::Path;

use crate::report::{Violation, XtaskError};
use crate::walk;

/// The only files allowed to say `.expose(` under crates/*/src: the
/// defining module and the two redemption points (gateway lands S3).
const EXPOSE_WHITELIST: [&str; 5] = [
    "crates/kernel/src/secret.rs",
    "crates/gateway/src/endpoint.rs",
    "crates/gateway/src/native.rs",
    // R1.18: renewing a subscription credential sends the refresh token
    // to the provider's token endpoint, which is a redemption point of
    // exactly the same kind as the two above - the last slot before the
    // wire. Widened here rather than worked around at the call site,
    // because the alternative was the assembly holding plaintext, and
    // that is the thing this list exists to prevent.
    "crates/gateway/src/credential.rs",
    // P5.01: an MCP server's configured header may name a credential
    // instead of carrying one, and the header is set on the request
    // being sent - the same last slot before the wire. Listed rather
    // than redeemed one layer up, for the reason the entry above
    // records: the alternative put plaintext in `bin::assembly`, which
    // is what this list exists to prevent.
    "crates/sprawling/src/mcp_http.rs",
];

/// Exact literals the detector flags that are not credentials.
///
/// The detector is right to flag them and stays unchanged: it exists to
/// capture anything key-shaped at the entrance, where a false positive
/// costs nothing. This gate answers a different question — "is a
/// credential committed here" — where a false positive costs a build,
/// and a vendor's own API identifier cannot be a credential.
///
/// Why a table here rather than a waiver comment at the site: a comment
/// is a hole that injected content could drive through, because content
/// can write comments. Content cannot write this table. Every entry is
/// an exact match, is reviewed, and lands with a `Verdict:` trailer,
/// since this path is inside the guard's protected face.
///
/// - `CanvasRenderingContext2d` — `web_sys`'s 2D canvas type, named in
///   `crates/web/Cargo.toml` as a feature and in `web::city_view` as the
///   type the isometric city is painted through. Twenty-four bytes with
///   a digit in it, which is what trips the mixed-alphabet rule.
const NOT_CREDENTIALS: [&str; 1] = ["CanvasRenderingContext2d"];

/// Whether these exact bytes are one of the reviewed identifiers.
///
/// Exact and whole: a token that merely starts with or contains a
/// reviewed name is still a finding, so nothing can be smuggled past
/// this by wearing one as a prefix.
fn is_reviewed_identifier(found: &[u8]) -> bool {
    NOT_CREDENTIALS
        .iter()
        .any(|known| known.as_bytes() == found)
}

pub(crate) fn check(root: &Path) -> Result<Vec<Violation>, XtaskError> {
    let mut violations = Vec::new();
    for path in walk::files(root)? {
        let rel = walk::rel(root, &path);
        if walk::in_isolation_zone(&rel) {
            continue;
        }
        let bytes = std::fs::read(&path).map_err(|source| XtaskError::Io {
            path: rel.clone(),
            source,
        })?;
        for span in kernel::scan(&bytes) {
            let end = span.start.saturating_add(span.len);
            if bytes
                .get(span.start..end)
                .is_some_and(is_reviewed_identifier)
            {
                continue;
            }
            let what = span
                .provider
                .map_or("high-entropy token".to_owned(), |p| format!("{p} shape"));
            violations.push(Violation {
                gate: "secret",
                location: format!("{rel}:byte {}", span.start),
                rule: "no secret shape may live in the repository (C13)".to_owned(),
                violation: format!("{what}, {} bytes", span.len),
                alternative: "replace the value with a secret:<realm>/<name> reference; \
                              if it is a scanner self-test sample, assemble it at runtime \
                              from short fragments"
                    .to_owned(),
            });
        }
        if rel.starts_with("crates/")
            && rel.contains("/src/")
            && rel.ends_with(".rs")
            && !EXPOSE_WHITELIST.contains(&rel.as_str())
        {
            let text = String::from_utf8_lossy(&bytes);
            for (n, line) in text.lines().enumerate() {
                if line.contains(".expose(") {
                    violations.push(Violation {
                        gate: "secret",
                        location: format!("{rel}:{}", n.saturating_add(1)),
                        rule: "Sealed::expose call sites are whitelisted".to_owned(),
                        violation: "an expose call outside the redemption points".to_owned(),
                        alternative: "pass the Sealed value onward; only gateway::endpoint \
                                      and gateway::native unseal, in the last slot before \
                                      the wire"
                            .to_owned(),
                    });
                }
            }
        }
    }
    Ok(violations)
}

#[cfg(test)]
#[allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    clippy::string_slice,
    reason = "test code"
)]
mod tests {
    use super::*;

    /// Assembled at runtime from short pieces: a literal key-shaped
    /// sample in the source would be a finding about this file.
    fn key_shaped() -> String {
        ["sk", "9fQ2xZ", "7Lm4Rt", "0Bv8Kd", "3Wp6"].join("")
    }

    #[test]
    fn a_reviewed_identifier_is_matched_whole_and_nothing_else_is() {
        // Derived from the table rather than spelled out: a long literal
        // here would be a finding about this file, which is the same
        // rule this gate applies to everyone else.
        let reviewed = NOT_CREDENTIALS[0];
        assert!(is_reviewed_identifier(reviewed.as_bytes()));
        let longer = format!("{reviewed}Extra");
        assert!(
            !is_reviewed_identifier(longer.as_bytes()),
            "a longer token that starts with a reviewed name is still a finding"
        );
        let inner = &reviewed[1..reviewed.len().saturating_sub(1)];
        assert!(
            !is_reviewed_identifier(inner.as_bytes()),
            "a piece of a reviewed name is not the reviewed name"
        );
        assert!(!is_reviewed_identifier(key_shaped().as_bytes()));
    }

    #[test]
    fn the_detector_still_finds_a_key_shaped_token() {
        let sample = key_shaped();
        let spans = kernel::scan(sample.as_bytes());
        assert!(
            !spans.is_empty(),
            "the allowlist must not have loosened what the detector looks for"
        );
        for span in spans {
            let end = span.start.saturating_add(span.len);
            assert!(
                !sample
                    .as_bytes()
                    .get(span.start..end)
                    .is_some_and(is_reviewed_identifier),
                "a key-shaped token is never a reviewed identifier"
            );
        }
    }
}
