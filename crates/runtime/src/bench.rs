// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.
// Copyright (c) 2026 2youg1 and the sprawling contributors

//! The tool bench: which door one call goes through, in which order,
//! and what comes back when a door says no.
//!
//! Gate routing is turn-layer work (Handoff verdict 10), so the
//! executor above stays thin: it hands the bench a call and receives an
//! exhaustive [`BenchOutcome`] rather than deciding anything itself.
//!
//! **Three orderings are load-bearing.**
//! - Dedup runs before any side effect, so a replayed call cannot bill
//!   or write twice.
//! - `exec` is forecast for discards *before* the Write door, because
//!   "this command deletes things" is a stronger claim than "this
//!   command writes somewhere" and deserves the stricter door.
//! - A `Deny` comes back as a `tool_result` carrying the refusal rather
//!   than ending the turn: the model that asked for something it may not
//!   have should learn that, and continue.
//!
//! The tool itself arrives as a `Box<dyn Tool>` and the sandbox behind
//! it is a port, so what this module owns is the ordering rather than
//! the effect.

use std::collections::{BTreeMap, BTreeSet};

use kernel::{
    Address, ApprovalItem, AxCode, AxError, DedupVerdict, DiscardForecast, Effect, EgressOutcome,
    EgressTarget, GateContext, GateOutcome, IdemKey, Locator, TaintSet, Tool, ToolCall,
    ToolOutcome, WriteDomain,
};

use serde_json::Value;

use memory::Checkpoint;

/// The tool bench: the turn layer's routing of a call through the gate
/// its own declared Effect names (Handoff verdict 10 — gate routing is
/// turn-layer work, so the executor stays thin).
///
/// Three orderings are load-bearing. Dedup runs before any side effect,
/// so a replayed call cannot bill or write twice. `exec` is forecast for
/// discards before the Write door, because "this command deletes things"
/// is a stronger claim than "this command writes somewhere" and deserves
/// the stricter door. And a Deny comes back as a `tool_result` carrying
/// the refusal rather than ending the turn: the model that asked for
/// something it may not have should learn that, and continue.
pub struct ToolBench {
    tools: BTreeMap<String, Box<dyn Tool>>,
    domain: WriteDomain,
    taint: TaintSet,
    seen: BTreeSet<IdemKey>,
    prior_public_egress: bool,
    /// The checkpoint net. A command the forecast suspects of deleting
    /// things does not get refused — text prediction is obfuscatable, so
    /// refusing on a substring would be security theatre that also
    /// blocks honest work. It gets fenced instead: commit first, then
    /// run, so whatever it deletes is restorable. Absent a net, such a
    /// command is refused, because running it unprotected is the one
    /// outcome nobody chose.
    checkpoint: Option<Checkpoint>,
    scope: String,
    /// Cluster keys the person has already allowed. Held rather than
    /// looked up: the bench runs inside a drive that owns the ledger,
    /// and a gate that read history mid-wave would be a second reader
    /// of the thing the driver is writing.
    granted: Vec<kernel::ClusterKey>,
    /// What this run was given to do, as the approvals list refers to
    /// it. An item that named no artifact would leave a person deciding
    /// about a spawn with nothing to open.
    job: Option<Locator>,
    /// Where this run works, which is what a delegation approval
    /// clusters by: the person is asked whether this resident may hand
    /// work down, once.
    asking: Option<Address>,
}

/// What the bench decided, alongside what the tool produced.
#[derive(Debug)]
pub enum BenchOutcome {
    /// The tool ran; this is its result. `fenced` carries the commit
    /// the wave was fenced against when the forecast suspected a
    /// discard, so the post-wave sweep knows what to restore from.
    Ran {
        outcome: ToolOutcome,
        fenced: Option<String>,
    },
    /// A gate refused. The refusal travels back as a tool_result, which
    /// keeps the turn alive and tells the model what it may not do.
    Refused { refusal: Box<AxError> },
    /// A gate wants a human. S3 has no answering face, so the caller
    /// sees the pending item's code and the run parks.
    Pending { item: Box<ApprovalItem> },
    /// The call was already made. Its earlier result stands.
    Duplicate,
}

impl ToolBench {
    pub fn new(domain: WriteDomain) -> ToolBench {
        ToolBench {
            tools: BTreeMap::new(),
            domain,
            taint: TaintSet::empty(),
            seen: BTreeSet::new(),
            prior_public_egress: false,
            checkpoint: None,
            scope: String::new(),
            granted: Vec::new(),
            job: None,
            asking: None,
        }
    }

