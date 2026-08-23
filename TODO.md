# TODO

Future work, highest priority first. A row names an outcome; its design lands in the SPEC of the crate that owns it before any code is written. A row leaves this file when the work is done, not when it is planned.

Where a row says **needs a ruling**, it touches something in the `guard` row of AGENTS.md and cannot start without the person's word, recorded as a `Verdict:` trailer.

Where a row says **evidence**, the claim was checked by running something or by a search whose empty result is the finding. Rows without it are judgements, and are marked as such.

## P1 — The sub-agent system: one level lands, the way back does not

**evidence**: a real dispatch whose model called `delegate` opened `lab/helper`, wrote its JOB.md, ran it, and left what came back waiting in the asking room's queue (`what_came_back_from_a_delegate_waits_in_the_room_that_asked_for_it`). `collab::workshop`'s `NodeContract` still has no caller outside its own file.

The tool, the desk and the run-starting path landed in `card-P1.01`; the way back and the fourth cancellation point landed in `card-P1.02`. What is left, in dependency order:

1. **`status.children` is still empty, and honestly so.** A child starts after its parent's turn settles, so during the parent's run there is nothing to report. It becomes a real field only once a run can watch its children while they work.
2. **The interface shows the tree.** `RunRow` carries no parent, so the live page's session list is flat. A person watching delegated work needs to see which run answers for which.
3. **Workshop orchestration and fan-in.** `NodeContract` already states what a node reaches, reads at which version, may write, and when it stops — and says its disk form is that node's `JOB.md`. It is the layer above one-level delegation, not on its critical path.
4. **Whether the person allowed it is a sentence, not a mechanism.** City.md says do not call `delegate` unless allowed; nothing enforces it. An approval class in front of the first spawn of a run would, and `kernel::approval` already has the shape.

## P2 — A building's rules are drafted by an agent, not typed by a person

`BUILDING.md` is a governance document with six sections, and asking a person to write one by hand is the wrong door. The reserved-subtree rule is not in the way: it says **no write domain** reaches `.sprawling/`, and `city::write_effort` (F2.16) already writes there through a validated path that no write domain touches. Give `BUILDING.md` the same kind of door — draft, `evaluate` into `BuildingRules`, refuse what does not evaluate — **and put an approval in front of it**, because a building rewriting its own rules mid-run is what the reserved subtree exists to prevent. Judgement, not evidence: the approval is my reading of the rule, not something the rule states.

## P3 — What the last run left unfinished

1. **Ask a provider what it serves before attaching it.** The model list arrives only as a side effect of `AttachEndpoint`, so a person cannot see what a key buys until it is registered, and cannot choose a subset. Needs a `Query` that probes a base URL and returns model ids, and a settings form that ticks the ones to admit. `gateway::endpoint` already does the probing; the wire has no way to ask for it.
2. **The two configuration layers nothing writes.** `[mcp]` and `[sandbox]` resolve city → building → room and have no surface. `city::write_effort` is the pattern to follow. A building's MCP servers belong on the building page.
3. **`POST /enroll` answers before the credential is stored.** The route returns 201 when the command reaches the desk, and the worker's refusal reaches nobody. The shape that fits: subscribe to the event stream, post with a `Reply`, answer whichever arrives first within a bounded wait — `secret_captured` (201), a refusal (422), or neither (202, saying why).
4. **The live page cannot see anything from before it opened.** The server broadcasts and never backfills. A `Query` returning a bounded slice of history would fix it; `memory::index` already maps `seq` to a byte offset, which is the half that would otherwise be hard.
5. **Ctrl-C is still a process death.** `/quit` closes the console and leaves the city serving; `sprawling resume` recovers a hard stop. What is missing is the Handoff an orderly close would write, so a stop that was chosen and a stop that was a crash are indistinguishable in the record. **The path, now traced**: `tokio::select!` in `assembly::serve` against `tokio::signal::ctrl_c`, then a `HandoffWritten` at `RunId::CITY` whose must-read is the city's own norms. Two obstacles, both stated rather than discovered later — tokio needs its `signal` feature (**needs a ruling**: the root `Cargo.toml`; no new package), and the ledger lives inside the worker thread, so the close has to reach it as a `Command` the desk does not yet have.

## P4 — The ledger of numbers

1. **Two size readings have drifted past their slack.** `frontend_artifact` measures 524,254 B against a best of 490,594 B, and `release_binary` 8,317,952 B against 7,249,920 B. The likely cause is `web::lang`'s two-language table (F2.14, F2.18) and the binary that embeds it. `just check` does not build either artifact, so CI is not red; a `just dist` is. Rebuild both, then either recover the size or record the readings with the reason. **needs a ruling**: `xtask/budgets.toml`.

## P5 — Claims made by construction rather than by observation

Not known to be wrong; not watched.

- Nobody has unpacked the Linux archive on a Linux desktop and watched the binary open a browser. `release.yml` builds it, `ci.yml` tests the tree that goes into it, and no human has run the result. **Split it**: the ubuntu job can prove the archive unpacks and the binary serves — what stays unproven is a person seeing a browser window, and the release notes say so rather than implying otherwise.
- The Chinese interface has been read on a headless Edge at three window sizes and by nobody in a real browser. What a screenshot cannot show: whether the wording is right to somebody who did not write it.
