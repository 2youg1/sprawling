// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! One building, as the files in it say it is.

use std::path::Path;

use kernel::Address;

use super::name_of;

/// How much of one document travels to a page.
///
/// These files grow for as long as a building works, and the interface
/// reads them rather than edits them. A cut is stated on the answer, so
/// a reader who needs the rest knows there is a rest.
pub(super) const DOC_BYTES_MAX: usize = 64 * 1024;

/// One building, as the files in it say it is.
///
/// The files are the authority, so the documents, the rooms and the
/// archive are read at the moment of asking rather than kept as a second
/// copy of what the disk says. The plan arrives already read, from the
/// one projection that reads it: a second parse here would be a second
/// answer to "what is stuck and why", and only one of them would be
/// folding the records that say why.
pub(crate) fn read_building(
    city_root: &Path,
    addr: &Address,
    plan: crate::plan_view::PlanReading,
) -> Option<channels::BuildingAnswer> {
    let root = city_root.join(addr.as_str());
    if !root.is_dir() {
        return None;
    }
    let crate::plan_view::PlanReading {
        progress,
        problems,
        rows: plan,
        blocked,
        ready: _,
    } = plan;
    // What counts as a room is `city::rooms`, which the model-facing
    // roster reads too: a page and an agent disagreeing about which
    // rooms a building has would be two answers to one question. A
    // directory this cannot read has no rooms to draw, which is what a
    // page owes its reader - the roster propagates the same failure
    // instead, because a resident told it is alone would act on it.
    let rooms: Vec<String> = city::rooms(city_root, addr)
        .unwrap_or_default()
        .iter()
        .map(|room| name_of(room).to_owned())
        .collect();
    let mut docs = Vec::new();
    // The rules, read by their own path: a building's rules live inside
    // a dot directory, and the walk below reads files rather than
    // directories, so a page that only walked would have quietly lost
    // the tab that shows what this building is allowed to do.
    if let Ok(bytes) = std::fs::read(city::building_path(city_root, addr)) {
        docs.push(doc_from(city::BUILDING_FILE.to_owned(), &bytes));
    }
    if let Ok(entries) = std::fs::read_dir(&root) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            if entry.path().is_dir() {
                continue;
            }
            if !name.ends_with(".md") {
                continue;
            }
            let Ok(bytes) = std::fs::read(entry.path()) else {
                continue;
            };
            docs.push(doc_from(name, &bytes));
        }
    }
    docs.sort_by_key(|doc| doc_order(&doc.name));
    let archive = city::archive_index(city_root, addr)
        .unwrap_or_default()
        .into_iter()
        .map(|entry| channels::ArchiveLine {
            kind: entry.kind.as_str().to_owned(),
            day: entry.day,
            subject: entry.subject,
        })
        .collect();
    // The building's own rung of the ladder, not the resolved value: a
    // form filled from the resolved value would write the city's
    // setting into the building the first time anybody pressed save.
    let own = city::config_path(city_root, addr, city::Layer::Building)
        .ok()
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| city::ConfigLayer::parse(&text).ok())
        .unwrap_or_default();
    Some(channels::BuildingAnswer {
        addr: addr.clone(),
        progress,
        problems,
        plan,
        blocked,
        rooms,
        docs,
        archive,
        sandbox: own.sandbox().cloned(),
        mcp: own
            .mcp()
            .map(<[kernel::McpServer]>::to_vec)
            .unwrap_or_default(),
    })
}

/// One document as a page receives it, cut to what travels.
pub(super) fn doc_from(name: String, bytes: &[u8]) -> channels::BuildingDoc {
    let head = bytes.get(..bytes.len().min(DOC_BYTES_MAX)).unwrap_or(bytes);
    channels::BuildingDoc {
        name,
        text: String::from_utf8_lossy(head).into_owned(),
        bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        truncated: bytes.len() > DOC_BYTES_MAX,
    }
}