    /// Hands the bench the work it serves: where the run stands and what
    /// it was given to do.
    ///
    /// Without it a spawn is refused rather than allowed, because an
    /// approval item that named neither the asker nor an artifact would
    /// reach a person as a question about nothing.
    #[must_use]
    pub fn for_job(mut self, asking: Address, job: Locator) -> ToolBench {
        self.asking = Some(asking);
        self.job = Some(job);
        self
    }

    /// Records that this cluster has already been allowed.
    ///
    /// The caller folds these from the ledger's answers, so a resumed
    /// run does not stop at the door the person just opened.
    pub fn grant(&mut self, cluster: kernel::ClusterKey) {
        self.granted.push(cluster);
    }

    /// Hands the bench its checkpoint net. Without one, a suspected
    /// discard is refused rather than run unprotected.
    pub fn with_checkpoint(mut self, checkpoint: Checkpoint, scope: &str) -> ToolBench {
        self.checkpoint = Some(checkpoint);
        self.scope = scope.to_owned();
        self
    }

    /// Registers a tool under its own declared name. A second tool
    /// claiming a taken name is refused rather than shadowing the first.
    pub fn register(&mut self, tool: Box<dyn Tool>) -> Result<(), AxError> {
        let name = tool.meta().name.as_str().to_owned();
        if self.tools.contains_key(&name) {
            return Err(AxError::failure(
                AxCode::InvalidArgs,
                "register tool",
                format!("`{name}` is already registered"),
            ));
        }
        self.tools.insert(name, tool);
        Ok(())
    }

    pub fn taint_mut(&mut self) -> &mut TaintSet {
        &mut self.taint
    }

    /// The registered tool's declaration. Callers packaging a result
    /// need its `temporal` to decide whether a clock line is due.
    pub fn meta_of(&self, name: &str) -> Option<&kernel::ToolMeta> {
        self.tools.get(name).map(|tool| tool.meta())
    }

