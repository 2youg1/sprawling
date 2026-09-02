// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Forming a city in a directory, and saying what was already there.

use std::path::{Path, PathBuf};

use kernel::{Address, AxCode, AxError, EventDraft, EventKind, EventRef};
use kernel::{Ledger, Payload, RunId};
use memory::JsonlLedger;

use crate::serving::open_vault;

use super::{RunWorker, ScanReport, ledger_dir, now_ms};

/// The city segment of every prefix, and a file the person is meant to
/// edit: `init` writes it into the city, and every later run reads that
/// copy. The binary carries the default so a fresh city is complete
/// without a checkout.
pub(super) const CITY_MD: &str = include_str!("../../../../docs/City.md");

#[derive(Debug)]
pub struct InitReport {
    pub ledger_dir: PathBuf,
    pub genesis: EventRef,
    /// What was already in the directory when the city formed, so the
    /// person who pointed at a year of their own work is told what was
    /// laid down beside it and what was left alone.
    pub standing: city::Standing,
    /// The folders that became buildings. Empty unless the caller asked
    /// for it: what is already on disk becomes governed only because
    /// somebody said so.
    pub adopted: Vec<Address>,
}

/// Whether the folders already in a directory become buildings.
///
/// Exhaustive rather than a flag, because the two are different acts: one
/// forms a city beside existing work and leaves it alone, and the other
/// puts that work under rules. A boolean would make them look like one
/// act with a setting.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Adopt {
    Nothing,
    EveryFolder,
}

/// What a directory holds, read from the directory itself.
///
/// The decision is `city::survey`'s; this only does the reading. A
/// directory that cannot be listed reads as empty, and the city forms -
/// the alternative is refusing to start over a permission error that the
/// next write would report anyway, with a better sentence.
pub(super) fn standing_of(city_root: &Path) -> city::Standing {
    let mut entries: Vec<(String, bool)> = Vec::new();
    if let Ok(listing) = std::fs::read_dir(city_root) {
        for entry in listing.flatten() {
            let is_dir = entry.file_type().map(|kind| kind.is_dir()).unwrap_or(false);
            entries.push((entry.file_name().to_string_lossy().into_owned(), is_dir));
        }
    }
    city::survey(&entries, has_history(city_root))
}

/// Whether this directory already carries a city's history.
///
/// The one fact `init` refuses on and `up` branches on, read from one
/// place so the two can never disagree about what counts as a city.
pub fn has_history(city_root: &Path) -> bool {
    std::fs::read_dir(ledger_dir(city_root))
        .map(|mut entries| entries.next().is_some())
        .unwrap_or(false)
}

/// `sprawling init <dir>`: the genesis write. The city is born when
/// `city_initialized` becomes line zero; a second init refuses — history
/// starts once.
///
/// Adopts nothing: what is already in the directory is left alone, and
/// `form_city` is the entry that puts it under rules.
///
/// # Errors
/// Whatever `form_city` reports, the refusal above included.
pub fn init_city(city_root: &Path) -> Result<InitReport, AxError> {
    form_city(city_root, Adopt::Nothing)
}

/// Forms a city in a directory, and says what was already there.
///
/// `Adopt::EveryFolder` is the case a person with a workspace wants:
/// each top-level folder becomes a building with its own rules, its
/// files untouched. Adoption happens after genesis, because a building
/// is recorded against a city and there is no city before line zero.
///
/// # Errors
/// Refuses a directory that already has history, and propagates whatever
/// the ledger, the store or the filesystem says.
pub fn form_city(city_root: &Path, adopt: Adopt) -> Result<InitReport, AxError> {
    let standing = standing_of(city_root);
    let dir = ledger_dir(city_root);
    if has_history(city_root) {
        return Err(AxError::failure(
            AxCode::ConfigInvalid,
            "initialize city",
            dir.display().to_string(),
        )
        .with_recovery("this city already has history; open it, or init a fresh directory"));
    }
    std::fs::create_dir_all(&dir).map_err(|err| {
        AxError::failure(
            AxCode::StorageFatal,
            "create ledger directory",
            err.to_string(),
        )
    })?;
    let now = now_ms()?;
    let (mut ledger, _report) =
        JsonlLedger::open(&dir, now).map_err(memory::MemoryError::into_ax)?;
    let genesis = ledger.append(EventDraft {
        run: RunId::CITY,
        t: now,
        who: "city".to_owned(),
        // The city's own name, recorded where every other fact about
        // this city is recorded. Without it the name lived only in a
        // directory entry, and every interface said "no city" over a
        // city that had been running for a month.
        addr: city_address(city_root),
        kind: EventKind::CityInitialized,
        data: Payload::empty(),
        ig: false,
    })?;
    let city_md = city_root.join(city::CITY_FILE);
    if !city_md.exists() {
        std::fs::write(&city_md, CITY_MD).map_err(|source| {
            AxError::failure(
                AxCode::StorageFatal,
                "write the city prompt",
                format!("{}: {source}", city_md.display()),
            )
            .with_recovery("check the city directory is writable")
        })?;
    }
    let mut adopted = Vec::new();
    if let (Adopt::EveryFolder, city::Standing::Work { adoptable, .. }) = (adopt, &standing) {
        // Through the same door `sprawling adopt` uses, so a folder
        // taken in at genesis and one taken in a month later end up
        // governed by the same rules.
        let (vault, _notice) = open_vault();
        let mut worker =
            RunWorker::new(city_root, vault, runtime::diagnostics::Diagnostics::off())?;
        for addr in adoptable {
            worker.adopt_building(addr.clone())?;
            adopted.push(addr.clone());
        }
    }
    Ok(InitReport {
        ledger_dir: dir,
        genesis,
        standing,
        adopted,
    })
}

