// This Source Code Form is subject to the terms of the Mozilla Public
// License, v. 2.0. If a copy of the MPL was not distributed with this
// file, You can obtain one at https://mozilla.org/MPL/2.0/.

//! S2.03: the living skeleton. Dispatch -> four-phase turns -> run_frozen,
//! all through scripted adapters, chain-verified, byte-deterministic.
//! A9's first standing assertions live here: cancel takes effect at phase
//! boundaries, never inside a phase.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing,
    reason = "test code"
)]

use citysim::{CancelPoint, Scenario, ScriptModel, ScriptTool, run_scenario};
use kernel::{
    Address, ClockStampGranularity, CostTier, Effect, FrozenConfig, ModelReturn, Payload,
    RenderIntent, RunId, Temporal, ToolCall, ToolMeta, ToolName, ToolOutcome, WriteDomain,
};
use runtime::turn::ToolBench;

/// The bench the scenarios drive: the real one (S3.14), carrying
/// whichever tools a scenario wants registered.
fn bench_with(tools: Vec<Box<dyn kernel::Tool>>) -> ToolBench {
    let domain = WriteDomain::new(vec![Address::parse("sim/lobby/room1").unwrap()]).unwrap();
    let mut bench = ToolBench::new(domain);
    for tool in tools {
        bench.register(tool).unwrap();
    }
    bench
}

fn quiet_config() -> FrozenConfig {
    FrozenConfig {
        clock_stamp: ClockStampGranularity::Off,
        clock_zones: Vec::new(),
        sandbox: kernel::SandboxLimits::default(),
        mcp: Vec::new(),
        effort: None,
    }
}

fn kinds(lines: &[Vec<u8>]) -> Vec<String> {
    lines
        .iter()
        .map(|line| {
            let value: serde_json::Value = serde_json::from_slice(line).unwrap();
            value["kind"].as_str().unwrap().to_owned()
        })
        .collect()
}

fn probe_meta() -> ToolMeta {
    ToolMeta {
        name: ToolName::parse("probe").unwrap(),
        disclosure: "scripted probe; call it when the scenario says so".into(),
        params: Payload::empty(),
        effect: Effect::Read,
        cost_tier: CostTier::Free,
        timeout: None,
        render: RenderIntent::Generic,
        temporal: Temporal::Timeless,
    }
}

fn probe_call() -> ToolCall {
    ToolCall {
        id: "call-1".to_owned(),
        name: ToolName::parse("probe").unwrap(),
        args: Payload::empty(),
    }
}

fn scenario(cancel: Option<CancelPoint>) -> Scenario {
    Scenario {
        run: RunId::parse("0198f6a2-7c4a-7bbb-9d1e-0000000000ff").unwrap(),
        who: "worker@sim.1".into(),
        addr: Address::parse("sim/lobby/room1").unwrap(),
        task: "close the minimal loop".into(),
        goal: "one tool wave, then stop".into(),
        job_md: "# JOB\nRun one probe, then conclude.".into(),
        model: ScriptModel::new(vec![
            ModelReturn {
                usage: None,
                stop: None,
                billed_usd_micros: None,
                message: Payload::empty(),
                calls: vec![probe_call()],
            },
            ModelReturn {
                usage: None,
                stop: None,
                billed_usd_micros: None,
                message: Payload::empty(),
                calls: vec![],
            },
        ]),
        bench: bench_with(vec![Box::new(ScriptTool::new(
            probe_meta(),
            vec![Ok(ToolOutcome {
                result: Payload::empty(),
            })],
        ))]),
        config: quiet_config(),
        checkpoint: None,
        cancel,
        steer: None,
        budget_turns: 8,
    }
}

#[test]
fn the_minimal_loop_closes_and_the_chain_verifies() {
    let report = run_scenario(scenario(None)).unwrap();
    assert_eq!(report.completion, "done");
    assert_eq!(
        kinds(&report.lines),
        [
            "checkpoint_committed",
            "run_started",
            "prompt_assembled",
            "model_called",
            "model_returned",
            "tool_called",
            "tool_result",
            "prompt_assembled",
            "model_called",
            "model_returned",
            "handoff_written",
            "run_frozen",
        ]
    );
    // A6 in the account: the frozen line cites its evidence.
    let frozen: serde_json::Value = serde_json::from_slice(report.lines.last().unwrap()).unwrap();
    assert_eq!(frozen["data"]["completion"], "done");
    assert_eq!(frozen["data"]["evidence"][0]["kind"], "model_returned");
    citysim::check_chain(report.lines).unwrap();
}

