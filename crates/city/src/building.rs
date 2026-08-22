// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! Which building governs an address, and how a new one comes into being.
//!
//! A building is a top-level address. That is not a naming habit: an
//! address decides where a run may write, what it reads by default, and
//! who it reports to, and all three answers are read off the first
//! segment. A building nested inside a building would give those
//! questions a second answer.
//!
//! The bytes a new building starts with are the template a person reads
//! in `docs/templates/BUILDING.md`, not a copy of it kept here. One
//! string, one authority; the confidential template differs from the
//! ordinary one by the single line whose value the city refuses to
//! assume.
//!
//! What a new building starts with beyond its rules — the plan, the
//! memo, the handoff — is `crate::spine_files`'s to lay out.

use std::path::{Path, PathBuf};

use kernel::{Address, AxCode, AxError, Payload};

use crate::policy::BUILDING_FILE;

/// The rules a new building starts with. Instantiated at compile time so
/// that a moved or renamed template breaks the build rather than a city.
const TEMPLATE_RULES: &str = include_str!("../../../docs/templates/BUILDING.md");
const NAME_PLACEHOLDER: &str = "<building name>";
const ORDINARY_LINE: &str = "`confidential: false`";
const CONFIDENTIAL_LINE: &str = "`confidential: true`";

/// What a new building is laid out as. Exhaustive: a template exists
/// because some kind of building needs different bytes on its first day,
/// and a kind nobody creates is an authority nobody reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BuildingTemplate {
    /// An ordinary building: data may leave, any model may answer.
    Minimal,
    /// Data enters and does not leave, the model pool is local, and
    /// writes stop at this building's own subtree.
    Confidential,
}

impl BuildingTemplate {
    /// Reads a template name as it arrived from the control surface.
    ///
    /// # Errors
    /// Refuses a name this version does not lay out, and says which ones
    /// it does: a caller that guessed needs the list, not a verdict.
    pub fn parse(name: &str) -> Result<BuildingTemplate, AxError> {
        match name {
            "minimal" => Ok(BuildingTemplate::Minimal),
            "confidential" => Ok(BuildingTemplate::Confidential),
            other => Err(AxError::failure(
                AxCode::InvalidArgs,
                "read a building template name",
                other.to_owned(),
            )
            .with_recovery("this version lays out `minimal` and `confidential`")),
        }
    }

    #[must_use]
    pub fn name(self) -> &'static str {
        match self {
            BuildingTemplate::Minimal => "minimal",
            BuildingTemplate::Confidential => "confidential",
        }
    }

    /// The `BUILDING.md` bytes this template starts a building with.
    ///
    /// # Errors
    /// Refuses when the template no longer carries the line a
    /// confidential building differs by: producing an ordinary building
    /// from the confidential template is the one failure here that
    /// nobody would notice until data left.
    fn rules(self, addr: &Address) -> Result<String, AxError> {
        let named = TEMPLATE_RULES.replace(NAME_PLACEHOLDER, addr.as_str());
        match self {
            BuildingTemplate::Minimal => Ok(named),
            BuildingTemplate::Confidential => {
                if !named.contains(ORDINARY_LINE) {
                    return Err(AxError::failure(
                        AxCode::ConfigInvalid,
                        "lay out a confidential building",
                        format!("the template no longer carries {ORDINARY_LINE}"),
                    )
                    .with_recovery(
                        "restore that line in docs/templates/BUILDING.md; the confidential \
                         template is the ordinary one with that value flipped",
                    ));
                }
                Ok(named.replace(ORDINARY_LINE, CONFIDENTIAL_LINE))
            }
        }
    }
}

/// A building: the top-level address that governs a run.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Building {
    addr: Address,
}

