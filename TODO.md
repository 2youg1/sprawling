# TODO

Future work, highest priority first. A row names an outcome; its design lands in the SPEC of the crate that owns it before any code is written. A row leaves this file when the work is done, not when it is planned.

Where a row says **needs a ruling**, it touches something in the `guard` row of AGENTS.md and cannot start without the person's word, recorded as a `Verdict:` trailer.

Where a row says **evidence**, the claim was checked by running something or by a search whose empty result is the finding. Rows without it are judgements, and are marked as such.

## P3 — What the last run left unfinished

1. **The two configuration layers nothing writes.** `[mcp]` and `[sandbox]` resolve city → building → room and have no surface. `city::write_effort` is the pattern to follow. A building's MCP servers belong on the building page.
2. **`POST /enroll` answers before the credential is stored.** The route returns 201 when the command reaches the desk, and the worker's refusal reaches nobody. The shape that fits: subscribe to the event stream, post with a `Reply`, answer whichever arrives first within a bounded wait — `secret_captured` (201), a refusal (422), or neither (202, saying why).
3. **The live page cannot see anything from before it opened.** The server broadcasts and never backfills. A `Query` returning a bounded slice of history would fix it; `memory::index` already maps `seq` to a byte offset, which is the half that would otherwise be hard.
4. **Ctrl-C is still a process death.** `/quit` closes the console and leaves the city serving; `sprawling resume` recovers a hard stop. What is missing is the Handoff an orderly close would write, so a stop that was chosen and a stop that was a crash are indistinguishable in the record. **The path, now traced**: `tokio::select!` in `assembly::serve` against `tokio::signal::ctrl_c`, then a `HandoffWritten` at `RunId::CITY` whose must-read is the city's own norms. Two obstacles, both stated rather than discovered later — tokio needs its `signal` feature (**needs a ruling**: the root `Cargo.toml`; no new package), and the ledger lives inside the worker thread, so the close has to reach it as a `Command` the desk does not yet have.

## P4 — The ledger of numbers

1. **Two size readings have drifted past their slack.** `frontend_artifact` measures 524,254 B against a best of 490,594 B, and `release_binary` 8,317,952 B against 7,249,920 B. The likely cause is `web::lang`'s two-language table (F2.14, F2.18) and the binary that embeds it. `just check` does not build either artifact, so CI is not red; a `just dist` is. Rebuild both, then either recover the size or record the readings with the reason. **needs a ruling**: `xtask/budgets.toml`.

## P5 — Claims made by construction rather than by observation

Not known to be wrong; not watched.

- Nobody has unpacked the Linux archive on a Linux desktop and watched the binary open a browser. `release.yml` builds it, `ci.yml` tests the tree that goes into it, and no human has run the result. **Split it**: the ubuntu job can prove the archive unpacks and the binary serves — what stays unproven is a person seeing a browser window, and the release notes say so rather than implying otherwise.
- The Chinese interface has been read on a headless Edge at three window sizes and by nobody in a real browser. What a screenshot cannot show: whether the wording is right to somebody who did not write it.
