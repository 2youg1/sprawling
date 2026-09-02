// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! Main's assembly point — the dirtiest component and the only
//! omniscient one: it knows every concrete type, and nothing knows it.
//! Ledger handle, clock source, RNG seed and spawn points are injected
//! from here and nowhere else; citysim is the second Main.
//!
//! The clock is sampled *here only* (determinism rule 2): every callee
//! takes time as a parameter, and the sample stays in this file so that
//! the rule keeps naming one place.
//!
//! **This file holds the worker and the modules below hold its methods.**
//! `RunWorker` is declared here, so its twenty-two private fields are
//! visible throughout `assembly` and nowhere else — a private item
//! reaches the module that declares it and that module's descendants, so
//! the split cost no field its privacy. What stays here is what every
//! submodule needs and no submodule owns: the worker itself, the one
//! clock sample, the record it appends, the two hooks a live control
//! surface installs, and the door a `Command` enters by.
//!
//! The `use` block below is the one place the sixteen submodules see
//! each other through. A submodule imports from `super`, never from a
//! sibling, so what one part of the assembly point offers another is
//! stated once, here, and reads as a list rather than as a graph.

mod building_page;
mod commanding;
mod credentials;
mod dispatching;
mod driving;
mod folds;
mod freezing;
mod genesis;
mod mcp;
mod naming;
mod plans;
mod reviewing;
mod settling;
mod waking;
mod workbench;

pub(crate) use building_page::read_building;
use credentials::{Ceilings, Chosen, Entered};
use dispatching::{Agreed, Assignment, DISPATCH_TURN_BUDGET, Given, Knock, run_id_for};
pub(crate) use dispatching::{Dispatched, acp_dispatch};
use driving::{Driven, Driving};
use folds::{Governance, HALTED, RELEASED, artifact_of, new_inbox};
pub(crate) use folds::{Standing, rebuild_views};
pub(crate) use genesis::city_address;
use genesis::city_segment;
pub use genesis::{Adopt, InitReport, form_city, has_history, init_city};
use mcp::{PYTHON_WASM_ENV, connect_mcp, mounts_under, transport_site};
use naming::{
    autonomy_name, building_of, mode_of, name_of, not_built, plan_node_of, read_autonomy,
    scope_name,
};
use plans::Reporter;
use settling::{Ending, Sweep};
use workbench::{CITY_VERIFIER, Desks, Site, Workbench};

use crate::effect;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use kernel::{Address, AxCode, AxError, EventDraft, EventKind};
use kernel::{EventRecord, Ledger, Locator, Payload, RunId, TimeMs};
use memory::{Cas, JsonlLedger};
use runtime::Interrupt;

use crate::serving::Posted;

/// The single sanctioned sampling point (clippy.toml disallowed-methods). Everything below this call takes `TimeMs` as a
/// parameter.
pub(crate) fn now_ms() -> Result<TimeMs, AxError> {
    #[expect(
        clippy::disallowed_methods,
        reason = "the one sampling point: Main injects time"
    )]
    let elapsed = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|err| {
            AxError::failure(AxCode::ConfigInvalid, "sample wall clock", err.to_string())
                .with_recovery("fix the system clock; it reads before the unix epoch")
        })?;
    let millis = u64::try_from(elapsed.as_millis()).map_err(|_| {
        AxError::failure(
            AxCode::ConfigInvalid,
            "sample wall clock",
            "beyond u64 millis",
        )
    })?;
    Ok(TimeMs::new(millis))
}

/// Where a city keeps its ledger: under the reserved prefix, outside
/// every WriteDomain (C17).
pub(crate) fn ledger_dir(city_root: &Path) -> PathBuf {
    city_root.join(".sprawling").join("ledger")
}

