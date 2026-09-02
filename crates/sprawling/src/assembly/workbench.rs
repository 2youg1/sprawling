// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! What one run works at: where it stands, and the bench it is given.

use std::path::{Path, PathBuf};

use kernel::{Address, AxError, EventKind};
use kernel::{Locator, Model, RunId};
use runtime::bench::ToolBench;
use runtime::{EditTool, ExecTool, StatusTool};

use crate::effect;

use super::{
    Assignment, Given, PYTHON_WASM_ENV, RunWorker, autonomy_name, connect_mcp, mounts_under,
    new_inbox, now_ms, run_id_for, transport_site,
};

/// The sandbox this build carries, if it carries one.
///
/// # Errors
/// Propagates what starting the engine reports. A build that says it
/// carries one and cannot start it refuses the dispatch rather than
/// falling back: falling back is how a run that a person believed was
/// sandboxed turns out not to have been.
#[cfg(feature = "sandbox")]
pub(super) fn execution_engine() -> Result<Box<dyn runtime::Sandbox>, AxError> {
    Ok(Box::new(runtime::WasmtimeSandbox::new()?))
}

/// The engine `exec` runs a program in: none, in a build without one.
///
/// # Errors
/// None today; the signature matches the arm that can fail so the call
/// site does not change shape with the feature.
#[cfg(not(feature = "sandbox"))]
pub(super) fn execution_engine() -> Result<Box<dyn runtime::Sandbox>, AxError> {
    Ok(Box::new(runtime::AbsentSandbox))
}

pub(super) fn host_shell() -> Option<std::path::PathBuf> {
    let named = if cfg!(windows) { "COMSPEC" } else { "SHELL" };
    if let Ok(path) = std::env::var(named)
        && !path.is_empty()
    {
        return Some(std::path::PathBuf::from(path));
    }
    let fallback = if cfg!(windows) {
        std::path::PathBuf::from("cmd.exe")
    } else {
        std::path::PathBuf::from("/bin/sh")
    };
    Some(fallback)
}

/// Who checks a delegate's own done check. Not the delegate: the whole
/// point of `Claim::verified` is that a producer's verdict on its own
/// work is not verification.
pub(super) const CITY_VERIFIER: &str = "city";

/// Where one run stands before anything is built for it: the rules over
/// it, the model it may reach, who it runs as, and the tree it writes in.
///
/// One value rather than three, because the three readings of this phase
/// interlock: the lease is named after the run, the run's id is minted
/// after a credential renewal that may go to the network, and that
/// renewal is chosen by the building's own rules. Splitting them would
/// move a clock sample, and where the one sampling point lands is not
/// something a structural change may alter (ARCHITECTURE.md section 10).
pub(super) struct Site {
    pub(super) building: city::Building,
    pub(super) rules: city::BuildingRules,
    /// City, building and resident layers, resolved once and frozen for
    /// the whole run.
    pub(super) config: kernel::FrozenConfig,
    pub(super) model: gateway::ModelEntry,
    pub(super) adapter: Box<dyn Model + Send>,
    pub(super) identity: city::Identity,
    pub(super) who: String,
    pub(super) run_id: RunId,
    /// Some when the building asks for review: the tree this run writes
    /// in, which goes back whether the run finished or failed.
    pub(super) lease: Option<memory::WorktreeLease>,
    pub(super) write_root: PathBuf,
    branch: Option<String>,
}

/// What the model may see, what routes what it calls, and who it may
/// hand work down to.
///
/// `ToolBench` routes one call; this is the whole bench a run works at:
/// the catalogue the model was told about, the router behind it, and
/// the delegate desk two of the tools write to. The catalogue is kept
/// rather than the tool definitions it renders, so there is one answer
/// to what this run admits rather than a list and a copy of it.
pub(super) struct Workbench {
    pub(super) catalog: std::rc::Rc<std::cell::RefCell<runtime::Catalog>>,
    pub(super) bench: ToolBench,
    pub(super) delegates: std::rc::Rc<std::cell::RefCell<collab::DelegateDesk>>,
}

/// Who this run can reach: the residents beside it, and the sub-agents
/// under it.
///
/// Both are read by one answer - what `status` tells the model about the
/// city around it - so they arrive as one value rather than as two
/// parameters a caller could hand over half of.
pub(super) struct Reach<'a> {
    pub(super) seen: &'a city::Neighbourhood,
    pub(super) delegates: &'a std::rc::Rc<std::cell::RefCell<collab::DelegateDesk>>,
}