impl Building {
    /// Which building governs `addr`.
    ///
    /// # Errors
    /// Refuses the reserved subtree. `.sprawling/` holds the city's own
    /// ledger and configuration; treating it as a building would make
    /// the city's configuration readable as some building's own.
    pub fn of(addr: &Address) -> Result<Building, AxError> {
        let head = addr.as_str().split('/').next().unwrap_or(addr.as_str());
        let head = Address::parse(head)?;
        if head.is_reserved() {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "name the building that governs an address",
                addr.as_str().to_owned(),
            )
            .with_recovery(
                "address a building instead; the reserved subtree is the city's own account \
                 and belongs to no building",
            ));
        }
        Ok(Building { addr: head })
    }

    #[must_use]
    pub fn addr(&self) -> &Address {
        &self.addr
    }

    /// Where this building's own files live.
    #[must_use]
    pub fn root(&self, city_root: &Path) -> PathBuf {
        let mut path = city_root.to_path_buf();
        for segment in self.addr.as_str().split('/') {
            path.push(segment);
        }
        path
    }

    /// Whether `addr` is a room of this building.
    #[must_use]
    pub fn holds(&self, addr: &Address) -> bool {
        addr.is_within(&self.addr)
    }
}

/// Lays out a new building and returns it.
///
/// # Errors
/// Refuses a room address, the reserved subtree, and a building that
/// already exists. The last one matters most: overwriting would replace
/// the rules a running building works under, and those rules may be the
/// ones that keep its data at home.
pub fn create(
    city_root: &Path,
    addr: &Address,
    template: BuildingTemplate,
) -> Result<Building, AxError> {
    let building = Building::of(addr)?;
    if building.addr() != addr {
        return Err(AxError::failure(
            AxCode::InvalidArgs,
            "create a building",
            addr.as_str().to_owned(),
        )
        .with_recovery(format!(
            "create the building `{}` and dispatch to `{}` inside it; a room is not a building",
            building.addr().as_str(),
            addr.as_str()
        )));
    }
    let root = building.root(city_root);
    std::fs::create_dir_all(&root).map_err(|err| storage(&root, &err))?;
    let file = root.join(BUILDING_FILE);
    let rules = template.rules(addr)?;
    // `create_new` rather than exists-then-write: the refusal and the
    // write are one operation, so no second caller lands in between.
    match std::fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&file)
    {
        Ok(mut handle) => {
            std::io::Write::write_all(&mut handle, rules.as_bytes())
                .map_err(|err| storage(&file, &err))?;
            // After the refusal point, not before it: a building that
            // was refused leaves nothing of itself behind.
            crate::spine_files::lay_out(&root, addr)?;
            Ok(building)
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => Err(AxError::failure(
            AxCode::InvalidArgs,
            "create a building",
            addr.as_str().to_owned(),
        )
        .with_recovery(
            "this building already has rules; edit its BUILDING.md, or create a building \
             at an address nobody occupies",
        )),
        Err(err) => Err(storage(&file, &err)),
    }
}

/// Adopts a directory that already exists - a checked-out repository, a
/// folder of notes - as a building. The same layout `create` writes, on
/// top of what is already there: `BUILDING.md` must not exist yet, and
/// the spine files are only laid where they are missing, so nothing the
/// directory holds is overwritten.
///
/// # Errors
/// Refuses an address with no directory (that is a create, not an
/// adopt), a room address, and a directory that is already a building.
pub fn adopt(city_root: &Path, addr: &Address) -> Result<Building, AxError> {
    let building = Building::of(addr)?;
    let root = building.root(city_root);
    if !root.is_dir() {
        return Err(AxError::failure(
            AxCode::InvalidArgs,
            "adopt a building",
            format!("{} holds no directory", addr.as_str()),
        )
        .with_recovery(
            "move or clone the directory under the city first, or use create for an empty \
             building",
        ));
    }
    create(city_root, addr, BuildingTemplate::Minimal)?;
    Ok(building)
}