/// Runs the work a Command asks for. It owns the ledger, so the city has
/// one writer; commands reach it through a channel, and the socket task
/// that accepted them is free again immediately.
/// What the startup scan found and repaired.
pub struct ScanReport {
    pub(crate) lines: usize,
    pub(crate) closed_calls: usize,
    /// The one count a caller branches on rather than prints: `resume`
    /// adds a line telling the person where to answer. `lines` and
    /// `closed_calls` reach nobody outside `summary`, so they stay in.
    pub waiting_approvals: usize,
}

impl ScanReport {
    /// One line a person reads: what was verified, what was closed, and
    /// what is still owed an answer.
    #[must_use]
    pub fn summary(&self) -> String {
        format!(
            "{} line(s) verified; {} unknown-outcome call(s) closed; {} approval(s) waiting",
            self.lines, self.closed_calls, self.waiting_approvals
        )
    }
}

pub struct RunWorker {
    city_root: PathBuf,
    ledger: JsonlLedger,
    cas: Cas,
    /// Every endpoint the person attached and every model they chose,
    /// folded from the ledger. The worker keeps its own copy because a
    /// dispatch needs it synchronously, before the record it just wrote
    /// has reached any observer.
    book: gateway::EndpointBook,
    /// The vault. Shared because a redemption closure outlives the call
    /// that builds it; the lock is held for one resolve at a time.
    vault: Arc<std::sync::Mutex<gateway::Custodian>>,
    /// What a running dispatch asks at its safe points. `None` in a
    /// worker driven one command at a time, which is every worker except
    /// the one behind a live control surface.
    interrupts: Option<Box<dyn FnMut(RunId) -> Interrupt + Send>>,
    /// Where a model's text goes while it is still arriving. `None` in
    /// every worker but the one behind a live control surface, and that
    /// is the switch: a run whose city has nobody watching asks its
    /// provider for no stream at all, so replay and citysim take the
    /// byte-identical path they always took.
    watching: Option<std::sync::Arc<dyn Fn(channels::Delta) + Send + Sync>>,
    /// What waits for a person, who may answer it, what has been
    /// allowed, which scopes are shut, and what each waiting item is
    /// holding up. The worker keeps its own copy for the same reason it
    /// keeps the endpoint book: an answer is decided synchronously,
    /// before the record it just wrote has reached any observer.
    governance: Governance,
    /// What is waiting for each room, folded from the signal records.
    /// A dispatch lends its room's queue to the signal tool and takes it
    /// back when the drive ends, so exactly one queue exists per room.
    pub(super) inboxes: std::collections::BTreeMap<Address, collab::Inbox>,
    /// What each room already got back from work it handed down. Kept
    /// beside the inboxes because it is folded from the same lines and
    /// belongs to the same room.
    pub(super) joins: std::collections::BTreeMap<Address, collab::FanIn>,
    /// The requests waiting for someone to check them, folded from the
    /// pull request records.
    pub(super) requests: Vec<collab::OpenRequest>,
    /// The ground residents have claimed, folded from `goal_registered`
    /// in the order the claims were made — which is the order the
    /// conflict check reads them in.
    pub(super) goals: Vec<kernel::GoalEntry>,
    /// What each building is working towards, and the depth-zero
    /// position that lets one be declared. Held by the worker because
    /// the worker is what acts on it; rebuilt from the records on open,
    /// like the endpoint book and the goal register beside it.
    pursuits: std::collections::BTreeMap<Address, kernel::Pursuit>,
    delegator: kernel::Delegator,
    /// Which room holds each node of each building's plan, folded from
    /// the claim records and kept up to date as this worker writes them.
    plan_holders:
        std::collections::BTreeMap<Address, std::collections::BTreeMap<kernel::NodeId, String>>,
    /// The instant the schedule was last read against. Set when the
    /// worker opens, so a city that was off owes nothing for the time it
    /// was off.
    last_tick: TimeMs,
    /// Whether the run being dispatched began with somebody else's text.
    /// Set for the length of one dispatch by `wake`; it decides whether
    /// the approvals that run raises can be waived by a policy.
    tainted_arrival: bool,
    /// When each subscription credential stops working, by provider.
    /// Folded from the capture records, so a restarted city renews on
    /// the same schedule rather than discovering expiry through a 401.
    expiries: std::collections::BTreeMap<String, u64>,
    /// Logins begun and not yet redeemed, by provider. Held in memory
    /// on purpose: a PKCE verifier proves that the process which asked
    /// is the process which redeems, so a verifier that outlived the
    /// process would be proving nothing. A restart means starting the
    /// login again, which is one browser visit.
    logins: std::collections::BTreeMap<String, gateway::OauthPending>,
    /// The diagnostic log. Write-only, and nothing here reads it back:
    /// turning it off must leave the ledger byte-identical.
    log: runtime::diagnostics::Diagnostics,
    /// Residents who were spoken to while nobody was home. Held between
    /// the run that spoke and the runs that answer, because delivery
    /// happens after the speaker has frozen.
    knocks: Vec<Knock>,
}

