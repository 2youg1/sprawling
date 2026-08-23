# TODO

Future work, highest priority first. A row names an outcome; its design lands in the SPEC of the crate that owns it before any code is written. A row leaves this file when the work is done, not when it is planned.

Where a row says **needs a ruling**, it touches something in the `guard` row of AGENTS.md and cannot start without the person's word, recorded as a `Verdict:` trailer.

Where a row says **evidence**, the claim was checked by running something or by a search whose empty result is the finding. Rows without it are judgements, and are marked as such.

## P0 — The product asserts what it cannot do

Each of these is a place where the interface, the prompt, or a document promises a capability the code refuses. They are first because a person or a model that is lied to once stops trusting the parts that are true.

1. **Seven commands reach a worker that refuses them.** `handle` in `bin::assembly` has ten arms; `Halt`, `Release`, `Takeover`, `Rollback`, `BatchByBuilding`, `CreatePolicy` and `Attach` fall to `E_INVALID_ARGS: "this stage runs Dispatch; the rest land with their cards"`. The client sends them, `channels::control` classifies them, and `web::lang` has the words for two of them. **evidence**: `grep -rn "Command::Halt"` hits only `wire.rs` and `control.rs`; `channels::control::classify` has no caller outside its own crate. Take `Halt`/`Release` first: the city-wide shedding posture already exists in `kernel::backpressure`, so what is missing is the arm that sets it.
2. **Eight of the status tool's twelve fields are constants.** `assembly::status_snapshot` writes `mode: PlanGoal` whatever the run's mode is, and zeroes for `ctx_used`, `budget_usd`, `budget_tokens`, `locks`, `children`, `worktree_disk`, with `trust` fixed at `"owner"`. Only `who`, `addr`, `write_domain` and `signals_pending` are read from anything. City.md tells the model to call `status` for the time, the usage and the budget, so a model that obeys gets a row of zeros. **evidence**: `assembly.rs:3563`.
3. **`README.zh-CN.md` claims halting and releasing a city works.** It is listed under 能做, "each backed by an end-to-end assertion or a real measurement". Correct the document in the same change that either implements P0.1 or admits the gap; reality wins over the document, and the document is corrected first.
4. **The untranslated tail, fifteen strings.** `alert`'s notification text, `app::status_line`, `progress`'s `no plan`, `route`'s refusal. **This is not a translation job**: the thirteen `Msg` variants exist in `web::lang` with both languages filled and have zero callers. It is a signature change on four pure functions plus their call sites. `web-SPEC.md` §8-39 records the shape the other ten pages followed. **evidence**: `grep -rn "Msg::ProgressNoPlan"` and its twelve neighbours return nothing outside `lang.rs`.

## P1 — The sub-agent system: every decision built, no execution path

The gate, the depth typestate, the two delegate kinds and the work graph are all written and tested. Nothing in production calls any of them, so a city cannot make an agent. **evidence**: `kernel::gate::spawn` is called only from kernel's own tests and `refusal_matrix.rs`; `collab::workshop`'s `NodeContract` and `collab::fanin`'s `FanIn` have no callers outside their own files; `StatusSnapshot.children` is `Vec::new()` at its one construction point.

In dependency order — each step is small, and none can be skipped:

1. **A tool that spawns.** It asks `kernel::gate::spawn(parent, kind)` for admission, which is the door that already exists. `DelegateKind::Ephemeral` is the one-shot sub-agent; `DelegateKind::Resident` is the standing one.
2. **A way for a run to start a run.** `RunPlan` is constructed in `assembly::dispatch_in` and in citysim, and a tool cannot reach the assembly layer. The design tension is real: `bin::assembly` is the only omniscient point, and spawning asks a running thing to have it build another. The shape that fits the existing wiring is **a spawn that posts a `Dispatch`** carrying the parent run and the delegate kind, so it goes through the same desk a person's dispatch goes through.
3. **The fourth cancellation point.** `runtime::turn` has three; runtime-SPEC defers the fourth — before a spawn — "until the first spawn producer", which is step 1.
4. **`status` reports real children.** Follows P0.2, and is the half of it that only matters once spawning works: a parent that cannot see what it sent out cannot decide anything about it.
5. **The interface shows the tree.** `RunRow` carries no parent, so the live page's session list is flat. A person watching delegated work needs to see which run answers for which.
6. **The catalog discloses all of it, and City.md states the rules**, including one sentence City.md does not have yet: *unless the person has allowed it, do not call the delegate tool.* A capability the model can see is one it will try, and the permission is the person's to give. A capability that exists and is not in the catalog does not exist for the model — `runtime::catalog`'s own header says so. Each new tool needs its one-line disclosure, and City.md needs the rules that the disclosure cannot hold: one level deep, a delegate gets a small task and a stop condition, a delegate's result is a claim to verify. **This row closes with every other row in P1, not before**: a rule written for a tool that does not exist yet is the defect this whole section is about.
7. **Workshop orchestration and fan-in.** `NodeContract` already states what a node reaches, reads at which version, may write, and when it stops — and says its disk form is that node's `JOB.md`. It is the layer above spawning, not on its critical path.