    /// The door this call's declared Effect names.
    ///
    /// `None` means the door is open and the tool may run; `Some` means
    /// the door answered for this call and the tool does not run. The
    /// answer is the bench's, not the gate's: a refusal flows back as a
    /// tool result rather than ending the turn, and an escalation parks
    /// unless the person has already allowed that cluster.
    ///
    /// # Errors
    /// Refuses a call that declares an effect this bench does not route,
    /// an egress that names no host, a spawn or a rule change on a bench
    /// built without a job, and arguments that will not serialise for
    /// the secret scan.
    fn admit(
        &mut self,
        call: &ToolCall,
        name: &str,
        effect: &Effect,
        ctx: &GateContext,
    ) -> Result<Option<BenchOutcome>, AxError> {
        match effect {
            Effect::Read => {}
            Effect::Write { domain: target } => {
                let verdict = kernel::domain(&self.domain, target, &self.taint);
                if let Some(answered) = self.settled(verdict) {
                    return Ok(Some(answered));
                }
            }
            Effect::Connector { label } => {
                // Same door, same scan; only the target differs. A
                // connector's destination is its registration's, so
                // there is nothing for the call to name and nothing for
                // a model to get wrong.
                let spans = kernel::scan(&scanned(call, "scan connector args")?);
                let verdict = kernel::egress(
                    &spans,
                    &EgressTarget::Connector {
                        label: label.clone(),
                    },
                    self.prior_public_egress,
                );
                if let Some(answered) = self.crossed(verdict) {
                    return Ok(Some(answered));
                }
            }
            Effect::Egress => {
                let spans = kernel::scan(&scanned(call, "scan egress args")?);
                // The target is the tool's to declare; a call that does
                // not say where it is sending cannot be judged, and an
                // unjudged egress is the one thing the door exists for.
                let host = call
                    .args
                    .as_map()
                    .get("host")
                    .and_then(Value::as_str)
                    .ok_or_else(|| {
                        AxError::failure(
                            AxCode::InvalidArgs,
                            "invoke tool",
                            format!("`{name}` declares Egress but named no host"),
                        )
                    })?
                    .to_owned();
                let verdict = kernel::egress(
                    &spans,
                    &EgressTarget::Public { host },
                    self.prior_public_egress,
                );
                if let Some(answered) = self.crossed(verdict) {
                    return Ok(Some(answered));
                }
            }
            Effect::Spawn => {
                let (Some(asking), Some(job)) = (self.asking.as_ref(), self.job.as_ref()) else {
                    return Err(AxError::failure(
                        AxCode::ToolUnavailable,
                        "invoke tool",
                        format!("`{name}` declares Spawn and this bench was built without a job"),
                    )
                    .with_recovery(
                        "build the bench with `for_job`; a spawn a person cannot be asked about \
                         is a spawn nobody allowed",
                    ));
                };
                // The room is the tool's own argument, so the person is
                // told where the work is going without this layer
                // learning the tool's schema: an unreadable room reads
                // as the asking address, and the item still names a real
                // place.
                let room = call
                    .args
                    .as_map()
                    .get("room")
                    .and_then(Value::as_str)
                    .and_then(|raw| Address::parse(raw).ok())
                    .unwrap_or_else(|| asking.clone());
                let verdict = kernel::delegation(ctx, asking, &room, job, &self.taint);
                if let Some(answered) = self.settled(verdict) {
                    return Ok(Some(answered));
                }
            }
            Effect::Govern => {
                let (Some(asking), Some(job)) = (self.asking.as_ref(), self.job.as_ref()) else {
                    return Err(AxError::failure(
                        AxCode::ToolUnavailable,
                        "invoke tool",
                        format!("`{name}` declares Govern and this bench was built without a job"),
                    )
                    .with_recovery(
                        "build the bench with `for_job`; a rule change a person cannot be asked \
                         about is a rule change nobody allowed",
                    ));
                };
                let scope = call
                    .args
                    .as_map()
                    .get("scope")
                    .and_then(Value::as_str)
                    .and_then(|raw| Address::parse(raw).ok())
                    .unwrap_or_else(|| asking.clone());
                // What the person is being asked to allow, in their own
                // reading rather than as a category name.
                let proposal = call
                    .args
                    .as_map()
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let verdict = kernel::govern(ctx, asking, &scope, proposal, job, &self.taint);
                if let Some(answered) = self.settled(verdict) {
                    return Ok(Some(answered));
                }
            }
            Effect::Spend => {
                // No Spend tool instance exists until the egress proxy
                // lands (P1); the door is wired so the first one meets it.
                return Err(AxError::failure(
                    AxCode::ToolUnavailable,
                    "invoke tool",
                    format!("`{name}` declares Spend, which has no instance before P1"),
                ));
            }
            _ => {
                return Err(AxError::failure(
                    AxCode::InvalidArgs,
                    "invoke tool",
                    format!("`{name}` declares an effect this bench does not route"),
                ));
            }
        }
        Ok(None)
    }

    /// What one gate's verdict means to this bench.
    ///
    /// The granted check lives here and nowhere else. An answer the
    /// person already gave is not asked again, and the grant is per
    /// cluster because that is the unit they were shown and answered
    /// in. The rule used to be written out at each of the three doors
    /// that can escalate.
    fn settled(&self, outcome: GateOutcome) -> Option<BenchOutcome> {
        match outcome {
            GateOutcome::Allow => None,
            GateOutcome::Deny { refusal } => Some(BenchOutcome::Refused { refusal }),
            GateOutcome::Escalate { item } => {
                (!self.granted.contains(&item.cluster_key)).then(|| BenchOutcome::Pending {
                    item: Box::new(item),
                })
            }
        }
    }

    /// What one egress verdict means to this bench.
    ///
    /// The first public egress is remembered here, once, for both doors
    /// that can reach outside: a connector's registered destination and
    /// a call's own host.
    fn crossed(&mut self, outcome: EgressOutcome) -> Option<BenchOutcome> {
        match outcome {
            EgressOutcome::Allow {
                first_public_egress,
            } => {
                if first_public_egress {
                    self.prior_public_egress = true;
                }
                None
            }
            EgressOutcome::Deny { refusal } => Some(BenchOutcome::Refused { refusal }),
        }
    }