/// Reading order for a building's documents: the plan, then the record of
/// decisions, then the handoff, then the rules. A person opening a
/// building wants to know what it is doing before they read what it is
/// allowed to do.
pub(super) fn doc_order(name: &str) -> (u8, String) {
    let rank = match name {
        city::ROADMAP_FILE => 0,
        "Memo.md" => 1,
        "Handoff.md" => 2,
        city::BUILDING_FILE => 3,
        city::URBANITE_FILE => 4,
        _ => 5,
    };
    (rank, name.to_owned())
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

    /// The rules a person may read on the building page are the rules
    /// the city obeys, and the page reads them from a directory the
    /// walk deliberately skips.
    #[test]
    fn a_building_page_still_shows_the_rules_that_govern_it() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        city::create_building(
            dir.path(),
            &Address::parse("lab").unwrap(),
            city::BuildingTemplate::Minimal,
        )
        .unwrap();
        let lab = Address::parse("lab").unwrap();
        let plan = crate::plan_view::PlanView::default().of(dir.path(), &lab);
        let answer = read_building(dir.path(), &lab, plan).expect("a created building has a page");
        let rules = answer
            .docs
            .iter()
            .find(|doc| doc.name == city::BUILDING_FILE)
            .expect("the page lost the tab that says what this building may do");
        assert!(rules.text.contains("confidential"), "{}", rules.text);
        assert!(
            !answer.rooms.iter().any(|room| room.starts_with('.')),
            "a reserved subtree is not a room: {:?}",
            answer.rooms
        );
    }

    /// A plan nobody can open and a plan somebody wrote badly are two
    /// different facts, and the page used to state the second one.
    ///
    /// Reading the file as empty runs it through `check_roadmap_shape`,
    /// which finds no header row and answers `no six-column table
    /// found`. That sends a person to edit a table when what they have
    /// to fix is a file that will not open - the same misreport R2.06
    /// removed from the dispatch path, still standing on the page.
    #[test]
    fn a_building_page_says_the_plan_cannot_be_read_rather_than_that_it_is_malformed() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        let lab = Address::parse("lab").unwrap();
        city::create_building(dir.path(), &lab, city::BuildingTemplate::Minimal).unwrap();
        // A directory where the plan belongs, so the read fails for a
        // reason that is not "it is not there yet".
        let plan = city::roadmap_path(dir.path(), &lab);
        let _ = std::fs::remove_file(&plan);
        std::fs::create_dir_all(&plan).unwrap();

        let plan = crate::plan_view::PlanView::default().of(dir.path(), &lab);
        let answer =
            read_building(dir.path(), &lab, plan).expect("the building is still a building");
        assert!(
            answer
                .problems
                .iter()
                .any(|problem| problem.contains(city::ROADMAP_FILE)),
            "the page has to name the file a person must fix: {:?}",
            answer.problems
        );
    }

    #[test]
    fn a_city_where_the_building_is_gone_hears_nothing_and_says_so() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        std::fs::write(
            city::watch_path(dir.path()),
            "[[source]]
name = \"github\"
matches = \"pr\"
addr = \"gone/room1\"
",
        )
        .unwrap();
        let (base_url, provider) = fake_openai(&["m-local"], vec![completion("unused", None)]);
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        // Not a refusal: nothing listening is a fact about the city, and
        // the person who wrote the table is the one who can act on it.
        worker
            .handle(channels::Command::Wake {
                source: "github".to_owned(),
                subject: "pr opened".to_owned(),
                body: "x".to_owned(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::new(0), b"wake"),
            })
            .unwrap();
        drop(provider);
    }

    /// `[sandbox]` and `[mcp]` resolve city -> building -> room and
    /// nothing wrote either, so a person was governed by settings they
    /// could not change without a text editor.
    #[test]
    fn a_building_can_be_told_what_its_runs_may_reach() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        let mut worker = RunWorker::new(
            dir.path(),
            gateway::Custodian::in_memory(),
            runtime::diagnostics::Diagnostics::off(),
        )
        .unwrap();
        worker
            .handle(channels::Command::CreateBuilding {
                addr: Address::parse("lab").unwrap(),
                template: channels::TemplateName::parse("minimal").unwrap(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"create"),
            })
            .unwrap();
        let room = Address::parse("lab/room1").unwrap();
        let before = city::load_config(dir.path(), &room).unwrap();
        assert!(!before.sandbox.shell, "the shell arm is off by default");

        worker
            .handle(channels::Command::ConfigureBuilding {
                addr: room.clone(),
                sandbox: Some(kernel::SandboxLimits {
                    shell: true,
                    fuel: 4096,
                    mounts: vec![Address::parse("lab/shared").unwrap()],
                }),
                mcp: Some(vec![kernel::McpServer {
                    label: kernel::ServerLabel::parse("docs").unwrap(),
                    transport: kernel::McpTransport::Http {
                        url: "https://mcp.example/v1".to_owned(),
                        header: None,
                    },
                }]),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"reach"),
            })
            .unwrap();

        // The ladder is the authority: what a run in the room resolves
        // to is what the building's own rung now says.
        let after = city::load_config(dir.path(), &room).unwrap();
        assert!(after.sandbox.shell);
        assert_eq!(after.sandbox.fuel, 4096);
        assert_eq!(after.mcp.len(), 1);
        assert_eq!(after.mcp[0].label.as_str(), "docs");

        // And the page reads the building's own rung back, not the
        // resolved value, so saving twice does not copy the city's
        // settings down into the building.
        let lab = Address::parse("lab").unwrap();
        let plan = crate::plan_view::PlanView::default().of(dir.path(), &lab);
        let shown = read_building(dir.path(), &lab, plan).expect("the building page has an answer");
        assert_eq!(shown.mcp.len(), 1);
        assert!(shown.sandbox.is_some_and(|limits| limits.shell));
    }

    /// A building's rules are a governance document, and asking a
    /// person to type one by hand is the wrong door. An agent drafts
    /// them; the person is shown the proposal and allows it; the file
    /// lands in the reserved subtree that no write domain reaches.
    #[test]
    fn a_building_can_be_asked_to_rewrite_its_own_rules_and_the_person_decides() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        let proposal = serde_json::json!({
            "op": "propose",
            "text": "# lab\n\nconfidential: false\nreview: true\n\n## Write domain\n\n- lab\n",
        });
        let (base_url, _provider) = fake_openai(
            &["m-local"],
            vec![
                completion_with("drafting the rules", "rules", "tu_1", proposal.clone()),
                completion("waiting on a person", None),
                completion_with("drafting the rules", "rules", "tu_2", proposal),
                completion("done", None),
            ],
        );
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        worker
            .handle(channels::Command::CreateBuilding {
                addr: Address::parse("lab").unwrap(),
                template: channels::TemplateName::parse("minimal").unwrap(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"create"),
            })
            .unwrap();
        let before = city::load(dir.path(), &Address::parse("lab").unwrap()).unwrap();
        assert!(!before.review(), "the template does not ask for review");

        worker
            .handle(channels::Command::Dispatch {
                addr: Address::parse("lab/room1").unwrap(),
                task: "this building's work needs checking before it lands".to_owned(),
                goal: "the rules say so, then stop".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"dispatch"),
                session: None,
                effort: None,
            })
            .unwrap();
        assert!(
            !city::load(dir.path(), &Address::parse("lab").unwrap())
                .unwrap()
                .review(),
            "a building rewrote its own rules without anybody being asked"
        );

        let waiting = worker
            .governance
            .pending
            .values()
            .next()
            .cloned()
            .expect("the person was never asked");
        assert_eq!(waiting.cluster_key.class, kernel::ApprovalClass::Governance);
        assert!(
            waiting.action_desc.contains("review: true"),
            "the person is shown what they are allowing: {}",
            waiting.action_desc
        );
        allow_the_one_pending_item(&mut worker);

        let after = city::load(dir.path(), &Address::parse("lab").unwrap()).unwrap();
        assert!(after.review(), "the allowed proposal never landed");
        assert!(
            city::building_path(dir.path(), &Address::parse("lab").unwrap())
                .to_string_lossy()
                .contains(".sprawling"),
            "the rules live where no write domain reaches"
        );
    }
}