impl Site {
    /// What a checkpoint fence covers for this run.
    ///
    /// Under review the worktree is this run's alone, so everything that
    /// changed inside it is this run's to offer - the shelf entries it
    /// filed included, which sit at the building rather than in the
    /// room. Without a lease the fence stays on the room, which is the
    /// only place a run may write in the city itself.
    pub(super) fn fence_scope(&self, addr: &Address) -> String {
        if self.lease.is_some() {
            self.building.addr().as_str().to_owned()
        } else {
            addr.as_str().to_owned()
        }
    }
}

/// The desks one dispatch lends out, and takes back when the drive ends.
///
/// Grouped because they are lent and taken back together: five handles
/// passed side by side are five chances to take four of them back. Four
/// of them settle in one order in `settle_desks`; `pr` settles after,
/// once the run has something to show for itself, and it belongs here
/// all the same - what makes them one value is the lending, not the
/// settling.
pub(super) struct Desks {
    pub(super) signals: std::rc::Rc<std::cell::RefCell<collab::SignalDesk>>,
    pub(super) goals: std::rc::Rc<std::cell::RefCell<collab::GoalDesk>>,
    pub(super) plan: std::rc::Rc<std::cell::RefCell<collab::ClaimDesk>>,
    pub(super) shelf: std::rc::Rc<std::cell::RefCell<collab::ArchiveDesk>>,
    pub(super) pr: std::rc::Rc<std::cell::RefCell<collab::PrDesk>>,
    /// Where the shared plan lives, so the claims that survive are
    /// written back to the file they were checked against.
    pub(super) plan_path: PathBuf,
    /// What was already in the room's queue when it was lent out, which
    /// is what `status` reports as waiting. Read before the queue goes
    /// to the desk, so it is counted here or not at all.
    waiting: u32,
}

/// What a run can be told about itself at the moment it starts.
///
/// Every field here is read from something. Eight of them used to be
/// constants — the mode was always `plan_goal`, the write domain was
/// the room rather than what the building granted, and the budget, the
/// context limit and the locks were zeros. City.md tells a model to call
/// `status` for exactly those, so a model that obeyed got a row of
/// noughts and learnt not to ask again.
///
/// `ctx_used` and `children` stay at their empty values, and both are
/// true: nothing has been read at dispatch, and this city cannot yet
/// make a child. `worktree_disk` is zero because measuring a tree costs
/// a walk of it, and a number nobody has asked for is not worth one.
pub(super) struct Situation<'a> {
    pub(super) addr: &'a Address,
    pub(super) who: &'a str,
    signals_pending: u32,
    mode: runtime::Mode,
    write_domain: &'a kernel::WriteDomain,
    worktree: &'a Path,
    trust: &'a kernel::Autonomy,
    context_tokens: u64,
    pub(super) budget: kernel::BudgetCap,
    locks: Vec<String>,
    neighbours: u32,
}

pub(super) fn status_snapshot(situation: Situation<'_>) -> runtime::StatusSnapshot {
    runtime::StatusSnapshot {
        who: situation.who.to_owned(),
        addr: situation.addr.clone(),
        mode: situation.mode,
        ctx_used: kernel::Tokens::default(),
        ctx_limit: kernel::Tokens::new(situation.context_tokens),
        budget_usd: situation.budget.usd,
        budget_tokens: situation.budget.tokens,
        trust: autonomy_name(situation.trust),
        write_domain: situation
            .write_domain
            .prefixes()
            .map(|prefix| prefix.as_str().to_owned())
            .collect::<Vec<String>>()
            .join(", "),
        locks: situation.locks,
        worktree_path: situation.worktree.display().to_string(),
        worktree_disk: kernel::ByteLen::default(),
        signals_pending: situation.signals_pending,
        now: None,
        provider_mode: runtime::ProviderMode::Normal,
        neighbours: situation.neighbours,
    }
}

