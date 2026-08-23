# TODO

Future work, highest priority first. A row names an outcome; its design lands in the SPEC of the crate that owns it before any code is written. A row leaves this file when the work is done, not when it is planned.

Where a row says **needs a ruling**, it touches something in the `guard` row of AGENTS.md and cannot start without the person's word, recorded as a `Verdict:` trailer.

## P0 — What this run left unfinished

Twelve cards landed F2.07–F2.18: the four barriers between a new city and its first run, a reserved subtree at every scope, a building's own rules and skills inside it, named sessions with a room each, thinking effort beside the send button, the launcher removal, the flag-as-path fix, and ten pages reading in the reader's language. What is left, in the order I would take it:

1. **The untranslated tail, about fifteen strings.** `alert`'s notification text, `app::status_line`, `progress`'s `no plan`, `route`'s refusal. Each sits in a pure function with no language parameter, so this card is a signature change plus the callers, not a translation job. `web-SPEC.md` §8-39 records the shape the rest followed.
2. **Ask a provider what it serves before attaching it.** Today the model list arrives only as a side effect of `AttachEndpoint`, so a person cannot see what a key buys until it is registered, and cannot choose a subset. Needs a `Query` that probes a base URL and returns model ids, and a settings form that ticks the ones to admit. `gateway::endpoint` already does the probing; the wire has no way to ask for it.
3. **The two configuration layers nothing writes.** `[mcp]` and `[sandbox]` resolve city → building → room and have no surface. `city::write_effort` (F2.16) is the pattern to follow: read the file, change one key, write it back, refuse a file this build cannot parse. A building's MCP servers belong on the building page.
4. **Ctrl-C is still a process death.** `/quit` closes the console and leaves the city serving; `sprawling resume` recovers a hard stop. What is missing is the Handoff an orderly close would write, so a stop that was chosen and a stop that was a crash are indistinguishable in the record.

## P1 — Four product gaps, each blocked on a different thing

1. **Three commands the interface cannot send.** `SetAutonomy` needs a decision about what `Owner`/`Delegate`/`Deferred` mean on screen; `BatchByBuilding` needs an address `ApprovalItem` does not carry, and deriving it from `actor` is a guess in the one queue where a guess answers for something nobody read; `Attach` needs an upload surface that does not exist, with `.sprawling/staging/` as its landing place.
2. **No tool reads a file.** `Catalog::expand` therefore has no caller: the reading room can name a skill and never hand it over, and two arms of the catalog ablation cannot be run. Per-building admission is the design, and `city::config_layers` already resolves the three layers it needs.
3. **`POST /enroll` answers before the credential is stored.** The route returns 201 when the command reaches the desk, and the worker's refusal reaches nobody. The shape that fits: subscribe to the event stream, post with a `Reply`, answer whichever arrives first within a bounded wait — `secret_captured` (201), a refusal (422), or neither (202, saying why).
4. **The live page cannot see anything from before it opened.** The server broadcasts and never backfills. A `Query` returning a bounded slice of history would fix it; `memory::index` already maps `seq` to a byte offset, which is the half that would otherwise be hard.

## P2 — Claims made by construction rather than by observation

Not known to be wrong; not watched.

- Nobody has unpacked the Linux archive on a Linux desktop and watched the binary open a browser. `release.yml` builds it, `ci.yml` tests the tree that goes into it, and no human has run the result. **Split it**: the ubuntu job can prove the archive unpacks and the binary serves — what stays unproven is a person seeing a browser window, and the release notes say so rather than implying otherwise
- The Chinese interface has been read on a headless Edge at three window sizes and by nobody in a real browser. What a screenshot cannot show: whether the wording is right to somebody who did not write it.