impl RunWorker {
    /// # Errors
    /// Propagates whatever opening the ledger or the store reports, and
    /// whatever the ledger says about its own chain: a worker that
    /// cannot read the city's history cannot know what is attached.
    pub fn new(
        city_root: &Path,
        vault: gateway::Custodian,
        log: runtime::diagnostics::Diagnostics,
    ) -> Result<Self, AxError> {
        let dir = ledger_dir(city_root);
        let (ledger, _report) =
            JsonlLedger::open(&dir, now_ms()?).map_err(memory::MemoryError::into_ax)?;
        RunWorker::over(city_root, vault, log, ledger)
    }

    /// Builds a worker around a ledger somebody else opened.
    ///
    /// Where the history comes from is not this worker's decision to
    /// make, and taking it as a parameter is the same correction
    /// ARCHITECTURE.md section 3 already asks for on the model adapter:
    /// a component that builds its own dependency cannot be driven
    /// against a second one. The ledger is a concrete `JsonlLedger` on
    /// both paths - the `Vfs` underneath it is what differs - so nothing
    /// here becomes a seam and no `pub trait` moves.
    ///
    /// # Errors
    /// Propagates whatever opening the store reports, and whatever the
    /// ledger says about its own chain: a worker that cannot read the
    /// city's history cannot know what is attached.
    pub(crate) fn over(
        city_root: &Path,
        vault: gateway::Custodian,
        log: runtime::diagnostics::Diagnostics,
        ledger: JsonlLedger,
    ) -> Result<Self, AxError> {
        let now = now_ms()?;
        let dir = ledger_dir(city_root);
        let Standing {
            book,
            governance,
            collaboration,
        } = Standing::fold(&dir)?;
        let cas = Cas::open(&city_root.join(".sprawling").join("cas"))
            .map_err(memory::MemoryError::into_ax)?;
        // The one place a `Delegator` is minted in this process, which
        // is what makes "a sub-agent cannot set the city working" a
        // fact about the code rather than a rule somebody follows.
        let delegator = kernel::Delegator::root();
        let pursuits = collaboration.pursuits(&delegator);
        Ok(RunWorker {
            city_root: city_root.to_path_buf(),
            ledger,
            cas,
            book,
            vault: Arc::new(std::sync::Mutex::new(vault)),
            interrupts: None,
            watching: None,
            governance,
            inboxes: collaboration.inboxes,
            joins: collaboration.joins,
            pursuits,
            plan_holders: collaboration.plan_holders,
            goals: collaboration.goals,
            requests: collaboration.requests,
            delegator,
            last_tick: now,
            tainted_arrival: false,
            expiries: std::collections::BTreeMap::new(),
            logins: std::collections::BTreeMap::new(),
            log,
            knocks: Vec::new(),
        })
    }

    /// Writes one diagnostic line, anchored to where the ledger stands.
    fn note(&mut self, level: runtime::diagnostics::Level, module: &str, message: &str) {
        let site = runtime::diagnostics::Site {
            run: RunId::CITY,
            seq: self.ledger.position(),
            module,
        };
        self.log.write(level, site, message);
    }