impl RunWorker {
    /// Settles where this run stands, before anything is built for it.
    ///
    /// The building's rules, the model behind them, who the run is, and
    /// the tree it writes in are one phase rather than three, because
    /// they interlock: a lease is named after the run, the run's id is
    /// minted after a credential renewal that may reach the network, and
    /// which credential that is comes from the rules. Cutting them apart
    /// would move a clock sample, and the single sampling point is not
    /// something a structural change may relocate.
    ///
    /// # Errors
    /// Propagates a building that cannot be read, rules or configuration
    /// that will not load, a tag with no model behind it, a credential
    /// that will not renew, and whatever the checkpoint or the worktree
    /// says about lending a tree out.
    pub(super) fn stand_up(&mut self, at: &Assignment, given: &Given) -> Result<Site, AxError> {
        let addr = &at.addr;
        // The building's own rules decide which models this run may
        // reach, so they are read before one is chosen.
        let building = city::Building::of(addr)?;
        let rules = city::load(&self.city_root, building.addr())?;
        // City, building and resident layers, resolved once and frozen
        // for the whole run: re-reading them mid-run would let the two
        // halves of one session be shaped by two different settings.
        let config = city::load_config(&self.city_root, addr)?;
        let chosen = self.book.select(kernel::ModelTag::Main, rules.policy())?;
        // A subscription credential that expires mid-run is a run that
        // dies on its second turn, so it is renewed before the run
        // starts rather than after a call comes back refused. The
        // endpoint a login attached carries the provider's own name.
        self.renew_if_stale(&chosen.endpoint.name.clone())?;
        let chosen = self.book.select(kernel::ModelTag::Main, rules.policy())?;
        let model = chosen.entry.clone();
        let adapter = self.adapter_for(&chosen)?;

        // Who runs this: the address's own URBANITE.md when it has one,
        // and an ephemeral worker when it does not. The identity supplies
        // the resident segment, so the same resident reads the same
        // instructions on every run and the prefix stays cacheable across
        // its whole life.
        let identity = city::Identity::load(&self.city_root, addr)?;
        let who = identity.who();

        // The run's identity is fixed before the tools are built: three
        // of them mint ids from it, and an id minted from a run that did
        // not exist yet would not be the same id on a replay.
        let run_id = run_id_for(&given.job, addr, now_ms()?);
        // What this run was sent to do, held for as long as anything it
        // raises is still waiting. `run_started` carries the same three
        // facts and a restarted worker folds them from there; this is the
        // live half, registered out of the values the plan below is built
        // from so the two cannot say different things.
        self.governance
            .sent(run_id, &given.task, &given.goal, at.budget);

        // A building under review gives every run its own tree, and the
        // run writes there instead of in the city. Nothing it writes is
        // visible until somebody else checks it — the losing line of the
        // design made physical rather than promised.
        //
        // The fence goes up first: a worktree branches from a commit, so
        // the city needs one before it can lend anything out.
        let mut lease = None;
        if rules.review() {
            memory::Checkpoint::open(&self.city_root)
                .map_err(memory::MemoryError::into_ax)?
                .ensure_base(addr.as_str(), now_ms()?, &who)
                .map_err(memory::MemoryError::into_ax)?;
            let trees =
                memory::Worktrees::open(&self.city_root).map_err(memory::MemoryError::into_ax)?;
            let name = memory::WorktreeName::parse(&run_id.to_string())
                .map_err(memory::MemoryError::into_ax)?;
            let claimed = trees.claim(&name).map_err(memory::MemoryError::into_ax)?;
            self.record_for(
                run_id,
                effect::Line {
                    who: who.to_owned(),
                    addr: addr.clone(),
                    kind: EventKind::WorktreeOpened,
                    data: claimed
                        .opened_payload()
                        .map_err(memory::MemoryError::into_ax)?,
                },
            )?;
            lease = Some(claimed);
        }
        let write_root = lease
            .as_ref()
            .map_or_else(|| self.city_root.clone(), |held| held.path().to_path_buf());
        let branch = lease.as_ref().map(|held| held.name().as_str().to_owned());
        Ok(Site {
            building,
            rules,
            config,
            model,
            adapter,
            identity,
            who,
            run_id,
            lease,
            write_root,
            branch,
        })
    }