#[test]
fn the_loop_is_byte_deterministic() {
    let one = run_scenario(scenario(None)).unwrap();
    let two = run_scenario(scenario(None)).unwrap();
    assert_eq!(one.lines, two.lines, "same scenario, same bytes");
}

#[test]
fn a9_cancel_before_assemble_stops_before_any_window_bytes() {
    let report = run_scenario(scenario(Some(CancelPoint::BeforeAssemble { turn: 1 }))).unwrap();
    assert_eq!(report.completion, "cancelled");
    let ks = kinds(&report.lines);
    // The first turn ran whole; the second never assembled.
    assert_eq!(
        ks,
        [
            "checkpoint_committed",
            "run_started",
            "prompt_assembled",
            "model_called",
            "model_returned",
            "tool_called",
            "tool_result",
            "cancel_received",
            "handoff_written",
            "run_frozen",
        ]
    );
    citysim::check_chain(report.lines).unwrap();
}

#[test]
fn a9_cancel_before_call_leaves_the_assembled_prompt_on_the_ledger() {
    let report = run_scenario(scenario(Some(CancelPoint::BeforeCall { turn: 0 }))).unwrap();
    assert_eq!(report.completion, "cancelled");
    assert_eq!(
        kinds(&report.lines),
        [
            "checkpoint_committed",
            "run_started",
            "prompt_assembled",
            "cancel_received",
            "handoff_written",
            "run_frozen",
        ]
    );
}

#[test]
fn a9_cancel_before_wave_completes_the_model_call_but_runs_no_tool() {
    let report = run_scenario(scenario(Some(CancelPoint::BeforeWave { turn: 0 }))).unwrap();
    assert_eq!(report.completion, "cancelled");
    let ks = kinds(&report.lines);
    assert_eq!(
        ks,
        [
            "checkpoint_committed",
            "run_started",
            "prompt_assembled",
            "model_called",
            "model_returned",
            "cancel_received",
            "handoff_written",
            "run_frozen",
        ]
    );
    assert!(
        !ks.contains(&"tool_called".to_owned()),
        "cancel at the wave boundary must run no tool"
    );
}

#[test]
fn exhausting_the_turn_budget_freezes_with_limit() {
    let mut sc = scenario(None);
    // A model that always asks for another probe: the executor's turn
    // budget is the only stop.
    sc.model = ScriptModel::new(
        (0..16)
            .map(|_| ModelReturn {
                usage: None,
                stop: None,
                billed_usd_micros: None,
                message: Payload::empty(),
                calls: vec![probe_call()],
            })
            .collect(),
    );
    sc.bench = bench_with(vec![Box::new(ScriptTool::new(
        probe_meta(),
        (0..16)
            .map(|_| {
                Ok(ToolOutcome {
                    result: Payload::empty(),
                })
            })
            .collect(),
    ))]);
    sc.budget_turns = 3;
    let report = run_scenario(sc).unwrap();
    assert_eq!(report.completion, "limit");
    let ks = kinds(&report.lines);
    assert_eq!(ks.iter().filter(|k| *k == "prompt_assembled").count(), 3);
    assert_eq!(ks.last().unwrap(), "run_frozen");
}

#[test]
fn the_domain_door_bites_an_out_of_domain_write_inside_the_loop() {
    let mut sc = scenario(None);
    let write_meta = ToolMeta {
        name: ToolName::parse("edit_elsewhere").unwrap(),
        disclosure: "writes outside the room; exists to be refused".into(),
        params: Payload::empty(),
        effect: Effect::Write {
            domain: Address::parse("other/building/file.md").unwrap(),
        },
        cost_tier: CostTier::Free,
        timeout: None,
        render: RenderIntent::Generic,
        temporal: Temporal::Timeless,
    };
    sc.model = ScriptModel::new(vec![
        ModelReturn {
            usage: None,
            stop: None,
            billed_usd_micros: None,
            message: Payload::empty(),
            calls: vec![ToolCall {
                id: "call-2".to_owned(),
                name: ToolName::parse("edit_elsewhere").unwrap(),
                args: Payload::empty(),
            }],
        },
        ModelReturn {
            usage: None,
            stop: None,
            billed_usd_micros: None,
            message: Payload::empty(),
            calls: vec![],
        },
    ]);
    sc.bench = bench_with(vec![Box::new(ScriptTool::new(
        write_meta,
        vec![Ok(ToolOutcome {
            result: Payload::empty(),
        })],
    ))]);
    let report = run_scenario(sc).unwrap();
    assert_eq!(report.completion, "done");
    let denial = report
        .lines
        .iter()
        .map(|line| serde_json::from_slice::<serde_json::Value>(line).unwrap())
        .find(|v| v["kind"] == "tool_result")
        .unwrap();
    assert_eq!(denial["data"]["error"]["code"], "E_OUTSIDE_WRITE_DOMAIN");
    let gate = &denial["data"]["error"]["gate"];
    assert!(gate["rule"].as_str().unwrap().contains("write domain"));
    assert!(!gate["alternative"].as_str().unwrap().is_empty());
    citysim::check_chain(report.lines).unwrap();
}

