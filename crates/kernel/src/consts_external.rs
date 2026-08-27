// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! External-fact constants: values that follow the
//! outside world. Changing one requires evidence that the world changed,
//! never our own preference. Data only — zero branches by charter.

/// Provider-side explicit cache breakpoint ceiling.
pub const CACHE_BREAKPOINTS_MAX: u32 = 4;

/// Prompt-cache lifetime treated as the async-discipline horizon (6.4).
pub const PROMPT_CACHE_TTL_SECS: u64 = 300;

/// EventRecord `v`: one monotonic integer, no major/minor split (3.1).
pub const EVENT_LOG_V: u32 = 1;

/// The L0 tool set: always present, never discovered (5.1).
pub const L0_TOOLS: [&str; 3] = ["exec", "edit", "status"];

/// One secret shape: prefix + charset + length window (7.1). The table is
/// data, not code; it grows with providers, and `kernel::secret::scan`
/// (S2) plus `xtask secret` are its only consumers.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretShape {
    pub provider: &'static str,
    pub prefix: &'static str,
    pub charset: SecretCharset,
    /// Closed interval over the length of the part after the prefix.
    pub len: (u16, u16),
}

/// Character sets that public token formats draw from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretCharset {
    /// `[A-Za-z0-9]`
    Base62,
    /// `[A-Za-z0-9_-]`
    Base64Url,
    /// `[0-9a-f]`
    HexLower,
    /// `[A-Z0-9]`
    UpperBase36,
}

/// Public provider token shapes, per each provider's published format.
pub const SECRET_SHAPES: [SecretShape; 8] = [
    SecretShape {
        provider: "anthropic",
        prefix: "sk-ant-",
        charset: SecretCharset::Base64Url,
        len: (24, 120),
    },
    SecretShape {
        provider: "openai",
        prefix: "sk-proj-",
        charset: SecretCharset::Base64Url,
        len: (24, 200),
    },
    SecretShape {
        provider: "github",
        prefix: "ghp_",
        charset: SecretCharset::Base62,
        len: (36, 40),
    },
    SecretShape {
        provider: "github-oauth",
        prefix: "gho_",
        charset: SecretCharset::Base62,
        len: (36, 40),
    },
    SecretShape {
        provider: "aws",
        prefix: "AKIA",
        charset: SecretCharset::UpperBase36,
        len: (16, 16),
    },
    SecretShape {
        provider: "gitlab",
        prefix: "glpat-",
        charset: SecretCharset::Base64Url,
        len: (20, 22),
    },
    SecretShape {
        provider: "slack",
        prefix: "xoxb-",
        charset: SecretCharset::Base62,
        len: (24, 80),
    },
    SecretShape {
        provider: "google",
        prefix: "AIza",
        charset: SecretCharset::Base64Url,
        len: (35, 35),
    },
];

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
    use std::collections::BTreeSet;

    #[test]
    fn the_five_facts_hold_their_documented_values() {
        assert_eq!(CACHE_BREAKPOINTS_MAX, 4);
        assert_eq!(PROMPT_CACHE_TTL_SECS, 300);
        assert_eq!(EVENT_LOG_V, 1);
        assert_eq!(L0_TOOLS, ["exec", "edit", "status"]);
        assert_eq!(SECRET_SHAPES.len(), 8);
    }

    #[test]
    fn secret_shapes_are_wellformed_data() {
        let prefixes: BTreeSet<&str> = SECRET_SHAPES.iter().map(|s| s.prefix).collect();
        assert_eq!(
            prefixes.len(),
            SECRET_SHAPES.len(),
            "prefixes must be unique"
        );
        for shape in &SECRET_SHAPES {
            assert!(!shape.prefix.is_empty());
            assert!(!shape.provider.is_empty());
            let (lo, hi) = shape.len;
            assert!(
                lo > 0 && lo <= hi,
                "{}: empty or inverted window",
                shape.provider
            );
        }
    }
}