## P2 — The city can be changed from inside it, and says so in one line

1. **The catalog names the three self-change modes.** `up`, `sc` and `ud` exist in `runtime::mode` with their admission rules, and a run is only ever told about the one it is in — so an agent never learns that this city's own code and SPECs are changeable, or under what discipline. **One line, and only the modes the run is not in**: the current mode already has its own row, and most sessions never need this. It says what each mode is for and that the SPEC beside the code is read before either changes.
2. **`README.md` states the shape that makes this possible**: every component carries its SPEC next to it, so a person and an agent change the same thing by reading the same file. Both languages.

## P3 — A building's rules are drafted by an agent, not typed by a person

`BUILDING.md` is a governance document with six sections, and asking a person to write one by hand is the wrong door. The reserved-subtree rule is not in the way: it says **no write domain** reaches `.sprawling/`, and `city::write_effort` (F2.16) already writes there through a validated path that no write domain touches. Give `BUILDING.md` the same kind of door — draft, `evaluate` into `BuildingRules`, refuse what does not evaluate — **and put an approval in front of it**, because a building rewriting its own rules mid-run is what the reserved subtree exists to prevent. Judgement, not evidence: the approval is my reading of the rule, not something the rule states.

## P4 — What the last run left unfinished

1. **Ask a provider what it serves before attaching it.** The model list arrives only as a side effect of `AttachEndpoint`, so a person cannot see what a key buys until it is registered, and cannot choose a subset. Needs a `Query` that probes a base URL and returns model ids, and a settings form that ticks the ones to admit. `gateway::endpoint` already does the probing; the wire has no way to ask for it.
2. **The two configuration layers nothing writes.** `[mcp]` and `[sandbox]` resolve city → building → room and have no surface. `city::write_effort` is the pattern to follow. A building's MCP servers belong on the building page.
3. **`POST /enroll` answers before the credential is stored.** The route returns 201 when the command reaches the desk, and the worker's refusal reaches nobody. The shape that fits: subscribe to the event stream, post with a `Reply`, answer whichever arrives first within a bounded wait — `secret_captured` (201), a refusal (422), or neither (202, saying why).
4. **The live page cannot see anything from before it opened.** The server broadcasts and never backfills. A `Query` returning a bounded slice of history would fix it; `memory::index` already maps `seq` to a byte offset, which is the half that would otherwise be hard.
5. **Ctrl-C is still a process death.** `/quit` closes the console and leaves the city serving; `sprawling resume` recovers a hard stop. What is missing is the Handoff an orderly close would write, so a stop that was chosen and a stop that was a crash are indistinguishable in the record.

## P5 — The ledger of numbers

1. **Two size readings have drifted past their slack.** `frontend_artifact` measures 524,254 B against a best of 490,594 B, and `release_binary` 8,317,952 B against 7,249,920 B. The likely cause is `web::lang`'s two-language table (F2.14, F2.18) and the binary that embeds it. `just check` does not build either artifact, so CI is not red; a `just dist` is. Rebuild both, then either recover the size or record the readings with the reason. **needs a ruling**: `xtask/budgets.toml`.

## P6 — Claims made by construction rather than by observation

Not known to be wrong; not watched.

- Nobody has unpacked the Linux archive on a Linux desktop and watched the binary open a browser. `release.yml` builds it, `ci.yml` tests the tree that goes into it, and no human has run the result. **Split it**: the ubuntu job can prove the archive unpacks and the binary serves — what stays unproven is a person seeing a browser window, and the release notes say so rather than implying otherwise.
- The Chinese interface has been read on a headless Edge at three window sizes and by nobody in a real browser. What a screenshot cannot show: whether the wording is right to somebody who did not write it.
