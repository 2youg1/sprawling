// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
//
// Copyright (c) 2026 2youg1

//! Where a browser keeps what it remembers, and who that belongs to.
//!
//! Login state belongs to a Building, not to the city and not to a run.
//! A building is one project with one set of accounts; two buildings
//! sharing a profile would let a run reach an account nobody granted it,
//! and the reach would be invisible because nothing was passed.
//!
//! A confidential building keeps nothing at all. The point of that mark
//! is that what happens inside it leaves no trace outside the run, and a
//! cookie jar is a trace that outlives every run that wrote to it.

use kernel::{Address, AxCode, AxError, RESERVED_PREFIX};

/// Where a building's browser profile lives, and whether it may exist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Profile {
    /// A directory under the city's reserved subtree — readable by the
    /// browser, outside every write domain, so a run cannot edit its own
    /// stored credentials into something else.
    At { path: Address },
    /// Nothing is kept. The browser runs, and forgets.
    Ephemeral,
}

/// The reserved sub-path profiles live under.
pub const PROFILES_DIR: &str = "browser-profiles";

impl Profile {
    /// Decides where `building`'s profile lives.
    ///
    /// # Errors
    /// Refuses an address that is not a building — a room does not have
    /// its own login, because a login is a property of the project.
    pub fn of(building: &Address, confidential: bool) -> Result<Profile, AxError> {
        if building.is_reserved() {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "place a browser profile",
                building.as_str().to_owned(),
            )
            .with_recovery("name a building; the city's own subtree is not one"));
        }
        if building.as_str().contains('/') {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "place a browser profile",
                building.as_str().to_owned(),
            )
            .with_recovery(
                "name the building, not a room inside it: a login belongs to the project",
            ));
        }
        if confidential {
            return Ok(Profile::Ephemeral);
        }
        let path = Address::parse(&format!(
            "{RESERVED_PREFIX}/{PROFILES_DIR}/{}",
            building.as_str()
        ))?;
        Ok(Profile::At { path })
    }

    /// Whether anything survives the run.
    #[must_use]
    pub fn persists(&self) -> bool {
        matches!(self, Profile::At { .. })
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

    fn addr(raw: &str) -> Address {
        Address::parse(raw).unwrap()
    }

    #[test]
    fn two_buildings_never_share_what_the_browser_remembers() {
        let Profile::At { path: lab } = Profile::of(&addr("lab"), false).unwrap() else {
            panic!("an ordinary building keeps a profile");
        };
        let Profile::At { path: mill } = Profile::of(&addr("mill"), false).unwrap() else {
            panic!("an ordinary building keeps a profile");
        };
        assert_ne!(lab, mill);
        assert!(!lab.is_within(&mill) && !mill.is_within(&lab));
    }

    #[test]
    fn a_profile_sits_where_no_write_domain_reaches() {
        let Profile::At { path } = Profile::of(&addr("lab"), false).unwrap() else {
            panic!("an ordinary building keeps a profile");
        };
        assert!(
            path.is_reserved(),
            "a run that could edit its own cookie jar could grant itself an account"
        );
    }

    #[test]
    fn a_confidential_building_keeps_nothing() {
        let profile = Profile::of(&addr("lab"), true).unwrap();
        assert_eq!(profile, Profile::Ephemeral);
        assert!(!profile.persists());
    }

    #[test]
    fn a_room_has_no_login_of_its_own() {
        let err = Profile::of(&addr("lab/room1"), false).unwrap_err();
        assert_eq!(err.code(), &AxCode::InvalidArgs);
        assert!(err.recovery().contains("belongs to the project"));
    }

    #[test]
    fn the_citys_own_subtree_is_not_a_building() {
        assert!(Profile::of(&addr(RESERVED_PREFIX), false).is_err());
    }
}