    /// Opens the desks this run works at, and lends them what they hold.
    ///
    /// Every desk is a place a tool writes to and the settlement reads
    /// back, so they open together and come back as one value. The
    /// room's queue moves into the signal desk rather than being copied
    /// there: one queue per room at all times, and a copy would be a
    /// second answer to what arrived first.
    ///
    /// # Errors
    /// Propagates a plan that cannot be read and a shelf that cannot be
    /// indexed - both before a model is called, since a run built on
    /// either would spend a call to produce claims the city was always
    /// going to drop.
    pub(super) fn open_desks(&mut self, site: &Site, addr: &Address) -> Result<Desks, AxError> {
        let pr = std::rc::Rc::new(std::cell::RefCell::new(collab::PrDesk::new(
            site.who.clone(),
            addr.clone(),
            site.branch.clone(),
            site.branch
                .as_deref()
                .and_then(|name| collab::NodeId::parse(name).ok()),
            self.requests.clone(),
        )));

        // The room's queue is lent to the desk for the length of the
        // drive and taken back below. One queue exists per room at all
        // times; a copy would be a second answer to "what arrived first".
        let lent = self.inboxes.remove(addr).unwrap_or_else(new_inbox);
        let waiting = lent.pending();
        let signals = std::rc::Rc::new(std::cell::RefCell::new(collab::SignalDesk::new(
            site.run_id,
            addr.clone(),
            site.who.clone(),
            site.building.addr().clone(),
            now_ms()?,
            lent,
        )));
        let goals = std::rc::Rc::new(std::cell::RefCell::new(collab::GoalDesk::new(
            site.run_id,
            site.who.clone(),
            self.goals.clone(),
        )));

        // The plan is shared ground, so it is read from and written back
        // to the city even when the run writes everywhere else in its
        // own tree. A claim nobody else can see is not a claim; the work
        // stays private until it is checked, the fact that somebody is
        // doing it does not.
        let plan_path = city::roadmap_path(&self.city_root, site.building.addr());
        // A plan that is not there yet reads as empty; every other reason
        // this file cannot be read is reported here, before a model is
        // called. Reading them as empty spent a call to produce claims
        // that the compare-and-swap below was always going to drop, and
        // told the person a neighbour had moved their row.
        let plan_text = city::roadmap(&self.city_root, site.building.addr())?;
        let plan = std::rc::Rc::new(std::cell::RefCell::new(collab::ClaimDesk::new(
            site.who.clone(),
            addr.clone(),
            plan_text,
        )));

        // What the building already knows, computed from the shelf
        // rather than kept beside it. An index that was stored would be
        // a second copy of what the files say, and the files are the
        // ones that are true.
        // `archive_index` already answers `Ok(empty)` for a building with
        // no shelf, so anything it reports is a real failure and telling
        // the model this building knows nothing would be a lie about it.
        let held: Vec<collab::Held> = city::archive_index(&self.city_root, site.building.addr())?
            .into_iter()
            .map(|entry| collab::Held {
                kind: entry.kind.as_str().to_owned(),
                text: entry.subject,
            })
            .collect();
        let shelf = std::rc::Rc::new(std::cell::RefCell::new(collab::ArchiveDesk::new(
            addr.clone(),
            held,
        )));
        Ok(Desks {
            signals,
            goals,
            plan,
            shelf,
            pr,
            plan_path,
            waiting,
        })
    }

