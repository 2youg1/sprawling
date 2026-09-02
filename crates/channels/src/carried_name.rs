// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Names of things this crate does not own, on their way through it.
//!
//! A mode, a provider, a template, an upload handle: each arrives as
//! text and leaves as a type that cannot be empty and cannot carry a
//! control character. That is the whole of what this crate may judge.
//!
//! **The legal value set stays upstream.** `runtime::Mode` owns which
//! modes exist, `gateway` owns which providers do, `city` owns the
//! templates; a closed list here would be a second authority that goes
//! stale the moment either side adds one. So an unknown value is not an
//! error at this boundary — it is an error where the authority is, and
//! it says so with the name in hand.
//!
//! One macro rather than four hand-written newtypes: the four differ in
//! their name and their doc line and in nothing else, and writing the
//! same constructor four times is four places for it to drift.

use kernel::{AxCode, AxError};
use serde::{Deserialize, Serialize};

macro_rules! carried_name {
    ($name:ident, $doc:literal) => {
        #[doc = $doc]
        #[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
        #[serde(transparent)]
        pub struct $name(String);

        impl $name {
            /// Sole constructor. Rejects empty and control characters only:
            /// the legal value set belongs to the upstream authority, and a
            /// second copy of it here would be a second authority.
            pub fn parse(raw: &str) -> Result<Self, AxError> {
                if raw.is_empty() || raw.chars().any(char::is_control) {
                    return Err(AxError::failure(
                        AxCode::WireMismatch,
                        concat!("read ", stringify!($name), " from a frame"),
                        "the field is empty or holds control characters",
                    )
                    .with_recovery("send a non-empty single-line value"));
                }
                Ok(Self(raw.to_owned()))
            }

            /// The carried text. Whether it names something that exists is
            /// answered upstream, not here.
            #[must_use]
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }
    };
}

carried_name!(
    ModeTag,
    "A mode name in transit. Authority for the mode set is `runtime::Mode`."
);
carried_name!(
    ProviderName,
    "A provider name in transit. Authority for the provider set is `gateway`."
);
carried_name!(
    TemplateName,
    "A Building template name in transit. Authority is `city`."
);
carried_name!(
    UploadId,
    "Handle for bytes already delivered to the upload endpoint."
);
#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, reason = "test code")]
mod tests {
    use super::*;

    #[test]
    fn a_carried_name_rejects_empty_and_control_characters() {
        assert!(ModeTag::parse("").is_err());
        assert!(ModeTag::parse("plan\nsteal").is_err());
        assert_eq!(ModeTag::parse("plan").unwrap().as_str(), "plan");
        // No closed list: an unknown mode is upstream's to refuse, not ours.
        assert!(ModeTag::parse("a-mode-we-have-never-heard-of").is_ok());
    }
}
