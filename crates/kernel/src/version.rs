// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Optimistic concurrency: writes carry a
//! `base_version`; a stale base yields a verdict, never a silent merge.
//!
//! The mapping from [`VersionVerdict::Stale`] to `E_VERSION_CONFLICT`
//! plus a fresh diff lives with the edit tool (S3); kernel only judges
//! freshness and knows nothing about diffs.

use serde::{Deserialize, Serialize};

use crate::error::{AxCode, AxError};

/// Monotonic per-subject version; starts at [`Version::FIRST`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Version(u64);

impl Version {
    /// The first visible version of any subject.
    pub const FIRST: Version = Version(1);

    pub fn new(value: u64) -> Self {
        Version(value)
    }

    pub fn value(&self) -> u64 {
        self.0
    }

    pub fn next(self) -> Result<Version, AxError> {
        self.0.checked_add(1).map(Version).ok_or_else(|| {
            AxError::failure(AxCode::InvalidArgs, "advance version", self.0.to_string())
                .with_recovery("version space exhausted for this subject")
        })
    }
}

/// Freshness verdict — exhaustive, not a bool (decision-function shape).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VersionVerdict {
    Fresh,
    Stale { current: Version },
}

/// Pure freshness judgment. A base ahead of `current` is caller fiction
/// and reports `Stale` too: only `current` is the truth.
pub fn check_base(current: Version, base: Version) -> VersionVerdict {
    if base == current {
        VersionVerdict::Fresh
    } else {
        VersionVerdict::Stale { current }
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

    #[test]
    fn first_version_is_one_and_advances_checked() {
        assert_eq!(Version::FIRST.value(), 1);
        assert_eq!(Version::FIRST.next().unwrap().value(), 2);
        assert!(Version::new(u64::MAX).next().is_err());
    }

    #[test]
    fn matching_base_is_fresh() {
        let current = Version::new(7);
        assert_eq!(check_base(current, Version::new(7)), VersionVerdict::Fresh);
    }

    #[test]
    fn stale_and_ahead_bases_both_report_stale_with_current() {
        let current = Version::new(7);
        for base in [Version::new(6), Version::new(8)] {
            assert_eq!(
                check_base(current, base),
                VersionVerdict::Stale { current },
                "only `current` is the truth; an ahead base is caller fiction"
            );
        }
    }

    #[test]
    fn serde_is_transparent_u64() {
        assert_eq!(serde_json::to_string(&Version::new(3)).unwrap(), "3");
        let v: Version = serde_json::from_str("9").unwrap();
        assert_eq!(v, Version::new(9));
    }
}