    /// Lays out what the model may see and what routes what it calls.
    ///
    /// The catalogue and the bench are one phase because they are one
    /// registration: the catalogue is what the model was told exists,
    /// the bench is what routes the call it makes, and a name on one
    /// list and not the other is either a tool nobody can call or a
    /// call nobody was told about. The delegate desk comes back with
    /// them because two tools hold it and the settlement reads it after
    /// the drive.
    ///
    /// # Errors
    /// Propagates a write domain that will not resolve, a neighbourhood
    /// that cannot be scanned, any tool that refuses to be built, a
    /// duplicate registration on either list, and whatever the reading
    /// room reports.
    pub(super) fn lay_out_workbench(
        &mut self,
        site: &Site,
        desks: &Desks,
        at: &Assignment,
        job_locator: &Locator,
    ) -> Result<Workbench, AxError> {
        let (addr, mode) = (&at.addr, at.mode);
        // The catalog is the single source of `ChatRequest.tools`: the
        // bench routes a call, the catalog is what the model was told
        // exists, and one registration feeds both.
        //
        // The admitted set is decided here and frozen with the run. It
        // has to be: a provider hashes the tool array ahead of the system
        // prompt, so a tool admitted mid-run would invalidate the whole
        // conversation's cache. Progressive disclosure is about what a
        // line says, not about when a tool appears.
        let catalog = std::rc::Rc::new(std::cell::RefCell::new(runtime::Catalog::new()));
        // The mode a run sits in is a capability like any other: it says
        // what this run admits, and until it was set here the mode's own
        // catalog entry reached no model.
        catalog.borrow_mut().set_mode(mode);
        let edit = EditTool::new(&site.write_root, addr.clone(), site.rules.write_domain()?)?;
        // Who this run can reach, read once at dispatch and frozen with
        // it. Nothing here can move under the run: the assembly is
        // single-threaded, so no second run executes while this one
        // drives, and a signal this run sends is delivered after the
        // drive returns. The same value answers the `neighbours` tool
        // and the count `status` reports.
        let seen =
            city::Neighbourhood::scan(&self.city_root, site.building.addr(), addr, &|room| {
                self.inboxes.get(room).map_or(0, collab::Inbox::pending)
            })?;
        // Where this run stands, carried rather than worked out: a run
        // that inferred its own depth would be one wrong answer away
        // from a delegate that delegates.
        let delegates = std::rc::Rc::new(std::cell::RefCell::new(collab::DelegateDesk::new(
            at.depth(),
            site.building.addr().clone(),
        )));
        let status = self.status_tool(
            site,
            desks,
            at,
            Reach {
                seen: &seen,
                delegates: &delegates,
            },
        )?;
        let signal_tool = collab::SignalTool::new(std::rc::Rc::clone(&desks.signals))?;
        let goal_tool = collab::GoalTool::new(addr.clone(), std::rc::Rc::clone(&desks.goals))?;
        let pr_tool = collab::PrTool::new(addr.clone(), std::rc::Rc::clone(&desks.pr))?;
        let claim_tool = collab::ClaimTool::new(std::rc::Rc::clone(&desks.plan))?;
        let delegate_tool = collab::DelegateTool::new(std::rc::Rc::clone(&delegates))?;
        // What this room already got back. Copied rather than lent: the
        // authority is `self.joins`, which is folded from the ledger's
        // handback lines, and a desk that took it away would leave the
        // worker unable to answer the same question after the run.
        let mut held = collab::FanIn::new();
        if let Some(existing) = self.joins.get(addr) {
            for artifact in existing.artifacts() {
                held.accept(artifact.clone());
            }
        }
        let workshop = std::rc::Rc::new(std::cell::RefCell::new(collab::WorkshopDesk::new(
            site.who.clone(),
            held,
        )));
        let workshop_tool = collab::WorkshopTool::new(
            std::rc::Rc::clone(&workshop),
            std::rc::Rc::clone(&delegates),
        )?;
        let archive_tool = collab::ArchiveTool::new(std::rc::Rc::clone(&desks.shelf))?;
        // The one door into the building's own governance. It reaches
        // the reserved subtree, which no write domain does, so it goes
        // through the person rather than through the write gate.
        let rules_tool = city::RulesTool::new(&self.city_root, site.building.addr().clone())?;
        // The execution boundary. What the run may reach is the frozen
        // config's answer; where the engine and the interpreter live is
        // the machine's, so a city carried elsewhere does not carry this
        // machine's paths with it.
        let exec = ExecTool::new(
            site.write_root.join(addr.as_str()),
            mounts_under(&site.write_root, &site.config.sandbox.mounts),
            std::env::var(PYTHON_WASM_ENV)
                .ok()
                .map(std::path::PathBuf::from),
            execution_engine()?,
            if site.config.sandbox.shell {
                host_shell()
            } else {
                None
            },
            runtime::Fuel(site.config.sandbox.fuel),
            addr.clone(),
        )?;
        // The one door onto the rest of the city. It is registered
        // beside `signal` rather than behind it because the two answer
        // different questions - who is there, and what to say to them -
        // and until this line a model could only reach an address
        // somebody had already handed it.
        let neighbours_tool = city::NeighboursTool::new(seen)?;
        // The one tool that reads, and the only caller of the catalog's
        // second-level disclosure: without it a building's reading room
        // could name a skill and never hand it over. It holds the
        // catalog rather than a copy of what is in it, so a skill
        // admitted below this line is still reachable by name.
        let read = runtime::ReadTool::new(&site.write_root, std::rc::Rc::clone(&catalog))?;
        // The net, not the forecast, is the defence (semantic authority
        // 4.4). Two handles on one repository: the bench fences a
        // command its forecast suspects, and the driver fences every
        // wave, so whatever a wave deletes has a commit to come back
        // from. Both stand where the run writes, which is its own tree
        // when the building asks for review.
        let mut bench = ToolBench::new(site.rules.write_domain()?)
            .with_checkpoint(
                memory::Checkpoint::open(&site.write_root).map_err(memory::MemoryError::into_ax)?,
                addr.as_str(),
            )
            .for_job(addr.clone(), job_locator.clone());
        for cluster in &self.governance.granted {
            bench.grant(cluster.clone());
        }
        // One registration feeds both. The catalogue is what the model
        // was told exists and the bench is what routes the call it
        // makes, so a name on one list and not the other is either a
        // tool nobody can call or a call nobody was told about. These
        // used to be two lists of thirteen lines, agreeing by hand.
        //
        // The order is the catalogue's: `render` puts the tools in front
        // of the model in this order and the resident segment is hashed,
        // so this sequence is part of what stays cacheable across a run.
        let mut admitted: Vec<Box<dyn kernel::Tool>> = vec![
            Box::new(archive_tool),
            Box::new(exec),
            Box::new(claim_tool),
            Box::new(edit),
            Box::new(status),
            Box::new(signal_tool),
            Box::new(goal_tool),
            Box::new(pr_tool),
            Box::new(delegate_tool),
            Box::new(workshop_tool),
            Box::new(rules_tool),
            Box::new(neighbours_tool),
            Box::new(read),
        ];
        // External tools, for a building whose configuration names a
        // server. They join the table here, before the catalogue is
        // rendered, because the tool table is frozen with the run: what
        // the model is told exists is decided once.
        for server in self.mcp_tools(
            &site.config,
            &site.write_root,
            site.rules.policy().confidential,
        ) {
            let tool: Box<dyn kernel::Tool> = Box::new(server);
            admitted.push(tool);
        }
        for tool in admitted {
            catalog.borrow_mut().admit_tool(tool.meta())?;
            bench.register(tool)?;
        }
        self.admit_reading_room(&catalog, &site.rules, &site.building, addr)?;
        Ok(Workbench {
            catalog,
            bench,
            delegates,
        })
    }