    /// Appends one city record and folds it into the worker's own book.
    /// The append comes first: the book states what the history says,
    /// never what the process hoped to write.
    fn record(&mut self, kind: EventKind, data: Payload) -> Result<(), AxError> {
        self.record_where(kind, None, data)
    }

    /// The same, for a record that belongs to one address. A pursuit is
    /// a building's, and a record with no address would be a fact about
    /// the city that no view could file under the building it changed.
    fn record_at(&mut self, kind: EventKind, addr: Address, data: Payload) -> Result<(), AxError> {
        self.record_where(kind, Some(addr), data)
    }

    fn record_where(
        &mut self,
        kind: EventKind,
        addr: Option<Address>,
        data: Payload,
    ) -> Result<(), AxError> {
        let draft = EventDraft {
            run: RunId::CITY,
            t: now_ms()?,
            who: "owner".to_owned(),
            addr,
            kind,
            data: data.clone(),
            ig: false,
        };
        self.ledger.append(draft)?;
        self.governance.absorb(kind, RunId::CITY, None, &data);
        self.book.apply_payload(kind, &data)
    }

    /// Appends one line attributed to a run rather than to the city.
    /// Separate from `record` because that one speaks for the city: an
    /// effect a resident caused must carry the resident's name, or the
    /// history cannot say who spoke.
    fn record_for(&mut self, run: RunId, line: effect::Line) -> Result<(), AxError> {
        let effect::Line {
            who,
            addr,
            kind,
            data,
        } = line;
        self.ledger.append(EventDraft {
            run,
            t: now_ms()?,
            who,
            addr: Some(addr.clone()),
            kind,
            data: data.clone(),
            ig: false,
        })?;
        // The book states what the history says, whoever wrote the line.
        // Without this an approval a run raised was on the ledger and
        // absent from `pending`, so the person could not answer it until
        // the process restarted and folded the ledger again. It is the
        // same fold a restart runs, shown the line this process wrote.
        self.governance.absorb(kind, run, Some(&addr), &data);
        Ok(())
    }

    /// Sends every appended record to `sink` once it is durable.
    pub(crate) fn observe(&mut self, sink: Box<dyn FnMut(&EventRecord) + Send>) {
        self.ledger.observe(sink);
    }

    /// Sends every increment a model produces to `sink`, before the call
    /// it belongs to has settled.
    ///
    /// Separate from [`Self::observe`] because the two carry different
    /// kinds of thing: that one carries history and this one carries a
    /// view of work in progress. A city with nobody watching never
    /// installs one, and a run whose city has none asks its provider for
    /// no stream at all.
    pub(crate) fn watch(&mut self, sink: std::sync::Arc<dyn Fn(channels::Delta) + Send + Sync>) {
        self.watching = Some(sink);
    }

    /// Where a running dispatch asks what arrived. Attached by the serve
    /// wiring, absent in a worker driven command by command: a source
    /// nobody set means a run that nothing interrupts.
    pub(crate) fn attach_interrupts(&mut self, source: Box<dyn FnMut(RunId) -> Interrupt + Send>) {
        self.interrupts = Some(source);
    }