#[test]
fn a_tool_error_flows_back_as_a_tool_result_and_the_loop_continues() {
    let mut sc = scenario(None);
    sc.bench = bench_with(vec![Box::new(ScriptTool::new(probe_meta(), vec![]))]);
    // Script: one wave (tool will fail: outcome script exhausted), then conclude.
    let report = run_scenario(sc).unwrap();
    assert_eq!(report.completion, "done");
    let last_result = report
        .lines
        .iter()
        .map(|line| serde_json::from_slice::<serde_json::Value>(line).unwrap())
        .rfind(|v| v["kind"] == "tool_result")
        .unwrap();
    assert_eq!(last_result["data"]["error"]["code"], "E_TOOL_UNAVAILABLE");
}

#[test]
fn a_scenario_ledger_forks_into_a_byte_identical_prefix() {
    // The fork face consumed from the scenario side (A19 on skeleton
    // output): verify, cut at a mid node, compare bytes.
    let report = run_scenario(scenario(None)).unwrap();
    let verified = runtime::replay::verify_lines(report.lines.clone()).unwrap();
    let at = kernel::Seq::new(4);
    let prefix = runtime::fork::prefix(&verified, at).unwrap();
    assert_eq!(prefix.len(), 5);
    assert_eq!(prefix.as_slice(), &report.lines[..5]);
    // Past the tail: refused, never clamped.
    let over = kernel::Seq::new(9999);
    assert!(runtime::fork::prefix(&verified, over).is_err());
}