    /// Builds the one tool that answers what this run is, to itself.
    ///
    /// Everything a `status` answer holds is read here, at dispatch, and
    /// frozen with the tool - except the children, which a closure reads
    /// live from the delegate desk because a run hands work down while
    /// it is going. A borrowed desk answers nothing rather than
    /// refusing: `status` reporting its own plumbing to a model would
    /// teach it about a lock it can do nothing about.
    ///
    /// # Errors
    /// Propagates a write domain that will not resolve and whatever the
    /// tool says about its own construction.
    fn status_tool(
        &self,
        site: &Site,
        desks: &Desks,
        at: &Assignment,
        reach: Reach<'_>,
    ) -> Result<StatusTool, AxError> {
        // What `status.children` reads, and the only part of the answer
        // that is not frozen here.
        let watched = std::rc::Rc::clone(reach.delegates);
        StatusTool::watching(
            status_snapshot(Situation {
                addr: &at.addr,
                who: &site.who,
                signals_pending: desks.waiting,
                mode: at.mode,
                write_domain: &site.rules.write_domain()?,
                worktree: &site.write_root,
                trust: &self.governance.autonomy,
                context_tokens: site.model.context_tokens,
                budget: at.budget,
                neighbours: reach.seen.residents(),
                // What this resident already holds, so a model asking
                // what it may touch is answered from the same list the
                // conflict check reads.
                locks: self
                    .goals
                    .iter()
                    .filter(|entry| entry.owner == site.who)
                    .map(|entry| entry.statement.clone())
                    .collect(),
            }),
            Box::new(move || {
                watched.try_borrow().map_or_else(
                    |_| Vec::new(),
                    |desk| {
                        desk.asked()
                            .iter()
                            .map(|work| runtime::ChildStatus {
                                room: work.room.clone(),
                                kind: work.kind,
                            })
                            .collect()
                    },
                )
            }),
        )
    }

    /// The external tools this run may reach, each already connected to
    /// its server.
    ///
    /// A server that cannot be started, cannot be asked what it offers,
    /// or offers something this city cannot name is left out and named
    /// in the diagnostics. That is the answer `city::library` already
    /// gives a building admitting a skill which is not on the shelves,
    /// and it holds for the same reason: what the model is told exists
    /// must equal what actually runs, and a building whose external
    /// service is down today is still a building that can work today.
    pub(super) fn mcp_tools(
        &mut self,
        config: &kernel::FrozenConfig,
        write_root: &std::path::Path,
        confidential: bool,
    ) -> Vec<protocol::McpTool> {
        if confidential && !config.mcp.is_empty() {
            // Process lifetime is this layer's own business, and an MCP
            // server is a program that may reach the network the moment
            // it starts. Nothing is started here. The tool-level refusal
            // in `protocol::McpTool::new` stays the authority on whether
            // such a tool may exist; this is the earlier consequence of
            // that rule, not a second copy of it.
            self.note(
                runtime::diagnostics::Level::Refuse,
                "bin::assembly",
                "this building is confidential; no external server is started for it",
            );
            return Vec::new();
        }
        let mut offered = Vec::new();
        let resolve = self.resolver();
        for server in &config.mcp {
            // The module a reader is sent to is the transport that
            // failed, not whichever one was written first: every MCP
            // failure used to be filed under `bin::mcp_stdio`, which
            // sent the last reader who followed it to the wrong file.
            let site = transport_site(&server.transport);
            match connect_mcp(server, write_root, confidential, &resolve) {
                Ok((tools, opened)) => {
                    self.note(
                        runtime::diagnostics::Level::Effect,
                        site,
                        &format!(
                            "{} is {} speaking {}, offering {} tool(s)",
                            server.label.as_str(),
                            opened.server,
                            opened.protocol_version,
                            tools.len()
                        ),
                    );
                    offered.extend(tools);
                }
                Err(err) => self.note(
                    runtime::diagnostics::Level::Refuse,
                    site,
                    &format!("{}: {err}; {}", server.label.as_str(), err.recovery()),
                ),
            }
        }
        offered
    }