    /// Closes the city in the record, so a stop somebody chose and a
    /// stop that was a crash are different lines rather than the same
    /// silence.
    ///
    /// The five sections are the city's own: what the next session must
    /// read is the city's norms, and where it left off is the position
    /// the ledger stands at. Written through `runtime::handoff`, which
    /// is the one construction point for the shape - a hand-built
    /// payload here would be a second one.
    ///
    /// # Errors
    /// Propagates the handoff's refusal of an empty must-read list, and
    /// the ledger's refusal to take the line.
    pub(crate) fn close_city(&mut self) -> Result<(), AxError> {
        // The city's own norm, not a building's: `city::norms` answers
        // for a run at an address, and this line belongs to the city.
        // Through the same reader the prefix uses. What this city's
        // norms are has one answer, and hashing zero bytes here made the
        // must-read locator point at nothing while the handoff went on
        // saying the next session must read them.
        let mut must_read = Vec::new();
        let bytes = city_segment(&self.city_root)?;
        let hash = self.cas.put(&bytes).map_err(memory::MemoryError::into_ax)?;
        must_read.push(Locator::parse(&format!("cas:b3-{hash}"))?);
        let standing = self.ledger.position();
        let handoff = runtime::handoff::Handoff::new(
            must_read,
            "the city was closed by the person running it".to_owned(),
            format!("the ledger stands at {}", standing.value()),
            "an orderly close, not a crash: nothing was interrupted mid-command".to_owned(),
            "`sprawling serve` on this directory continues from here".to_owned(),
        )?;
        self.note(
            runtime::diagnostics::Level::Effect,
            "bin::assembly",
            "the city is closing; its handoff is on the ledger",
        );
        self.record(EventKind::HandoffWritten, handoff.payload()?)
    }

    /// # Errors
    /// Refuses a command this stage does not run yet, naming what does.
    pub fn handle(&mut self, command: channels::Command) -> Result<(), AxError> {
        let name = command.name();
        let outcome = self.run_command(command);
        if let Err(err) = &outcome {
            // A refused command is the first thing a person asks about,
            // so it is written at the default floor. It is written here
            // rather than at the caller, because every caller wants it.
            self.note(
                runtime::diagnostics::Level::Refuse,
                "bin::assembly",
                &format!("{name} refused: {err}; {}", err.recovery()),
            );
        }
        outcome
    }

    /// Runs one command from the desk, refusal included.
    ///
    /// The one authority for what becomes of a command a person sent:
    /// it runs, and if it is refused the refusal goes both to the
    /// diagnostic log and to whoever asked. Before this existed the
    /// worker loop wrote `let _ = handle(command)`, so every refusal
    /// died in the log and the page that caused it said nothing.
    pub(crate) fn serve_one(&mut self, posted: Posted) {
        let Posted { command, reply } = posted;
        if let Err(err) = self.handle(command) {
            self.hand_back(&reply, err);
        }
    }