    /// Routes one call: dedup, then the door its Effect names, then the
    /// tool itself.
    pub fn invoke(
        &mut self,
        call: &ToolCall,
        key: &IdemKey,
        ctx: &GateContext,
    ) -> Result<BenchOutcome, AxError> {
        // Before any unreplayable effect (8.2).
        if kernel::dedup(&self.seen, key) == DedupVerdict::Duplicate {
            return Ok(BenchOutcome::Duplicate);
        }
        let name = call.name.as_str().to_owned();
        let Some(tool) = self.tools.get(&name) else {
            return Err(AxError::failure(
                AxCode::ToolUnavailable,
                "invoke tool",
                format!("no tool named `{name}` is registered"),
            )
            .with_recovery("call one of the tools listed in your catalog"));
        };
        let effect = tool.meta().effect.clone();

        // exec is forecast first. A hit does not refuse: it fences.
        let mut fenced = None;
        if name == "exec"
            && let Ok(arm) = crate::tools::parse_arm(call.args.as_map())
            && let DiscardForecast::Suspected { pattern } = kernel::forecast(&arm)
        {
            let Some(checkpoint) = self.checkpoint.as_mut() else {
                return Err(AxError::failure(
                    AxCode::ToolUnavailable,
                    "invoke tool",
                    format!("`{pattern}` may discard files and no checkpoint net is configured"),
                )
                .with_recovery(
                    "configure the checkpoint net, or run a command that does not delete",
                ));
            };
            let payload = checkpoint
                .wave_pre(&self.scope, ctx.now, &ctx.actor)
                .map_err(kernel_error_from_memory)?;
            fenced = payload
                .as_map()
                .get("oid")
                .and_then(Value::as_str)
                .map(str::to_owned);
        }

        if let Some(answered) = self.admit(call, &name, &effect, ctx)? {
            return Ok(answered);
        }

        // The key is recorded once the call is committed to, so a retry
        // after a gate refusal is not treated as a replay.
        self.seen.insert(*key);
        // Re-borrowed here: the forecast fence needed `self` mutably.
        let Some(tool) = self.tools.get_mut(&name) else {
            return Err(AxError::failure(
                AxCode::ToolUnavailable,
                "invoke tool",
                format!("no tool named `{name}` is registered"),
            ));
        };
        let outcome = tool.invoke(call)?;
        Ok(BenchOutcome::Ran { outcome, fenced })
    }
}

/// One call's arguments as the bytes the secret scan reads.
///
/// Two doors reach outside and both scan the same thing; `doing` names
/// which one, so a failure to serialise says which door it happened at.
///
/// # Errors
/// Refuses arguments that will not serialise, which is a call this
/// bench cannot judge rather than a call it may let through.
fn scanned(call: &ToolCall, doing: &'static str) -> Result<Vec<u8>, AxError> {
    serde_json::to_vec(&call.args)
        .map_err(|err| AxError::failure(AxCode::InvalidArgs, doing, err.to_string()))
}