/// A city's name, read from the directory it lives in.
///
/// Not every directory name is an address - a path can hold characters
/// an address may not - and a city whose directory cannot be spelled as
/// an address simply has no name to show, which is honest and rare.
pub(crate) fn city_address(city_root: &Path) -> Option<Address> {
    city_root
        .file_name()
        .and_then(|name| name.to_str())
        .and_then(|name| Address::parse(name).ok())
}

/// The city segment as this city has it: the file the person may edit,
/// falling back to the built-in copy when a city predates it.
///
/// The fallback is for one condition only. A city written before the
/// file existed does not have it, and the built-in copy is the right
/// answer there. Every other failure - a directory in its place, a
/// permission this process does not have - would have this hand a run
/// the built-in norms while the person's own edited norms sat unread on
/// the disk, and the run would obey the wrong document without anyone
/// being told.
///
/// # Errors
/// `E_STORAGE_FATAL` naming the path, for every failure except a file
/// that is not there.
pub(super) fn city_segment(city_root: &Path) -> Result<Vec<u8>, AxError> {
    let path = city_root.join(city::CITY_FILE);
    match std::fs::read(&path) {
        Ok(bytes) => Ok(bytes),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(CITY_MD.as_bytes().to_vec()),
        Err(err) => Err(AxError::failure(
            AxCode::StorageFatal,
            "read the city's norms",
            format!("{}: {err}", path.display()),
        )
        .with_recovery(
            "every run in this city is governed by that file; make it readable, \
             or take it away to fall back on the copy this build carries",
        )),
    }
}

impl RunWorker {
    /// Writes what a building's runs may reach into that building's own
    /// configuration layer.
    ///
    /// Nothing is recorded: `CONFIG.toml` is the authority for what a
    /// run is governed by, and an event carrying the same fact would be
    /// a second one. What the ledger keeps is what the run did with it.
    pub(super) fn configure_building(
        &mut self,
        addr: &Address,
        sandbox: Option<&kernel::SandboxLimits>,
        mcp: Option<&[kernel::McpServer]>,
    ) -> Result<(), AxError> {
        let building = city::Building::of(addr)?;
        if let Some(limits) = sandbox {
            city::write_sandbox(
                &self.city_root,
                building.addr(),
                city::Layer::Building,
                limits,
            )?;
        }
        if let Some(servers) = mcp {
            city::write_mcp(
                &self.city_root,
                building.addr(),
                city::Layer::Building,
                servers,
            )?;
        }
        self.note(
            runtime::diagnostics::Level::Effect,
            "city::config_layers",
            &format!("{} was reconfigured", building.addr().as_str()),
        );
        Ok(())
    }

    /// Lays out a building, then records that it exists.
    ///
    /// The file lands before the event because the event says the
    /// building is there: a history that claims a directory nobody made
    /// would be replayed as confidently as a true one.
    pub(super) fn create_building(&mut self, addr: Address, template: &str) -> Result<(), AxError> {
        let template = city::BuildingTemplate::parse(template)?;
        let building = city::create_building(&self.city_root, &addr, template)?;
        self.note(
            runtime::diagnostics::Level::Effect,
            "city::building",
            &format!(
                "{} laid out from the {} template",
                building.addr().as_str(),
                template.name()
            ),
        );
        let payload = city::building_created_payload(&building, template)?;
        self.record(EventKind::BuildingCreated, payload)
    }