    /// Hands a refusal to whoever asked for the command.
    ///
    /// `handle` has already written it to the diagnostic log, so the
    /// only case that earns a second line is the one a reader would
    /// otherwise misread: somebody did ask, and the answer arrived at a
    /// socket that had already closed.
    fn hand_back(&mut self, reply: &channels::Reply, error: AxError) {
        match reply.refuse(error) {
            channels::Delivered::ToThePeer | channels::Delivered::NobodyAsked => {}
            channels::Delivered::PeerGone => self.note(
                runtime::diagnostics::Level::Refuse,
                "bin::assembly",
                "the refusal above reached nobody: the peer that asked had closed its socket",
            ),
        }
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
pub(super) mod fixture {
    use super::*;

    /// Lays a building's rules where the city reads them.
    ///
    /// Through `city::building_path` rather than by joining a file name:
    /// a fixture that spells the path itself is a second authority for
    /// where the rules live, and it goes on passing after the real one
    /// has moved.
    pub(super) fn lay_rules(city_root: &Path, building: &str, text: &str) {
        let addr = Address::parse(building).unwrap();
        let file = city::building_path(city_root, &addr);
        std::fs::create_dir_all(file.parent().unwrap()).unwrap();
        std::fs::write(file, text).unwrap();
    }

    /// A loopback provider that answers a model list and then a fixed
    /// number of chat completions. These tests register it the way a
    /// person would, so nothing here reaches the worker by a door the
    /// production path does not have.
    /// A fake provider and what it was asked. Tests that only need it to
    /// answer bind it as `_provider`; tests about what went out on the
    /// wire read `bodies()`, because the request body is the only place
    /// a claim about the wire can be checked.
    pub(super) struct FakeProvider {
        seen: std::sync::Arc<std::sync::Mutex<Vec<String>>>,
        _handle: std::thread::JoinHandle<()>,
    }

    impl FakeProvider {
        pub(super) fn bodies(&self) -> Vec<String> {
            self.seen
                .lock()
                .unwrap()
                .iter()
                .map(|head| {
                    head.split_once("\r\n\r\n")
                        .map_or(String::new(), |(_, body)| body.to_owned())
                })
                .collect()
        }

        pub(super) fn exchanges(&self) -> Vec<String> {
            self.seen.lock().unwrap().clone()
        }
    }

    pub(super) fn fake_openai(models: &[&str], replies: Vec<String>) -> (String, FakeProvider) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        let list = serde_json::json!({
            "data": models
                .iter()
                .map(|id| serde_json::json!({ "id": id }))
                .collect::<Vec<_>>(),
        })
        .to_string();
        let seen = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let recorder = std::sync::Arc::clone(&seen);
        let handle = std::thread::spawn(move || {
            // The last reply repeats: a test says what the interesting
            // turns are, not how many turns the loop will take.
            let mut chats = replies.into_iter().peekable();
            let mut last = String::new();
            // One bad socket ends that socket, not the server. A client
            // is free to reset a connection at any point, including
            // before the accept completes, and a server that returns on
            // it takes every later turn of the script with it. The bound
            // is there so a listener that is genuinely gone stops rather
            // than spins.
            let mut refused = 0_u32;
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else {
                    refused = refused.saturating_add(1);
                    if refused > 64 {
                        return;
                    }
                    continue;
                };
                refused = 0;
                let mut head = String::new();
                let mut buf = [0u8; 4096];
                let mut whole = false;
                // A read that errors ends this request, not the loop
                // that serves the next one.
                while let Ok(n) = std::io::Read::read(&mut stream, &mut buf) {
                    if n == 0 {
                        break;
                    }
                    head.push_str(&String::from_utf8_lossy(&buf[..n]));
                    if let Some(end) = head.find("\r\n\r\n") {
                        let want = head
                            .lines()
                            .find_map(|line| {
                                line.to_ascii_lowercase()
                                    .strip_prefix("content-length: ")
                                    .and_then(|v| v.trim().parse::<usize>().ok())
                            })
                            .unwrap_or(0);
                        let body_seen = head.len().saturating_sub(end.saturating_add(4));
                        if body_seen >= want {
                            whole = true;
                            break;
                        }
                    }
                }
                // What counts as a request is settled here and nowhere
                // else. It used to be settled twice - the record asked
                // for a header terminator and the reply asked for
                // nothing at all - so a socket that carried no request
                // stayed off the record and still spent a scripted
                // reply, and every turn after it answered the question
                // before it.
                if !whole {
                    continue;
                }
                // The whole exchange, headers included: a test about
                // what went out on the wire needs the headers too.
                recorder.lock().unwrap().push(head.clone());
                let body = if head.starts_with("GET ") {
                    list.clone()
                } else {
                    match chats.next() {
                        Some(reply) => {
                            last.clone_from(&reply);
                            reply
                        }
                        None => last.clone(),
                    }
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = std::io::Write::write_all(&mut stream, response.as_bytes());
            }
        });
        (
            format!("http://{addr}/v1"),
            FakeProvider {
                seen,
                _handle: handle,
            },
        )
    }

    /// One OpenAI chat completion: `calls` decides whether the turn asks
    /// for the edit tool or ends. The call uses the edit tool's real
    /// contract (create form), so these tests exercise the same argument
    /// shape a model is told about - an invented shape here once hid the
    /// fact that no canary edit had ever landed on disk.
    /// One reply that calls a named tool with the arguments given.
    pub(super) fn completion_with(
        text: &str,
        tool: &str,
        id: &str,
        arguments: serde_json::Value,
    ) -> String {
        serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": text,
                    "tool_calls": [{
                        "id": id,
                        "type": "function",
                        "function": { "name": tool, "arguments": arguments.to_string() },
                    }],
                },
                "finish_reason": "tool_calls",
            }],
            "usage": { "prompt_tokens": 12, "completion_tokens": 5 },
        })
        .to_string()
    }

    pub(super) fn completion(text: &str, call: Option<(&str, &str)>) -> String {
        let mut message = serde_json::json!({ "role": "assistant", "content": text });
        let mut finish = "stop";
        if let Some((id, path)) = call {
            let arguments = serde_json::json!({
                "path": path,
                "base_version": "new",
                "old": "",
                "new": "noted\n",
            })
            .to_string();
            message["tool_calls"] = serde_json::json!([{
                "id": id,
                "type": "function",
                "function": { "name": "edit", "arguments": arguments },
            }]);
            finish = "tool_calls";
        }
        serde_json::json!({
            "choices": [{ "message": message, "finish_reason": finish }],
            "usage": { "prompt_tokens": 12, "completion_tokens": 5 },
        })
        .to_string()
    }

    /// A worker with one endpoint attached and one model chosen, exactly
    /// as the settings page would leave it.
    pub(super) fn worker_with_provider(
        city_root: &Path,
        base_url: &str,
        model: &str,
    ) -> Result<RunWorker, AxError> {
        let mut worker = RunWorker::new(
            city_root,
            gateway::Custodian::in_memory(),
            runtime::diagnostics::Diagnostics::off(),
        )?;
        worker.handle(channels::Command::AttachEndpoint {
            name: channels::ProviderName::parse("house").unwrap(),
            base_url: base_url.to_owned(),
            dialect: kernel::DialectKind::OpenAi,
            secret: None,
            auth_header: None,
            admit: Vec::new(),
            idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"attach"),
        })?;
        worker.handle(channels::Command::SelectModel {
            endpoint: channels::ProviderName::parse("house").unwrap(),
            model: model.to_owned(),
            tag: kernel::ModelTag::Main,
            context_tokens: 32_768,
            max_output_tokens: 4_096,
            idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"select"),
        })?;
        Ok(worker)
    }

    /// One completion that calls a named tool with the given arguments.
    /// Separate from `completion` because that helper hard-codes the edit
    /// tool's shape, and a tool面 test is about a different tool.
    pub(super) fn tool_completion(
        text: &str,
        id: &str,
        tool: &str,
        args: serde_json::Value,
    ) -> String {
        serde_json::json!({
            "choices": [{
                "message": {
                    "role": "assistant",
                    "content": text,
                    "tool_calls": [{
                        "id": id,
                        "type": "function",
                        "function": { "name": tool, "arguments": args.to_string() },
                    }],
                },
                "finish_reason": "tool_calls",
            }],
            "usage": { "prompt_tokens": 12, "completion_tokens": 5 },
        })
        .to_string()
    }

    pub(super) const PLAN_TWO_FREE_ROWS: &str = concat!(
        "| # | Item | Weight | Needs | Status | Evidence |\n",
        "|---|---|---|---|---|---|\n",
        "| 1 | wire the kiln | 1 |  | Not started |  |\n",
        "| 2 | glaze tests | 1 |  | Not started |  |\n",
    );

    pub(super) const PLAN_ONE_FREE_ROW: &str = concat!(
        "| # | Item | Weight | Needs | Status | Evidence |\n",
        "|---|---|---|---|---|---|\n",
        "| 1 | wire the kiln | 1 |  | Not started |  |\n",
    );

    /// Answers the one thing waiting, as the person would. Delegation
    /// now asks before it hands anything down, so a test that wants a
    /// delegate has to say yes first - which is the point of the door.
    pub(super) fn allow_the_one_pending_item(worker: &mut RunWorker) -> kernel::ClusterKey {
        let item = worker
            .governance
            .pending
            .values()
            .next()
            .cloned()
            .expect("exactly one thing is waiting");
        worker
            .handle(channels::Command::Approve {
                item: item.id.clone(),
                verdict: kernel::PolicyVerdict::Allow,
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"allow"),
            })
            .unwrap();
        item.cluster_key
    }
}