/// S3.14 — the closing card: one Resident, the real adapters, end to end.
///
/// The scripted model now speaks provider wire JSON and reaches the seam
/// through `gateway::dialect`, the same translation the endpoint uses.
/// The tools are the real L0 ones writing to a real directory. The gate
/// routing is the real bench. What stays simulated is the ledger (its
/// on-disk twin is already proven by conformance) and the HTTP face (a
/// network call is not deterministic).
#[test]
fn s3_14_one_resident_closes_the_loop_through_the_real_adapters() {
    use citysim::ScriptModel as Model;
    use kernel::DialectKind;
    use runtime::{EditTool, StatusSnapshot, StatusTool};

    let city = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(city.path().join("sim/lobby/room1")).unwrap();
    let target = city.path().join("sim/lobby/room1/notes.md");
    std::fs::write(&target, "status: draft\n").unwrap();
    let version = runtime::version_of(b"status: draft\n");

    let domain = WriteDomain::new(vec![Address::parse("sim/lobby/room1").unwrap()]).unwrap();
    let mut bench = ToolBench::new(domain.clone());
    bench
        .register(Box::new(
            EditTool::new(
                city.path(),
                Address::parse("sim/lobby/room1").unwrap(),
                domain,
            )
            .unwrap(),
        ))
        .unwrap();
    bench
        .register(Box::new(
            StatusTool::new(StatusSnapshot {
                who: "worker@sim.1".to_owned(),
                addr: Address::parse("sim/lobby/room1").unwrap(),
                mode: runtime::mode::Mode::Up,
                ctx_used: kernel::Tokens::new(900),
                ctx_limit: kernel::Tokens::new(8000),
                budget_usd: kernel::UsdMicros::new(500_000),
                budget_tokens: kernel::Tokens::new(50_000),
                trust: "trusted".to_owned(),
                write_domain: "sim/lobby/room1".to_owned(),
                locks: Vec::new(),
                worktree_path: "sim/lobby/room1".to_owned(),
                worktree_disk: kernel::ByteLen::new(14),
                signals_pending: 0,
                children: Vec::new(),
                now: None,
                provider_mode: runtime::ProviderMode::Normal,
            })
            .unwrap(),
        ))
        .unwrap();

    // Three turns of wire JSON: look around, edit, conclude.
    let model = Model::from_wire(
        DialectKind::Anthropic,
        vec![
            serde_json::json!({
                "content": [
                    { "type": "tool_use", "id": "c1", "name": "status", "input": {} },
                ],
                "stop_reason": "tool_use",
                "usage": { "input_tokens": 20, "output_tokens": 8 },
            }),
            serde_json::json!({
                "content": [
                    { "type": "text", "text": "marking it done" },
                    { "type": "tool_use", "id": "c2", "name": "edit", "input": {
                        "path": "sim/lobby/room1/notes.md",
                        "base_version": version,
                        "old": "draft",
                        "new": "final",
                    }},
                ],
                "stop_reason": "tool_use",
                "usage": { "input_tokens": 40, "output_tokens": 20 },
            }),
            serde_json::json!({
                "content": [{ "type": "text", "text": "done" }],
                "stop_reason": "end_turn",
                "usage": { "input_tokens": 60, "output_tokens": 4 },
            }),
        ],
    )
    .unwrap();

    let scenario = Scenario {
        run: RunId::parse("0198f6a2-7c4a-7bbb-9d1e-00000000031e").unwrap(),
        who: "worker@sim.1".into(),
        addr: Address::parse("sim/lobby/room1").unwrap(),
        task: "mark the note final".into(),
        goal: "one edit, verified".into(),
        job_md: "# JOB\nEdit notes.md.".into(),
        model,
        bench,
        // Minute stamps: the clock line rides the envelope.
        config: FrozenConfig {
            clock_stamp: ClockStampGranularity::Minute,
            clock_zones: Vec::new(),
            sandbox: kernel::SandboxLimits::default(),
            mcp: Vec::new(),
            effort: None,
        },
        // The real net over the real tree: A14's leading half.
        checkpoint: Some((
            memory::Checkpoint::open(city.path()).unwrap(),
            "sim/lobby/room1".to_owned(),
        )),
        cancel: None,
        steer: None,
        budget_turns: 8,
    };
    let report = run_scenario(scenario).unwrap();

    assert_eq!(report.completion, "done");
    // The real edit landed on the real file.
    assert_eq!(
        std::fs::read_to_string(&target).unwrap(),
        "status: final\n",
        "the tool wrote through to disk"
    );
    // The chain verifies offline, as every run must.
    runtime::replay::verify_lines(report.lines.clone()).unwrap();

    let ks = kinds(&report.lines);
    assert_eq!(ks.first().map(String::as_str), Some("checkpoint_committed"));
    assert_eq!(ks.last().map(String::as_str), Some("run_frozen"));
    assert_eq!(
        ks.iter().filter(|k| k.as_str() == "tool_result").count(),
        2,
        "status then edit: {ks:?}"
    );

    // A14's leading half, read off the event order: every tool_called is
    // preceded by a checkpoint_committed, so whatever the wave touches
    // has a commit to come back from.
    for (i, kind) in ks.iter().enumerate() {
        if kind == "tool_called" {
            let fence = ks
                .get(..i)
                .and_then(|before| before.iter().rposition(|k| k == "checkpoint_committed"));
            let model_returned = ks
                .get(..i)
                .and_then(|before| before.iter().rposition(|k| k == "model_returned"));
            assert!(
                fence > model_returned,
                "the fence must go up after the model asked and before the tool ran: {ks:?}"
            );
        }
    }

    // The clock line rode at least one envelope (granularity is Minute,
    // and status declares itself Timestamped).
    let stamped = report
        .lines
        .iter()
        .map(|line| serde_json::from_slice::<serde_json::Value>(line).unwrap())
        .filter(|v| v["kind"] == "tool_result")
        .any(|v| v.to_string().contains("1970-01-01"));
    assert!(stamped, "a clock line rode the envelope");
}

#[test]
fn s3_14_the_run_is_byte_identical_when_replayed() {
    // Determinism is the point of the simulator: the same scenario twice
    // must produce the same ledger, byte for byte.
    let first = run_scenario(scenario(None)).unwrap();
    let second = run_scenario(scenario(None)).unwrap();
    assert_eq!(first.lines, second.lines);
}