    /// Adopts a directory that already sits under the city as a
    /// building; the record says it was found, not built.
    ///
    /// # Errors
    /// Propagates what `city::adopt_building` reports — a path that is
    /// not a directory under this city among them — and whatever the
    /// ledger says about the record.
    pub fn adopt_building(&mut self, addr: Address) -> Result<(), AxError> {
        let building = city::adopt_building(&self.city_root, &addr)?;
        self.note(
            runtime::diagnostics::Level::Effect,
            "city::building",
            &format!("{} adopted as a building", building.addr().as_str()),
        );
        let payload = city::building_adopted_payload(&building)?;
        self.record(EventKind::BuildingCreated, payload)
    }

    /// The startup scan: closes the account of every
    /// tool call whose outcome the last process death left unknown, and
    /// reports what is still waiting on a person. Read-only apart from
    /// the closing `tool_result` drafts, which state E_TOOL_OUTCOME_UNKNOWN
    /// rather than guessing an outcome.
    ///
    /// # Errors
    /// Propagates whatever the chain says about itself: a history that
    /// does not verify is not a history to append closing drafts to.
    pub fn startup_scan(&mut self) -> Result<ScanReport, AxError> {
        let verified = runtime::replay::verify_ledger_dir(&ledger_dir(&self.city_root))?;
        let dangling = runtime::replay::dangling_tool_calls(&verified);
        let mut closed = 0usize;
        for (run, seq) in dangling {
            let call = verified.lines().iter().find_map(|line| match line {
                runtime::replay::VerifiedLine::Known { record, .. }
                    if record.run() == run && record.seq() == seq =>
                {
                    Some(record.clone())
                }
                _ => None,
            });
            let Some(call) = call else { continue };
            let draft = runtime::replay::outcome_unknown_draft(&call, now_ms()?)?;
            self.ledger.append(draft)?;
            closed = closed.saturating_add(1);
        }
        Ok(ScanReport {
            lines: verified.raw_lines().len(),
            closed_calls: closed,
            waiting_approvals: self.governance.pending.len(),
        })
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
    use crate::assembly::fixture::*;
    use crate::assembly::*;

    /// The city's norms are what a closing city tells the next session
    /// to read, so a close that cannot read them has nothing to say.
    ///
    /// `unwrap_or_default` made the must-read locator the hash of zero
    /// bytes: the handoff still claimed the next session must read the
    /// city's norms, and pointed at nothing.
    #[test]
    fn a_city_whose_norms_cannot_be_read_refuses_to_say_it_wrote_them_down() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        let mut worker = RunWorker::new(
            dir.path(),
            gateway::Custodian::in_memory(),
            runtime::diagnostics::Diagnostics::off(),
        )
        .unwrap();
        let norms = dir.path().join(city::CITY_FILE);
        std::fs::remove_file(&norms).unwrap();
        std::fs::create_dir_all(&norms).unwrap();

        let err = worker
            .close_city()
            .expect_err("a close that cannot name the norms is not an orderly close");
        assert!(
            err.to_string().contains(city::CITY_FILE),
            "the refusal has to name the file a person must fix: {err}"
        );
    }

