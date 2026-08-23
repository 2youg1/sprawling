# TODO

Future work, highest priority first. A row names an outcome; its design lands in the SPEC of the crate that owns it before any code is written. A row leaves this file when the work is done, not when it is planned.

Where a row says **needs a ruling**, it touches something in the `guard` row of AGENTS.md and cannot start without the person's word, recorded as a `Verdict:` trailer.

## P0 — The session a person can name, and the interface in their language

Three cards landed the first half of this programme: a `.sprawling` subtree is reserved at any depth (F2.08), a building's rules and configuration moved into its own (F2.09), and a building keeps its own skills there (F2.10). What is left, in order:

1. **A session has a name, and the name is the room it works in** — designed in `channels-SPEC.md` §8-7, not yet written. `Dispatch` carries `session: Option<SessionName>`, `WIRE_V` goes 4 → 5, the city mints `<building>/<name>` and suffixes a collision, and every surface shows the name instead of a `RunId` prefix. This is what stops several dispatches to one address from overwriting each other's files
2. **The interface in Chinese, with a switch** — `web::lang`, an exhaustive `Msg` enum so a missing translation fails to compile. **Measured, not estimated: 248 prose literals — a quoted string carrying a space, outside every test module — across 17 modules of `crates/web/src`, the heaviest being `settings` (39), `overview` (33) and `live` (28), plus the page shell.** The nav, the dispatch bar, the settings page and the first screen are perhaps forty of them and are worth doing first; the panel `scope` and `source` sentences are the long tail
3. **The three configured capabilities nothing writes** — `[model] effort`, `[sandbox]` and `[mcp]` resolve down city → building → room in `city::config_layers`, and no surface writes a `CONFIG.toml` at any layer. Thinking effort and a building's MCP servers are therefore built and unreachable

## P1 — What is left of the front end

The rebuild landed the shell, the first screen, the isometric city as shapes, and the interventions that had no way to be sent. Three pieces are left, and none of them blocks the others:

1. **`web::lang`.** An exhaustive `Msg` enum makes a missing translation fail to compile. Roughly 365 user-visible strings across the client today, each written inline. The font half of this row is settled and gone: no font file ships, none is fetched, and none is named — the stylesheet asks for the reader's own `sans-serif` and `monospace`, which is exactly the setting a browser exposes, and `web::theme` records why
2. **Settings, two levels.** Models / City / Interface / Security / About. Interface exists as one panel; the City pane needs Queries the wire does not have yet, so `WIRE_V` goes from 4 to 5
3. **The repetitions.** `Root` takes twenty-three parameters, thirteen of which are one concept; nine modules each hold their own copy of the "ask once the socket is live" dance

- Lands in `crates/web/`; colour, type, space and shape all come from `web::theme` and nowhere else, which the `color` gate and a unit test over `assets/index.html` hold together
- Decisions go into `crates/web/web-SPEC.md` first

## P2 — Three commands the interface still cannot send

`Fork`, `Takeover` and `Rollback` reached the client at F2.05. Three are still unreachable, each for a different reason, and each reason is the work:

- **`SetAutonomy`** — the wire carries `Autonomy::{Owner, Delegate, Deferred}` at a scope. What each of those means to a person, and what changes on screen when one is chosen, is not decided anywhere; a control that sets a mode nobody can describe is worse than no control
- **`BatchByBuilding`** — needs a Building address, and `ApprovalItem` carries `actor` rather than an address. Deriving the building from the actor string is a guess, and this is the queue where a wrong guess answers for something nobody read
- **`Attach`** — hands an uploaded artifact to a set of runs. The client has no upload surface at all, and `.sprawling/staging/` is where an upload lands

## P3 — A tool that reads a file, and the catalog ablation behind it

The catalog now reaches the model (`runtime-SPEC.md` §8-11), but only its first level. A skill's `expansion` is an address under the reserved prefix `.sprawling/`, and no tool in this build reads a file — `edit` changes one, it does not read one. So the reading room can name a skill and can never hand it over, and `Catalog::expand` has no path to a caller.

A read tool closes that, and it is a capability the model visibly lacks for its own sake: today the only way for a run to see a file it did not write is to shell out through `exec`.

With it, the ablation this was blocked on: hold the task fixed, vary how much the catalog says — bare name, one-line disclosure, disclosure plus an expansion fetched on demand, everything eagerly — and measure which tool gets called, how often the wrong one gets called, and what each level costs in prefix bytes. Two of those four arms cannot be run at all until the expansion is fetchable.

- Per-building admission is the design: a building that does not handle mail is not given a mail server, and a general capability like search sits in the city layer that every building inherits. `city::config_layers` already resolves exactly those three layers, so the mechanism exists and only the reading of it is untested
- The first-level reading is recorded: Resident 106 B → 1,176 B for eight tools and one mode

## P4 — Enrolment answers before the credential is stored

`POST /enroll` returns 201 as soon as the command is posted to the desk, and the worker's refusal reaches nobody — recorded in `channels-SPEC.md` §8. `sprawling enrol` therefore reports "accepted", not "stored", which is honest but is not the answer a person wants.

The desk is not read while a dispatch is running, so a synchronous wait would hang the request for minutes. The shape that fits: subscribe to the event stream, post the command with a `Reply`, and answer whichever arrives first within a bounded wait — `secret_captured` naming this reference (201), a refusal (422), or neither (202, and say why).

- Lands in `crates/channels/` and `crates/sprawling/`; the positive answer is already an event, so nothing new has to be invented to carry it

## P5 — The live page cannot see anything that happened before it opened

The server broadcasts and never backfills, so the live feed and the ledger page both begin at the moment a page connects. Both say so now, and the overview reads the city's own counts rather than folding a window it just opened — but "say so" is the mitigation, not the fix. A `Query` that returns a bounded slice of history would let a person open a city and read what it did this morning.

- Lands in `crates/channels/`; `memory::index` already maps `seq` to a byte offset, which is the half that would otherwise be hard

## P6 — Claims made by construction rather than by observation

Neither is known to be wrong; neither has been watched.

- Nobody has unpacked the Linux archive on a Linux desktop and seen `start.sh` open a browser. `release.yml` builds it, `ci.yml` tests the tree that goes into it, and no human has run the result. **Split it**: the ubuntu job can prove the archive unpacks, `start.sh` computes the right URL, and the binary serves — what stays unproven is a person seeing a browser window, and the release notes say so rather than implying otherwise
- Ctrl-C on a served city is still a process death rather than an orderly close. `/quit` closes the console and leaves the city serving; `sprawling resume` already recovers a hard stop, so what is missing is the Handoff a clean shutdown would write
- `dist/start.cmd` and `dist/start.sh` lost their reason to exist when `sprawling install` landed. Removing them changes `xtask package`'s entry table — **needs a ruling**