/// The memory crate owns its own error root; the turn layer speaks
/// AxError, so the conversion happens once, here.
fn kernel_error_from_memory(err: memory::MemoryError) -> AxError {
    err.into_ax()
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
    use crate::sandbox::{EchoSandbox, Fuel};
    use crate::tools::{EditTool, ExecTool};
    use kernel::{ApprovalId, Payload, RunId, Seq, TimeMs};
    use serde_json::Map;

    fn ctx() -> GateContext {
        GateContext {
            actor: "resident".to_owned(),
            now: TimeMs::new(1_700_000_000_000),
            item_id: ApprovalId::new("item-1").expect("id"),
        }
    }

    fn key(n: u64) -> IdemKey {
        IdemKey::derive(&RunId::from_bytes([1u8; 16]), Seq::new(n), b"action")
    }

    fn bench(root: &std::path::Path) -> ToolBench {
        let domain = WriteDomain::new(vec![Address::parse("work").unwrap()]).unwrap();
        let mut bench = ToolBench::new(domain.clone());
        bench
            .register(Box::new(
                EditTool::new(root, Address::parse("work").unwrap(), domain).unwrap(),
            ))
            .unwrap();
        bench
    }

    fn edit_call(path: &str, base: &str, old: &str, new: &str) -> ToolCall {
        let mut args = Map::new();
        for (k, v) in [
            ("path", path),
            ("base_version", base),
            ("old", old),
            ("new", new),
        ] {
            args.insert(k.to_owned(), Value::String(v.to_owned()));
        }
        ToolCall {
            id: "c1".to_owned(),
            name: kernel::ToolName::parse("edit").unwrap(),
            args: Payload::new(args).unwrap(),
        }
    }

    #[test]
    fn dedup_runs_before_the_side_effect() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("work")).unwrap();
        std::fs::write(tmp.path().join("work/a.txt"), "one\n").unwrap();
        let mut bench = bench(tmp.path());
        let version = crate::tools::version_of(b"one\n");
        let call = edit_call("work/a.txt", &version, "one", "two");

        let first = bench.invoke(&call, &key(1), &ctx()).unwrap();
        assert!(matches!(first, BenchOutcome::Ran { .. }));
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("work/a.txt")).unwrap(),
            "two\n"
        );
        // The same key again: the tool must not run a second time, and
        // the file must not change (the second edit would fail on the
        // stale version anyway — dedup means it is never attempted).
        let second = bench.invoke(&call, &key(1), &ctx()).unwrap();
        assert!(matches!(second, BenchOutcome::Duplicate));
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("work/a.txt")).unwrap(),
            "two\n"
        );
    }

    /// A tool that declares `Spawn` and nothing else. The bench's job
    /// here is the door, not the tool, so the tool does as little as a
    /// tool can.
    struct SpawnTool(kernel::ToolMeta);

    impl SpawnTool {
        fn new() -> SpawnTool {
            SpawnTool(kernel::ToolMeta {
                name: kernel::ToolName::parse("delegate").unwrap(),
                disclosure: "hand work down".to_owned(),
                params: Payload::empty(),
                effect: Effect::Spawn,
                cost_tier: kernel::CostTier::Heavy,
                timeout: None,
                render: kernel::RenderIntent::Generic,
                temporal: kernel::Temporal::Timeless,
            })
        }
    }

    impl Tool for SpawnTool {
        fn meta(&self) -> &kernel::ToolMeta {
            &self.0
        }

        fn invoke(&mut self, _call: &ToolCall) -> Result<ToolOutcome, AxError> {
            Ok(ToolOutcome {
                result: Payload::empty(),
            })
        }
    }

    fn spawn_call() -> ToolCall {
        let mut args = Map::new();
        args.insert("room".to_owned(), Value::String("work/helper".to_owned()));
        ToolCall {
            id: "c1".to_owned(),
            name: kernel::ToolName::parse("delegate").unwrap(),
            args: Payload::new(args).unwrap(),
        }
    }

    fn spawn_bench() -> ToolBench {
        let domain = WriteDomain::new(vec![Address::parse("work").unwrap()]).unwrap();
        let mut bench = ToolBench::new(domain).for_job(
            Address::parse("work/room1").unwrap(),
            Locator::parse(&format!("file:work/room1/JOB.md@{}", "a".repeat(40))).unwrap(),
        );
        bench.register(Box::new(SpawnTool::new())).unwrap();
        bench
    }

    /// City.md told a model not to delegate unless the person allowed
    /// it, and nothing checked. Now the first spawn stops at a door.
    #[test]
    fn a_spawn_waits_for_the_person_and_a_granted_cluster_walks_through() {
        let mut bench = spawn_bench();
        let waiting = bench.invoke(&spawn_call(), &key(1), &ctx()).unwrap();
        let BenchOutcome::Pending { item } = waiting else {
            panic!("the first spawn of a run is the person's to allow");
        };
        assert_eq!(item.cluster_key.class, kernel::ApprovalClass::Delegation);
        assert_eq!(
            item.cluster_key.detail, "work/room1",
            "the cluster is the resident asking, so one answer covers its whole session"
        );
        assert!(item.action_desc.contains("work/helper"), "{item:?}");

        let mut allowed = spawn_bench();
        allowed.grant(item.cluster_key.clone());
        assert!(matches!(
            allowed.invoke(&spawn_call(), &key(1), &ctx()).unwrap(),
            BenchOutcome::Ran { .. }
        ));
    }

    /// Fail-closed: a bench nobody told what work it serves cannot mint
    /// an item a person could answer, so it refuses rather than letting
    /// the spawn through unasked.
    #[test]
    fn a_spawn_on_a_bench_with_no_job_is_refused_rather_than_waved_through() {
        let domain = WriteDomain::new(vec![Address::parse("work").unwrap()]).unwrap();
        let mut bench = ToolBench::new(domain);
        bench.register(Box::new(SpawnTool::new())).unwrap();
        let err = bench.invoke(&spawn_call(), &key(1), &ctx()).unwrap_err();
        assert_eq!(*err.code(), AxCode::ToolUnavailable);
        assert!(err.recovery().contains("for_job"));
    }

    #[test]
    fn a_write_outside_the_domain_flows_back_as_a_refusal_not_a_dead_turn() {
        let tmp = tempfile::tempdir().unwrap();
        let domain = WriteDomain::new(vec![Address::parse("work").unwrap()]).unwrap();
        let elsewhere = WriteDomain::new(vec![Address::parse("elsewhere").unwrap()]).unwrap();
        let mut bench = ToolBench::new(domain);
        // A tool whose declared domain sits outside the run's domain.
        bench
            .register(Box::new(
                EditTool::new(tmp.path(), Address::parse("elsewhere").unwrap(), elsewhere).unwrap(),
            ))
            .unwrap();
        let outcome = bench
            .invoke(&edit_call("elsewhere/x", "v", "a", "b"), &key(2), &ctx())
            .unwrap();
        match outcome {
            BenchOutcome::Refused { refusal } => {
                assert_eq!(*refusal.code(), AxCode::OutsideWriteDomain);
            }
            other => panic!("expected a refusal that keeps the turn alive, got {other:?}"),
        }
    }

    #[test]
    fn a_suspected_discard_without_a_net_is_refused_and_with_one_is_fenced() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(tmp.path().join("work")).unwrap();
        std::fs::write(tmp.path().join("work/doomed.txt"), "bye").unwrap();
        let domain = WriteDomain::new(vec![Address::parse("work").unwrap()]).unwrap();

        let exec_call = |text: &str| {
            let mut args = Map::new();
            args.insert(
                "arm".to_owned(),
                serde_json::json!({ "shell": { "text": text } }),
            );
            ToolCall {
                id: "e1".to_owned(),
                name: kernel::ToolName::parse("exec").unwrap(),
                args: Payload::new(args).unwrap(),
            }
        };
        let exec_tool = || {
            Box::new(
                ExecTool::new(
                    tmp.path().to_path_buf(),
                    Vec::new(),
                    None,
                    Box::new(EchoSandbox::new()),
                    None,
                    Fuel(1000),
                    Address::parse("work").unwrap(),
                )
                .unwrap(),
            )
        };

        // No net: a command the forecast suspects is refused rather than
        // run unprotected.
        let mut bare = ToolBench::new(domain.clone());
        bare.register(exec_tool()).unwrap();
        let err = match bare.invoke(&exec_call("rm -rf work"), &key(3), &ctx()) {
            Err(err) => err,
            Ok(other) => panic!("expected a refusal, got {other:?}"),
        };
        assert_eq!(*err.code(), AxCode::ToolUnavailable);

        // With a net: the wave is fenced first, and the outcome carries
        // the commit the sweep will restore from.
        let checkpoint = Checkpoint::open(tmp.path()).unwrap();
        let mut fenced_bench = ToolBench::new(domain).with_checkpoint(checkpoint, "work");
        fenced_bench.register(exec_tool()).unwrap();
        // The shell arm is unconfigured, so the tool itself refuses —
        // but only after the fence went up, which is what we assert.
        let _ = fenced_bench.invoke(&exec_call("rm -rf work"), &key(4), &ctx());
        // The fence went up before the command was allowed to run: a
        // repository now exists with a commit to restore from.
        assert!(
            tmp.path().join(".git").exists(),
            "the checkpoint net was raised"
        );
        let mut probe = Checkpoint::open(tmp.path()).unwrap();
        let payload = probe
            .wave_pre("work", TimeMs::new(1_700_000_001_000), "probe")
            .unwrap();
        let oid = serde_json::to_value(&payload).unwrap()["oid"]
            .as_str()
            .unwrap()
            .to_owned();
        assert!(!oid.is_empty(), "the fence has a commit to restore from");
    }

    #[test]
    fn an_unregistered_name_is_refused_rather_than_routed_anywhere() {
        let tmp = tempfile::tempdir().unwrap();
        let mut bench = bench(tmp.path());
        let mut call = edit_call("work/a", "v", "a", "b");
        call.name = kernel::ToolName::parse("status").unwrap();
        let err = match bench.invoke(&call, &key(5), &ctx()) {
            Err(err) => err,
            Ok(other) => panic!("expected a refusal, got {other:?}"),
        };
        assert_eq!(*err.code(), AxCode::ToolUnavailable);
    }

    #[test]
    fn a_second_tool_claiming_a_taken_name_is_refused() {
        let tmp = tempfile::tempdir().unwrap();
        let mut bench = bench(tmp.path());
        let err = match bench.register(Box::new(
            EditTool::new(
                tmp.path(),
                Address::parse("work").unwrap(),
                WriteDomain::new(vec![Address::parse("work").unwrap()]).unwrap(),
            )
            .unwrap(),
        )) {
            Err(err) => err,
            Ok(()) => panic!("a name collision must refuse, not shadow"),
        };
        assert_eq!(*err.code(), AxCode::InvalidArgs);
    }
}