    #[test]
    fn deleting_every_log_line_leaves_the_history_byte_identical() {
        // The one test that keeps a log a diagnostic rather than data.
        // Two cities, the same work, one with every level on and one
        // with logging off: if a log line could reach a decision, these
        // two ledgers would differ somewhere.
        // One provider for both cities: a second listener would take a
        // second ephemeral port, and the ledger records the URL.
        let (base_url, _provider) = fake_openai(
            &["m-local"],
            vec![
                completion("editing", Some(("tu_1", "lab/room1/notes.md"))),
                completion("done", None),
            ],
        );
        let run_city = |log: runtime::diagnostics::Diagnostics| {
            let held = tempfile::tempdir().unwrap();
            // Both cities carry the same name, because the genesis record
            // now states it: two differently-named cities would differ in
            // their first line for a reason that has nothing to do with
            // logging, which is what this test is about.
            let dir = held.path().join("kiln");
            std::fs::create_dir_all(&dir).unwrap();
            let dir = dir.as_path();
            let report = init_city(dir).unwrap();
            let base_url = base_url.clone();
            let mut worker = RunWorker::new(dir, gateway::Custodian::in_memory(), log).unwrap();
            worker
                .handle(channels::Command::AttachEndpoint {
                    name: channels::ProviderName::parse("house").unwrap(),
                    base_url,
                    dialect: kernel::DialectKind::OpenAi,
                    secret: None,
                    auth_header: None,
                    admit: Vec::new(),
                    idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"attach"),
                })
                .unwrap();
            worker
                .handle(channels::Command::SelectModel {
                    endpoint: channels::ProviderName::parse("house").unwrap(),
                    model: "m-local".to_owned(),
                    tag: kernel::ModelTag::Main,
                    context_tokens: 32_768,
                    max_output_tokens: 4_096,
                    idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"select"),
                })
                .unwrap();
            // A command that fails, so the refuse level has something to
            // write in the noisy run and nothing to change in the quiet
            // one.
            let _ = worker.handle(channels::Command::Cancel {
                run: RunId::CITY,
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"cancel"),
            });
            let lines = runtime::replay::verify_ledger_dir(&report.ledger_dir)
                .unwrap()
                .raw_lines()
                .to_vec();
            // The volatile fields are the ones a second run must be
            // allowed to differ in: identifiers derived from time, and
            // the times themselves. What must match is everything else.
            lines
                .iter()
                .map(|line| {
                    let mut record: serde_json::Value =
                        serde_json::from_slice(line).unwrap_or(serde_json::Value::Null);
                    for volatile in ["t", "seq", "prev", "hash", "run", "id"] {
                        if let Some(map) = record.as_object_mut() {
                            map.remove(volatile);
                        }
                    }
                    record.to_string()
                })
                .collect::<Vec<String>>()
        };

        let written = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let sink = std::sync::Arc::clone(&written);
        let noisy = runtime::diagnostics::Diagnostics::new(
            runtime::diagnostics::Level::Wire,
            Box::new(move |line: &str| {
                sink.lock()
                    .unwrap_or_else(std::sync::PoisonError::into_inner)
                    .push(line.to_owned());
            }),
        );
        let with_logs = run_city(noisy);
        let without_logs = run_city(runtime::diagnostics::Diagnostics::off());
        assert_eq!(with_logs, without_logs);
        assert!(!with_logs.is_empty(), "the scenario has to do something");
        // And the noisy run really was noisy: an invariance that held
        // because nothing was ever written would prove nothing.
        assert!(
            !written
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_empty(),
            "the run with logging on wrote no lines"
        );
    }

    #[test]
    fn a_registration_survives_the_process_that_made_it() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        let (base_url, _provider) = fake_openai(&["m-small", "m-large"], Vec::new());
        let worker = worker_with_provider(dir.path(), &base_url, "m-large").unwrap();

        // The book is a projection: throwing it away and rebuilding from
        // the ledger has to produce the same answer, or what the city
        // can call depends on a process that has already exited.
        let rebuilt = Standing::fold(&ledger_dir(dir.path())).unwrap().book;
        let live = worker
            .book
            .select(kernel::ModelTag::Main, &kernel::BuildingPolicy::default())
            .unwrap();
        let cold = rebuilt
            .select(kernel::ModelTag::Main, &kernel::BuildingPolicy::default())
            .unwrap();
        assert_eq!(live.entry, cold.entry);
        assert_eq!(live.endpoint.base_url, cold.endpoint.base_url);
        assert_eq!(cold.endpoint.models, vec!["m-large", "m-small"]);
        assert!(cold.endpoint.is_local(), "a loopback provider is local");
    }

    #[test]
    fn a_building_created_from_the_control_surface_is_read_back_by_the_city() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let mut worker = RunWorker::new(
            dir.path(),
            gateway::Custodian::in_memory(),
            runtime::diagnostics::Diagnostics::off(),
        )
        .unwrap();
        let create = |name: &str| channels::Command::CreateBuilding {
            addr: Address::parse("vault").unwrap(),
            template: channels::TemplateName::parse(name).unwrap(),
            idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"create"),
        };

        worker.handle(create("confidential")).unwrap();

        // The building the city reads is the building the command made.
        let rules = city::load(dir.path(), &Address::parse("vault").unwrap()).unwrap();
        assert!(rules.policy().confidential);
        assert_eq!(rules.model_pool(), city::ModelPool::LocalOnly);

        // And the history says it happened, in the address's own words.
        let verified = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
        let history: String = verified
            .raw_lines()
            .iter()
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .collect::<Vec<String>>()
            .join("\n");
        assert!(history.contains("building_created"));
        assert!(history.contains("\"template\":\"confidential\""));

        // A second creation does not quietly relax the rules of a
        // building that is already working under them.
        let err = worker.handle(create("minimal")).unwrap_err();
        assert!(err.recovery().contains("already has rules"));
        assert!(
            city::load(dir.path(), &Address::parse("vault").unwrap())
                .unwrap()
                .policy()
                .confidential
        );
    }

    /// A person who already has a workspace could only be told to make a
    /// new one: `init` formed a city and said nothing about what was
    /// there, and `adopt` needed the folder to be inside a city already.
    #[test]
    fn a_folder_somebody_already_works_in_becomes_a_city_around_that_work() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("parser").join("src")).unwrap();
        std::fs::write(
            dir.path().join("parser").join("src").join("lib.rs"),
            "fn main() {}\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("notes")).unwrap();
        std::fs::write(dir.path().join("README.md"), "# my work\n").unwrap();

        let report = form_city(dir.path(), Adopt::EveryFolder).unwrap();
        let city::Standing::Work { adoptable, loose } = &report.standing else {
            panic!("a folder with work in it is not an empty one");
        };
        assert_eq!(adoptable.len(), 2);
        assert_eq!(*loose, 1, "the README is counted and left alone");
        assert_eq!(report.adopted.len(), 2);

        // The work itself is untouched, byte for byte.
        assert_eq!(
            std::fs::read_to_string(dir.path().join("parser").join("src").join("lib.rs")).unwrap(),
            "fn main() {}\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.path().join("README.md")).unwrap(),
            "# my work\n"
        );
        // And each folder is now a building with its own rules, in the
        // reserved subtree where its own runs cannot reach them.
        for name in ["parser", "notes"] {
            let addr = Address::parse(name).unwrap();
            assert!(
                city::building_path(dir.path(), &addr).is_file(),
                "{name} has no rules of its own"
            );
            assert!(city::load(dir.path(), &addr).is_ok());
        }

        // Forming a city over a city is refused: history starts once.
        let err = form_city(dir.path(), Adopt::EveryFolder).unwrap_err();
        assert_eq!(err.code(), &AxCode::ConfigInvalid);
    }

    /// A stop somebody chose and a stop that was a crash left the same
    /// silence in the record: `sprawling resume` recovered both, and
    /// nothing said which had happened.
    #[test]
    fn a_city_that_is_closed_says_so_before_it_stops() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let mut worker = RunWorker::new(
            dir.path(),
            gateway::Custodian::in_memory(),
            runtime::diagnostics::Diagnostics::off(),
        )
        .unwrap();
        worker.close_city().unwrap();

        let verified = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
        let last = verified
            .raw_lines()
            .last()
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .expect("the ledger has a last line");
        assert!(last.contains("handoff_written"), "{last}");
        assert!(
            last.contains("closed by the person"),
            "the record does not say the stop was chosen: {last}"
        );
        assert!(
            last.contains("cas:b3-"),
            "the next session is not told what to read first: {last}"
        );
    }

    #[test]
    fn init_writes_genesis_and_refuses_a_second_birth() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        assert_eq!(report.genesis.seq(), kernel::Seq::FIRST);
        assert_eq!(report.genesis.kind(), EventKind::CityInitialized);
        // The chain verifies offline (A2 face).
        runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
        // Genesis happens once.
        let err = init_city(dir.path()).unwrap_err();
        assert_eq!(err.code(), &AxCode::ConfigInvalid);
    }

    #[test]
    fn the_startup_scan_closes_dangling_calls_once_and_reports_the_rest() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        let mut worker = RunWorker::new(
            dir.path(),
            gateway::Custodian::in_memory(),
            runtime::diagnostics::Diagnostics::off(),
        )
        .unwrap();
        // A process death mid-call: tool_called with no tool_result.
        let run = RunId::from_bytes([7u8; 16]);
        let mut data = serde_json::Map::new();
        data.insert(
            "id".to_owned(),
            serde_json::Value::String("tu_9".to_owned()),
        );
        data.insert(
            "name".to_owned(),
            serde_json::Value::String("edit".to_owned()),
        );
        worker
            .record_for(
                run,
                effect::Line {
                    who: "lab/room1".to_owned(),
                    addr: Address::parse("lab/room1").unwrap(),
                    kind: EventKind::ToolCalled,
                    data: Payload::new(data).unwrap(),
                },
            )
            .unwrap();
        let report = worker.startup_scan().unwrap();
        assert_eq!(report.closed_calls, 1, "the dangling call is closed");
        // The account now shows an outcome; a second scan repairs nothing.
        let again = worker.startup_scan().unwrap();
        assert_eq!(again.closed_calls, 0, "the repair is idempotent");
        let verified = runtime::replay::verify_ledger_dir(&ledger_dir(dir.path())).unwrap();
        let closed = verified.lines().iter().any(|line| match line {
            runtime::replay::VerifiedLine::Known { record, .. } => {
                record.kind() == EventKind::ToolResult
                    && serde_json::to_string(record.data())
                        .unwrap()
                        .contains("E_TOOL_OUTCOME_UNKNOWN")
            }
            _ => false,
        });
        assert!(closed, "the closing result states the unknown outcome");
    }
}