    /// Admits the skills this building's own file names, and says which
    /// of them are not on the shelves.
    ///
    /// The city's shelves may hold a thousand; what costs resident bytes
    /// is the list this building admits. A name on that list which is not
    /// on the shelves is left out rather than promised, and noted so the
    /// person who wrote the name can see it went nowhere.
    ///
    /// # Errors
    /// Propagates a shelf that cannot be read and an entry the catalog
    /// refuses.
    fn admit_reading_room(
        &mut self,
        catalog: &std::rc::Rc<std::cell::RefCell<runtime::Catalog>>,
        rules: &city::BuildingRules,
        building: &city::Building,
        addr: &Address,
    ) -> Result<(), AxError> {
        // The reading room, and only it. The city's shelves may hold a
        // thousand skills; what costs resident bytes is the list this
        // building's own file admits, and a name on that list which is
        // not on the shelves is left out rather than promised.
        let shelves = city::Library::scan(&self.city_root, Some(building.addr()))?;
        for holding in shelves.reading_room(rules.reading_room()) {
            catalog.borrow_mut().admit_skill(runtime::CatalogEntry {
                name: holding.name.clone(),
                disclosure: holding.disclosure.clone(),
                expansion: holding.addr.as_str().to_owned(),
                // What the shelf held at this scan. The run records it,
                // so a document that changes content behind the same
                // name is a difference somebody can see later.
                hash: Some(holding.hash),
            })?;
        }
        for absent in shelves.missing(rules.reading_room()) {
            self.note(
                runtime::diagnostics::Level::Effect,
                "city::library",
                &format!(
                    "{} admits `{absent}`, which is not on the shelves",
                    addr.as_str()
                ),
            );
        }
        Ok(())
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

    #[test]
    fn a_building_whose_rules_do_not_parse_stops_the_run_rather_than_guessing() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        lay_rules(dir.path(), "lab", "# BUILDING.md\n\nnothing declared\n");
        let (base_url, _provider) = fake_openai(
            &["m-local"],
            vec![
                completion("editing", Some(("tu_1", "lab/room1/notes.md"))),
                completion("done", None),
            ],
        );
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        let err = worker
            .handle(channels::Command::Dispatch {
                addr: Address::parse("lab/room1").unwrap(),
                task: "anything".to_owned(),
                goal: "anything".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"dispatch"),
                session: None,
                effort: None,
            })
            .unwrap_err();
        assert!(err.recovery().contains("confidential: false"));
    }

    /// Until this tool existed a run could only signal an address
    /// somebody had already handed it, and a guessed one opened a queue
    /// nobody read. The evidence has to come through the production
    /// path, because what is being claimed is that the model is *told*.
    #[test]
    fn a_run_is_told_who_shares_its_building_and_what_to_bring_them() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        city::create_building(
            dir.path(),
            &Address::parse("lab").unwrap(),
            city::BuildingTemplate::Minimal,
        )
        .unwrap();
        let mason = dir.path().join("lab").join("mason");
        std::fs::create_dir_all(&mason).unwrap();
        std::fs::write(
            mason.join(city::URBANITE_FILE),
            "# URBANITE.md \u{2014} mason\n\n## Bring them\n\nAnything that has to survive a firing.\n",
        )
        .unwrap();
        std::fs::create_dir_all(dir.path().join("lab").join("store")).unwrap();

        let (base_url, provider) = fake_openai(
            &["m-local"],
            vec![
                tool_completion("who is here", "tu_1", "neighbours", serde_json::json!({})),
                tool_completion(
                    "and where do I stand",
                    "tu_2",
                    "status",
                    serde_json::json!({}),
                ),
                completion("done", None),
            ],
        );
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        worker
            .handle(channels::Command::Dispatch {
                addr: Address::parse("lab/room1").unwrap(),
                task: "find out who else is here".to_owned(),
                goal: "one answer is enough".to_owned(),
                mode: channels::ModeTag::parse("plan").unwrap(),
                budget: kernel::BudgetCap::default(),
                idem: kernel::IdemKey::derive(&RunId::CITY, kernel::Seq::FIRST, b"dispatch"),
                session: None,
                effort: None,
            })
            .unwrap();

        assert!(
            provider.bodies().join("\n").contains("neighbours"),
            "the tool table the model is given carries the way to ask"
        );
        let verified = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
        let history: String = verified
            .raw_lines()
            .iter()
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .collect::<Vec<String>>()
            .join("\n");
        assert!(
            history.contains("Anything that has to survive a firing"),
            "the line a resident wrote about itself is what reaches the one asking"
        );
        assert!(
            history.contains("lab/store"),
            "an open room is a place to send work to, not something to hide"
        );
        assert!(
            history.contains("neighbours: 1"),
            "status counts people rather than places: two rooms, one resident"
        );
    }

    #[test]
    fn a_signal_one_run_sends_is_read_by_the_run_that_pulls_it() {
        let dir = tempfile::tempdir().unwrap();
        let report = init_city(dir.path()).unwrap();
        let (base_url, provider) = fake_openai(
            &["m-local"],
            vec![
                tool_completion(
                    "telling room2",
                    "tu_1",
                    "signal",
                    serde_json::json!({
                        "action": "send",
                        "to": "lab/room2",
                        "kind": "mention",
                        "text": "the kiln is free after four",
                    }),
                ),
                completion("told them", None),
                tool_completion(
                    "checking",
                    "tu_2",
                    "signal",
                    serde_json::json!({ "action": "pull" }),
                ),
                completion("read it", None),
            ],
        );
        let mut worker = worker_with_provider(dir.path(), &base_url, "m-local").unwrap();
        for (n, room) in ["lab/room1", "lab/room2"].into_iter().enumerate() {
            worker
                .handle(channels::Command::Dispatch {
                    addr: Address::parse(room).unwrap(),
                    task: "talk to the neighbour".to_owned(),
                    goal: "one message, then stop".to_owned(),
                    mode: channels::ModeTag::parse("plan").unwrap(),
                    budget: kernel::BudgetCap::default(),
                    idem: kernel::IdemKey::derive(
                        &RunId::CITY,
                        kernel::Seq::new(u64::try_from(n).unwrap()),
                        b"dispatch",
                    ),
                    session: None,
                    effort: None,
                })
                .unwrap();
        }

        let verified = runtime::replay::verify_ledger_dir(&report.ledger_dir).unwrap();
        let history: String = verified
            .raw_lines()
            .iter()
            .map(|line| String::from_utf8_lossy(line).into_owned())
            .collect::<Vec<String>>()
            .join("\n");
        assert!(
            history.contains("signal_enqueued"),
            "a signal a tool sent is a fact the history keeps"
        );
        assert!(
            history.contains("signal_consumed"),
            "and taking it is a second fact, written by whoever took it"
        );

        let asked = provider.bodies().join("\n");
        assert!(
            asked.contains("the kiln is free after four"),
            "the point of the mechanism is that the other resident reads it"
        );
    }

    /// A build that says it carries an execution engine carries one.
    ///
    /// `AbsentSandbox` refuses with `this build carries no execution
    /// engine` and tells the reader to install a build with the `wasm`
    /// feature. Until the feature and this selection existed there was
    /// no such build: the absent engine was written into `dispatch_in`
    /// as a literal, so the sentence named an action nobody could take
    /// and `runtime::WasmtimeSandbox` had no caller outside its own
    /// tests.
    #[cfg(feature = "sandbox")]
    #[test]
    fn a_build_with_the_engine_feature_carries_one() {
        let mut engine = execution_engine().expect("a build with the feature starts its engine");
        // A module that is not there: whatever this reports, it is the
        // engine reporting it rather than the absence of one.
        let job = runtime::SandboxJob {
            wasm: std::path::PathBuf::from("no-such-module.wasm"),
            argv: Vec::new(),
            env: Vec::new(),
            stdin: Vec::new(),
            mounts: Vec::new(),
            fuel: runtime::Fuel(1_000),
        };
        let said = format!("{:?}", engine.run(&job));
        assert!(
            !said.contains("this build carries no execution engine"),
            "the feature is on and the run still met the absent engine: {said}"
        );
    }

    #[test]
    fn the_shell_arm_exists_only_where_a_layer_asked_for_it() {
        let dir = tempfile::tempdir().unwrap();
        init_city(dir.path()).unwrap();
        let room = Address::parse("lab/room1").unwrap();
        let closed = city::load_config(dir.path(), &room).unwrap();
        assert!(
            !closed.sandbox.shell,
            "silence is the closed answer, on every layer"
        );
        let building = city::config_path(dir.path(), &room, city::Layer::Building).unwrap();
        std::fs::create_dir_all(building.parent().unwrap()).unwrap();
        std::fs::write(&building, "[sandbox]\nshell = true\nfuel = 1000\n").unwrap();
        let opened = city::load_config(dir.path(), &room).unwrap();
        assert!(opened.sandbox.shell);
        assert_eq!(opened.sandbox.fuel, 1000);
    }
}
