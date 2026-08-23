// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! `BUILDING.md` evaluated into rules a machine can hold.
//!
//! A confidential building means three things at once, and each of them
//! is held somewhere that cannot be talked out of it: the model pool is
//! local, the write domain stops at the building's own subtree, and data
//! does not leave. This module decides the first two from the file; the
//! third is the egress door's.
//!
//! A building with no `BUILDING.md` is an ordinary building. A
//! `BUILDING.md` that exists and does not say whether it is confidential
//! is an error: defaulting a privacy decision quietly is the failure this
//! whole surface exists to prevent.

use std::path::{Path, PathBuf};

use kernel::{Address, AxCode, AxError, BuildingPolicy, EgressAllowlist, WriteDomain};

/// The file a building's rules live in, at the building root.
pub const BUILDING_FILE: &str = "BUILDING.md";

const CONFIDENTIAL_KEY: &str = "confidential:";
const REVIEW_KEY: &str = "review:";
const WRITE_HEADING: &str = "write domain";
const EGRESS_HEADING: &str = "egress";
const READING_HEADING: &str = "reading room";

/// Which models a run in this building may reach. Exhaustive rather than
/// a bool, because "any" and "local only" are two policies and a third
/// (a named pool) is a change to this enum, not a new flag beside it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelPool {
    Any,
    LocalOnly,
}

/// A building's rules, as evaluated from its file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BuildingRules {
    addr: Address,
    policy: BuildingPolicy,
    write_prefixes: Vec<Address>,
    egress: EgressAllowlist,
    review: bool,
    /// The skills this building takes into its catalog, by name. The
    /// city's shelves may hold a thousand; what costs resident bytes is
    /// this list, and a person writes it.
    reading_room: Vec<String>,
}

impl BuildingRules {
    /// The policy value that rides along on every model call.
    #[must_use]
    pub fn policy(&self) -> &BuildingPolicy {
        &self.policy
    }

    #[must_use]
    pub fn addr(&self) -> &Address {
        &self.addr
    }

    /// The domains this building may reach.
    ///
    /// A confidential building has none, whatever its file says: "data
    /// enters and does not leave" is the meaning of the setting, so a
    /// declared list under it would be a contradiction the reader would
    /// have to resolve. The list it declared is refused at evaluation.
    #[must_use]
    pub fn egress(&self) -> &EgressAllowlist {
        &self.egress
    }

    /// Whether work done here has to be checked by someone else before
    /// it reaches the building.
    ///
    /// A building that says yes gives each run its own tree, and nothing
    /// a run writes is visible until another resident merges it. Absent
    /// the line, the answer is no: a person who dispatches one agent to
    /// one room and watches it work should see the file change, and
    /// requiring a second agent for that would be a discipline nobody
    /// asked for.
    #[must_use]
    pub fn review(&self) -> bool {
        self.review
    }

    /// The names under `## Reading room`, in the order the file lists
    /// them. Absent section means an empty reading room: a building
    /// starts with the tools it is given and nothing else, because the
    /// alternative is every building paying for every skill the city has
    /// ever settled.
    #[must_use]
    pub fn reading_room(&self) -> &[String] {
        &self.reading_room
    }

    #[must_use]
    pub fn model_pool(&self) -> ModelPool {
        if self.policy.confidential {
            ModelPool::LocalOnly
        } else {
            ModelPool::Any
        }
    }

    /// The write domain a run in this building gets.
    ///
    /// # Errors
    /// Refuses a confidential building that declares a prefix outside
    /// itself. Trimming it silently would leave the file saying one thing
    /// and the city doing another; refusing says which line to change.
    pub fn write_domain(&self) -> Result<WriteDomain, AxError> {
        let mut prefixes = Vec::new();
        for prefix in &self.write_prefixes {
            if self.policy.confidential && !prefix.is_within(&self.addr) {
                return Err(AxError::failure(
                    AxCode::GateDenied,
                    "build the write domain of a confidential building",
                    format!("{} reaches outside {}", prefix.as_str(), self.addr.as_str()),
                )
                .with_recovery(
                    "remove that prefix, or drop `confidential: true` and say why in the file",
                ));
            }
            prefixes.push(prefix.clone());
        }
        if prefixes.is_empty() {
            prefixes.push(self.addr.clone());
        }
        WriteDomain::new(prefixes)
    }
}

/// Where a building's rules live.
#[must_use]
pub fn building_path(city_root: &Path, addr: &Address) -> PathBuf {
    let mut path = city_root.to_path_buf();
    for segment in addr.as_str().split('/') {
        path.push(segment);
    }
    path.push(BUILDING_FILE);
    path
}

