// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Starting a city, and moving a resident inside one.
//!
//! Both are decisions rather than actions. What a new city consists of
//! and what a move implies are answered here as values; making the
//! directories and writing the events is the binary's work. That split
//! is what lets "one instruction builds a city" be tested without
//! building one.
//!
//! A move is the harder of the two, and the reason is that an address
//! decides three things at once: where the resident may write, what it
//! reads by default, and who it reports to. Moving is therefore never a
//! rename — it is a new write domain, and the history stays where it
//! happened. A city that rewrote history to match a new address would be
//! a city where "this is where it was done" has no answer.

use kernel::{Address, AxCode, AxError, RESERVED_PREFIX};

use crate::building::BuildingTemplate;

/// What `init` makes. Values only, so the shape of a new city can be
/// asserted without a filesystem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CityPlan {
    /// Directories to create, city-root relative, in creation order.
    dirs: Vec<Address>,
    /// The first building, if the instruction named one.
    first: Option<(Address, BuildingTemplate)>,
}

impl CityPlan {
    /// Plans a city from one instruction.
    ///
    /// `first` is the building to raise immediately. An empty city is
    /// legal — it is what an empty directory becomes — but a city with a
    /// first building is the ordinary case, and the wizard exists so
    /// that takes one instruction rather than three.
    ///
    /// # Errors
    /// Refuses a first building whose name is the reserved prefix, one
    /// that names a room rather than a building, and one whose template
    /// this version does not have.
    pub fn new(first: Option<(&str, &str)>) -> Result<CityPlan, AxError> {
        let mut dirs = vec![Address::parse(RESERVED_PREFIX)?];
        let first = match first {
            None => None,
            Some((name, template)) => {
                let addr = Address::parse(name)?;
                if addr.is_reserved() {
                    return Err(AxError::failure(
                        AxCode::InvalidArgs,
                        "raise the first building",
                        name.to_owned(),
                    )
                    .with_recovery(format!(
                        "`{RESERVED_PREFIX}` is the city's own subtree; choose another name"
                    )));
                }
                if addr.as_str().contains('/') {
                    return Err(AxError::failure(
                        AxCode::InvalidArgs,
                        "raise the first building",
                        name.to_owned(),
                    )
                    .with_recovery("name a building, not a room inside one"));
                }
                let template = BuildingTemplate::parse(template)?;
                dirs.push(addr.clone());
                Some((addr, template))
            }
        };
        Ok(CityPlan { dirs, first })
    }

    #[must_use]
    pub fn dirs(&self) -> &[Address] {
        &self.dirs
    }

    #[must_use]
    pub fn first(&self) -> Option<&(Address, BuildingTemplate)> {
        self.first.as_ref()
    }
}

/// What moving a resident implies. Every field is something a caller has
/// to act on; nothing here is decoration.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Relocation {
    /// Where the resident was. Its history stays addressed to this.
    pub from: Address,
    /// Where it will be, and therefore what it may write.
    pub to: Address,
    /// Whether the move crosses buildings. A move inside one building
    /// changes a desk; a move between two changes an employer, and the
    /// reading room, the policy and the archive all change with it.
    pub crosses_building: bool,
}

/// Decides a move.
///
/// # Errors
/// Refuses a move to or from the reserved subtree, a move onto the same
/// address, and a move to an address that is not a room. The last one is
/// the interesting refusal: a resident lives in a room, and letting one
/// live at a building's root would give it the whole building's write
/// domain by a side door.
pub fn relocate(from: &Address, to: &Address) -> Result<Relocation, AxError> {
    for addr in [from, to] {
        if addr.is_reserved() {
            return Err(AxError::failure(
                AxCode::OutsideWriteDomain,
                "relocate a resident",
                addr.as_str().to_owned(),
            )
            .with_recovery("the city's own subtree houses nobody"));
        }
    }
    if from == to {
        return Err(AxError::failure(
            AxCode::InvalidArgs,
            "relocate a resident",
            from.as_str().to_owned(),
        )
        .with_recovery("name a different room; a move to the same address is not a move"));
    }
    if !to.as_str().contains('/') {
        return Err(AxError::failure(
            AxCode::InvalidArgs,
            "relocate a resident",
            to.as_str().to_owned(),
        )
        .with_recovery(
            "name a room inside a building; living at a building's root would hand over the \
             whole building's write domain",
        ));
    }
    let crosses_building = building_of(from) != building_of(to);
    Ok(Relocation {
        from: from.clone(),
        to: to.clone(),
        crosses_building,
    })
}

fn building_of(addr: &Address) -> &str {
    addr.as_str().split('/').next().unwrap_or(addr.as_str())
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
    fn one_instruction_plans_a_city_with_somewhere_to_work_in_it() {
        let plan = CityPlan::new(Some(("lab", "minimal"))).unwrap();
        assert_eq!(plan.dirs()[0].as_str(), RESERVED_PREFIX);
        assert_eq!(plan.dirs()[1].as_str(), "lab");
        let (first, _) = plan.first().unwrap();
        assert_eq!(first.as_str(), "lab");
    }

    #[test]
    fn an_empty_city_is_legal_and_is_what_an_empty_directory_becomes() {
        let plan = CityPlan::new(None).unwrap();
        assert_eq!(plan.dirs().len(), 1);
        assert!(plan.first().is_none());
    }

    #[test]
    fn the_first_building_cannot_be_the_citys_own_subtree_or_a_room() {
        let reserved = CityPlan::new(Some((RESERVED_PREFIX, "minimal"))).unwrap_err();
        assert!(reserved.recovery().contains("another name"));
        let room = CityPlan::new(Some(("lab/room1", "minimal"))).unwrap_err();
        assert!(room.recovery().contains("not a room"));
    }

    #[test]
    fn a_move_between_buildings_is_a_different_thing_from_a_move_inside_one() {
        let inside = relocate(&addr("lab/room1"), &addr("lab/room2")).unwrap();
        assert!(!inside.crosses_building);
        let across = relocate(&addr("lab/room1"), &addr("mill/room1")).unwrap();
        assert!(
            across.crosses_building,
            "the reading room, the policy and the archive all change with the employer"
        );
    }

    #[test]
    fn a_move_keeps_the_old_address_because_that_is_where_the_work_happened() {
        let move_ = relocate(&addr("lab/room1"), &addr("mill/room1")).unwrap();
        assert_eq!(
            move_.from.as_str(),
            "lab/room1",
            "history stays addressed to where it happened; a rename would delete that answer"
        );
    }

    #[test]
    fn nobody_may_move_to_a_buildings_root_or_into_the_reserved_subtree() {
        let root = relocate(&addr("lab/room1"), &addr("mill")).unwrap_err();
        assert!(root.recovery().contains("whole building's write domain"));
        let reserved = relocate(&addr("lab/room1"), &addr(RESERVED_PREFIX)).unwrap_err();
        assert_eq!(reserved.code(), &AxCode::OutsideWriteDomain);
        assert!(relocate(&addr(RESERVED_PREFIX), &addr("lab/room1")).is_err());
    }

    #[test]
    fn a_move_to_the_same_address_is_refused_rather_than_quietly_doing_nothing() {
        let err = relocate(&addr("lab/room1"), &addr("lab/room1")).unwrap_err();
        assert_eq!(err.code(), &AxCode::InvalidArgs);
    }
}