/// What the ledger records about a building coming into being.
/// `adopted` says how: laid out empty, or drawn over an existing
/// directory - the history should not claim it built what it found.
///
/// # Errors
/// Propagates the payload's own refusal to hold what it was given.
pub fn created_payload(
    building: &Building,
    template: BuildingTemplate,
) -> Result<Payload, AxError> {
    let mut map = serde_json::Map::new();
    map.insert(
        "addr".to_owned(),
        serde_json::Value::String(building.addr().as_str().to_owned()),
    );
    map.insert(
        "template".to_owned(),
        serde_json::Value::String(template.name().to_owned()),
    );
    Payload::new(map)
}

/// The ledger record for an adoption.
///
/// # Errors
/// Propagates the payload's own refusal to hold what it was given.
pub fn adopted_payload(building: &Building) -> Result<Payload, AxError> {
    let mut map = serde_json::Map::new();
    map.insert(
        "addr".to_owned(),
        serde_json::Value::String(building.addr().as_str().to_owned()),
    );
    map.insert(
        "template".to_owned(),
        serde_json::Value::String(BuildingTemplate::Minimal.name().to_owned()),
    );
    map.insert("adopted".to_owned(), serde_json::Value::Bool(true));
    Payload::new(map)
}

fn storage(path: &Path, err: &std::io::Error) -> AxError {
    AxError::failure(
        AxCode::StorageFatal,
        "lay out a building",
        format!("{}: {err}", path.display()),
    )
    .with_recovery("fix the path's permissions, then create the building again")
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
    use crate::policy::{self, ModelPool};

    fn addr(raw: &str) -> Address {
        Address::parse(raw).unwrap()
    }

    #[test]
    fn a_created_building_is_read_back_by_the_citys_own_parser() {
        let dir = tempfile::tempdir().unwrap();
        let building = create(dir.path(), &addr("lab"), BuildingTemplate::Minimal).unwrap();
        assert_eq!(building.addr(), &addr("lab"));

        let rules = policy::load(dir.path(), &addr("lab")).unwrap();
        assert!(!rules.policy().confidential);
        assert_eq!(rules.model_pool(), ModelPool::Any);
        // With nothing declared, a new building may write itself and no more.
        assert_eq!(rules.write_domain().unwrap().prefixes().count(), 1);

        let text = std::fs::read_to_string(building.root(dir.path()).join(BUILDING_FILE)).unwrap();
        assert!(
            text.contains("lab"),
            "a building's own rules name the building"
        );
        assert!(!text.contains(NAME_PLACEHOLDER));
        assert!(
            building
                .root(dir.path())
                .join(crate::spine_files::ROADMAP_FILE)
                .exists(),
            "a building exists with its plan, not only with its rules"
        );
    }

    #[test]
    fn the_confidential_template_produces_a_building_the_city_treats_as_confidential() {
        let dir = tempfile::tempdir().unwrap();
        create(dir.path(), &addr("vault"), BuildingTemplate::Confidential).unwrap();

        let rules = policy::load(dir.path(), &addr("vault")).unwrap();
        assert!(rules.policy().confidential);
        assert_eq!(rules.model_pool(), ModelPool::LocalOnly);
        assert!(rules.egress().is_empty());
    }

    #[test]
    fn a_second_birth_is_refused_and_the_first_rules_survive() {
        let dir = tempfile::tempdir().unwrap();
        create(dir.path(), &addr("vault"), BuildingTemplate::Confidential).unwrap();

        let err = create(dir.path(), &addr("vault"), BuildingTemplate::Minimal).unwrap_err();
        assert_eq!(err.code(), &AxCode::InvalidArgs);
        assert!(err.recovery().contains("already has rules"));
        assert!(
            policy::load(dir.path(), &addr("vault"))
                .unwrap()
                .policy()
                .confidential,
            "the refusal left the confidential rules in place"
        );
    }

    #[test]
    fn a_room_address_is_refused_and_the_refusal_names_the_building_to_create() {
        let dir = tempfile::tempdir().unwrap();
        let err = create(dir.path(), &addr("lab/room1"), BuildingTemplate::Minimal).unwrap_err();
        assert_eq!(err.code(), &AxCode::InvalidArgs);
        assert!(err.recovery().contains("`lab`"));
        assert!(!dir.path().join("lab").join("room1").exists());
        assert!(
            !dir.path().join("lab").exists(),
            "a refused building leaves nothing behind"
        );
    }

    #[test]
    fn the_reserved_subtree_belongs_to_no_building() {
        let dir = tempfile::tempdir().unwrap();
        let err = Building::of(&addr(".sprawling/cas/ab")).unwrap_err();
        assert!(err.recovery().contains("reserved subtree"));

        let err = create(dir.path(), &addr(".sprawling"), BuildingTemplate::Minimal).unwrap_err();
        assert_eq!(err.code(), &AxCode::InvalidArgs);
        assert!(!dir.path().join(".sprawling").join(BUILDING_FILE).exists());
    }

    #[test]
    fn a_building_holds_its_own_rooms_and_no_others() {
        let lab = Building::of(&addr("lab/room1")).unwrap();
        assert_eq!(lab.addr(), &addr("lab"));
        assert!(lab.holds(&addr("lab")));
        assert!(lab.holds(&addr("lab/room1/notes.md")));
        assert!(!lab.holds(&addr("laboratory/room1")));
        assert!(!lab.holds(&addr("vault")));
    }

    #[test]
    fn an_unknown_template_is_refused_with_the_set_this_version_lays_out() {
        let err = BuildingTemplate::parse("workshop").unwrap_err();
        assert_eq!(err.code(), &AxCode::InvalidArgs);
        assert!(err.recovery().contains("minimal"));
        assert!(err.recovery().contains("confidential"));
        assert_eq!(
            BuildingTemplate::parse(BuildingTemplate::Confidential.name()).unwrap(),
            BuildingTemplate::Confidential
        );
    }

    #[test]
    fn the_record_carries_the_address_and_the_template_and_nothing_else() {
        let building = Building::of(&addr("lab")).unwrap();
        let payload = created_payload(&building, BuildingTemplate::Confidential).unwrap();
        let map = payload.as_map();
        assert_eq!(map.len(), 2);
        assert_eq!(map.get("addr").and_then(|v| v.as_str()), Some("lab"));
        assert_eq!(
            map.get("template").and_then(|v| v.as_str()),
            Some("confidential")
        );
    }

    #[test]
    fn adopting_an_existing_directory_keeps_every_file_it_found() {
        let city = tempfile::tempdir().unwrap();
        let repo = city.path().join("imported");
        std::fs::create_dir_all(repo.join("src")).unwrap();
        std::fs::write(repo.join("src").join("main.rs"), "fn main() {}\n").unwrap();
        std::fs::write(repo.join("Roadmap.md"), "# my own roadmap\n").unwrap();

        let building = adopt(city.path(), &addr("imported")).unwrap();
        assert_eq!(building.addr().as_str(), "imported");
        // What was there is untouched; what was missing is laid.
        assert_eq!(
            std::fs::read_to_string(repo.join("src").join("main.rs")).unwrap(),
            "fn main() {}\n"
        );
        assert_eq!(
            std::fs::read_to_string(repo.join("Roadmap.md")).unwrap(),
            "# my own roadmap\n",
            "an adopted roadmap is the owner's, not the template's"
        );
        assert!(repo.join("BUILDING.md").is_file());
        assert!(repo.join("Memo.md").is_file());
        // Adopting twice refuses: it is already a building.
        assert!(adopt(city.path(), &addr("imported")).is_err());
        // Adopting nothing refuses toward create.
        let err = adopt(city.path(), &addr("ghost")).unwrap_err();
        assert!(err.recovery().contains("create"), "{err}");
        // The record says it was an adoption.
        let payload = adopted_payload(&building).unwrap();
        assert_eq!(
            payload.as_map().get("adopted"),
            Some(&serde_json::Value::Bool(true))
        );
    }
}