/// Loads and evaluates a building's rules.
///
/// # Errors
/// Propagates an unreadable file, and refuses one that does not state
/// whether the building is confidential.
pub fn load(city_root: &Path, addr: &Address) -> Result<BuildingRules, AxError> {
    let path = building_path(city_root, addr);
    match std::fs::read_to_string(&path) {
        Ok(text) => evaluate(addr, &text),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(BuildingRules {
            addr: addr.clone(),
            policy: BuildingPolicy::default(),
            write_prefixes: Vec::new(),
            egress: EgressAllowlist::default(),
            review: false,
            reading_room: Vec::new(),
        }),
        Err(err) => Err(AxError::failure(
            AxCode::StorageFatal,
            "read a building's rules",
            format!("{}: {err}", path.display()),
        )
        .with_recovery("fix the file's permissions; a building's rules are not optional")),
    }
}

/// Evaluates the text of a `BUILDING.md`.
///
/// # Errors
/// Refuses a file with no confidential declaration, or one whose value is
/// neither `true` nor `false` — a privacy setting that reads as a typo
/// must not resolve to the permissive side.
pub fn evaluate(addr: &Address, text: &str) -> Result<BuildingRules, AxError> {
    let mut confidential: Option<bool> = None;
    let mut review = false;
    let mut write_prefixes = Vec::new();
    let mut egress_entries: Vec<String> = Vec::new();
    let mut in_write_section = false;
    let mut in_egress_section = false;
    let mut in_reading_section = false;
    let mut reading_room: Vec<String> = Vec::new();
    for line in text.lines() {
        let trimmed = line.trim();
        let bare = trimmed.trim_start_matches(['#', '>', '`', '-', '*', ' ']);
        if trimmed.starts_with('#') {
            let heading = trimmed.to_ascii_lowercase();
            in_write_section = heading.contains(WRITE_HEADING);
            in_egress_section = heading.contains(EGRESS_HEADING);
            in_reading_section = heading.contains(READING_HEADING);
            continue;
        }
        if in_reading_section && (trimmed.starts_with("- ") || trimmed.starts_with("* ")) {
            let entry = bare.trim().trim_matches('`').trim();
            if !entry.is_empty() {
                reading_room.push(entry.to_owned());
            }
            continue;
        }
        if in_egress_section && (trimmed.starts_with("- ") || trimmed.starts_with("* ")) {
            let entry = bare.trim().trim_matches('`').trim();
            if !entry.is_empty() {
                egress_entries.push(entry.to_owned());
            }
            continue;
        }
        if let Some(rest) = bare.strip_prefix(REVIEW_KEY) {
            let value = rest.trim().trim_matches('`').trim();
            review = match value {
                "true" => true,
                "false" => false,
                other => {
                    return Err(AxError::failure(
                        AxCode::ConfigInvalid,
                        "evaluate a building's rules",
                        format!("`review: {other}` is neither true nor false"),
                    )
                    .with_recovery("write `review: true` or `review: false`"));
                }
            };
            continue;
        }
        if let Some(rest) = bare.strip_prefix(CONFIDENTIAL_KEY) {
            let value = rest.trim().trim_matches('`').trim();
            confidential = match value {
                "true" => Some(true),
                "false" => Some(false),
                other => {
                    return Err(AxError::failure(
                        AxCode::ConfigInvalid,
                        "evaluate a building's rules",
                        format!("`confidential: {other}` is neither true nor false"),
                    )
                    .with_recovery("write `confidential: true` or `confidential: false`"));
                }
            };
            continue;
        }
        if in_write_section
            && (trimmed.starts_with("- ") || trimmed.starts_with("* "))
            && let Ok(prefix) = Address::parse(bare.trim().trim_matches('`'))
        {
            write_prefixes.push(prefix);
        }
    }
    let Some(confidential) = confidential else {
        return Err(AxError::failure(
            AxCode::ConfigInvalid,
            "evaluate a building's rules",
            format!("{BUILDING_FILE} does not say whether this building is confidential"),
        )
        .with_recovery("add a `confidential: false` line, or `true` and read what it changes"));
    };
    if confidential && !egress_entries.is_empty() {
        return Err(AxError::failure(
            AxCode::ConfigInvalid,
            "evaluate a building's rules",
            format!(
                "a confidential building lists {} egress domain(s)",
                egress_entries.len()
            ),
        )
        .with_recovery(
            "remove the egress list, or drop `confidential: true`; a confidential building's \
             data does not leave, so a domain list under it contradicts the setting above it",
        ));
    }
    Ok(BuildingRules {
        addr: addr.clone(),
        policy: BuildingPolicy::new(confidential),
        write_prefixes,
        egress: EgressAllowlist::new(egress_entries),
        review,
        reading_room,
    })
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
    fn a_building_without_a_file_is_an_ordinary_building() {
        let dir = tempfile::tempdir().unwrap();
        let rules = load(dir.path(), &addr("lab")).unwrap();
        assert!(!rules.policy().confidential);
        assert_eq!(rules.model_pool(), ModelPool::Any);
        // With nothing declared, a building may write itself and no more.
        let domain = rules.write_domain().unwrap();
        assert_eq!(domain.prefixes().count(), 1);
    }

    /// The rules of a building are not writable by the runs they govern,
    /// and the write domain those rules declare is the one that has to
    /// fail to reach them.
    #[test]
    fn a_buildings_rules_sit_where_its_own_runs_cannot_write() {
        let dir = tempfile::tempdir().unwrap();
        let lab = addr("lab");
        let file = building_path(dir.path(), &lab);
        let relative = file
            .strip_prefix(dir.path())
            .unwrap()
            .to_string_lossy()
            .replace('\\', "/");
        let target = Address::parse(&relative).unwrap();
        assert!(target.is_reserved(), "{relative}");

        // The domain a building with no declarations gets: itself. It
        // must still fail to reach the file that would have declared
        // something else.
        let domain = load(dir.path(), &lab).unwrap().write_domain().unwrap();
        assert!(
            matches!(
                domain.admits(&target),
                kernel::DomainVerdict::Outside { .. }
            ),
            "a run in this building can rewrite the rules that govern it"
        );
    }

    /// A city raised before the move must not come back with its rules
    /// silently defaulted: `load` treats an absent file as an ordinary
    /// building, so a confidential one would quietly stop being
    /// confidential.
    #[test]
    fn rules_left_at_the_old_address_are_refused_rather_than_ignored() {
        let dir = tempfile::tempdir().unwrap();
        let lab = addr("lab");
        std::fs::create_dir_all(dir.path().join("lab")).unwrap();
        std::fs::write(
            dir.path().join("lab").join(BUILDING_FILE),
            "# BUILDING.md\n\n## confidential\n\n`confidential: true`\n",
        )
        .unwrap();
        let err = load(dir.path(), &lab).unwrap_err();
        assert_eq!(err.code(), &AxCode::ConfigInvalid);
        assert!(
            err.recovery().contains(".sprawling"),
            "the refusal does not say where the file goes: {}",
            err.recovery()
        );
    }

    #[test]
    fn a_file_that_does_not_say_is_refused_rather_than_assumed_open() {
        let err = evaluate(&addr("lab"), "# BUILDING.md\n\nno declaration here\n").unwrap_err();
        assert_eq!(err.code(), &AxCode::ConfigInvalid);
        assert!(err.recovery().contains("confidential: false"));
    }

    #[test]
    fn a_typo_in_the_privacy_setting_does_not_resolve_to_the_permissive_side() {
        let err = evaluate(&addr("lab"), "`confidential: yes`\n").unwrap_err();
        assert!(err.to_string().contains("neither true nor false"));
    }

    #[test]
    fn a_confidential_building_locks_the_model_pool_and_its_own_subtree() {
        let rules = evaluate(
            &addr("vault"),
            "# BUILDING.md\n\n## confidential\n\n`confidential: true`\n\n\
             ## Write domains\n\n- vault/work\n- vault/notes\n",
        )
        .unwrap();
        assert!(rules.policy().confidential);
        assert_eq!(rules.model_pool(), ModelPool::LocalOnly);
        let domain = rules.write_domain().unwrap();
        assert_eq!(domain.prefixes().count(), 2);
    }

    #[test]
    fn a_confidential_building_reaching_outside_itself_is_refused_by_name() {
        let rules = evaluate(
            &addr("vault"),
            "`confidential: true`\n\n## Write domains\n\n- vault/work\n- lab/shared\n",
        )
        .unwrap();
        let err = rules.write_domain().unwrap_err();
        assert!(err.to_string().contains("lab/shared"));
        assert!(err.recovery().contains("confidential: true"));
    }

    #[test]
    fn a_building_reaches_the_domains_it_names_and_nothing_else() {
        let rules = evaluate(
            &addr("lab"),
            "`confidential: false`\n\n## Egress\n\n- crates.io\n- `docs.rs`\n",
        )
        .unwrap();
        assert!(rules.egress().admits("static.crates.io"));
        assert!(!rules.egress().admits("pastebin.test"));
    }

    #[test]
    fn a_confidential_building_that_also_lists_domains_is_a_contradiction_and_is_refused() {
        let err = evaluate(
            &addr("vault"),
            "`confidential: true`\n\n## Egress\n\n- example.com\n",
        )
        .unwrap_err();
        assert!(err.recovery().contains("does not leave"));
    }

    #[test]
    fn a_confidential_building_reaches_nothing_public() {
        let rules = evaluate(&addr("vault"), "`confidential: true`\n").unwrap();
        assert!(rules.egress().is_empty());
        assert!(!rules.egress().admits("example.com"));
    }

    #[test]
    fn an_ordinary_building_may_declare_prefixes_beyond_itself() {
        let rules = evaluate(
            &addr("lab"),
            "`confidential: false`\n\n## Write domains\n\n- lab\n- shared/notes\n",
        )
        .unwrap();
        assert_eq!(rules.write_domain().unwrap().prefixes().count(), 2);
    }
}
