# TODO

Future work, highest priority first. A row names an outcome; its design lands in the SPEC of the crate that owns it before any code is written. A row leaves this file when the work is done, not when it is planned.

Where a row says **needs a ruling**, it touches something in the `guard` row of AGENTS.md and cannot start without the person's word, recorded as a `Verdict:` trailer.

## P0 — Make `sprawling` a word you can type

Unpacking the archive leaves a binary that is not on any search path, so the only way in is to find the folder and double-click `start.cmd`. Finding a script is worse than typing a command, and a desktop entry is worse than both.

`sprawling install` copies the running binary into the per-user program directory and puts that directory on the user's `PATH`; `--uninstall` reverses exactly what it did. No administrator rights, because nothing outside the user's own profile is touched. Package manifests (scoop, winget, brew) come after, not instead.

- Lands in `crates/sprawling/`, one new module registered in the module map before it is written
- Verified by installing on this machine, opening a **new** shell, and running `sprawling` with no arguments
- When it lands, `dist/start.cmd` and `dist/start.sh` lose their reason to exist; removing them changes `xtask package`'s entry table — **needs a ruling**

## P1 — A console, so the terminal stops being a dead end

`sprawling up` prints four lines and then blocks until Ctrl-C. That terminal is a surface the product currently throws away, and it is the only surface a machine without a browser has at all.

Serving a city enters a console instead. A line beginning `/` is a control verb; any other line is work dispatched to the selected room. `/web` opens the WebUI and carries the pairing token, so nobody has to copy one. With no argument, the working directory is the city — the gesture `claude` and `git` already taught everybody.

- **The verb table is a projection of `channels::wire`'s `COMMAND_NAMES` and `QUERY_NAMES`, never a second hand-written list.** A hand-written list is a second vocabulary that drifts, and the drift is invisible
- The console decides nothing: every judgement stays server-side, exactly as the browser client's does
- Ctrl-C becomes an orderly close that writes a Handoff, rather than a death `sprawling resume` has to clean up
- Not a TTY, not interactive: fall back rather than hang
- Refuses to grow tables or pictures. Those belong to the browser, and serving two masters is what the CLI literature warns against

## P2 — A refusal has to reach whoever asked for it

`assembly.rs:3471` is `let _ = worker.handle(command);`. A refused command is written to the diagnostic log and **nothing is sent to the client**, so both the WebUI and any future console are blind: a person presses "attach" and the page says nothing at all.

This was found by driving a real city over its own wire. Two commands were refused — a base URL missing `/v1`, and the consequent failure to select a model — and the only place either appeared was the server's own log file.

Bigger than it looks: commands travel socket → desk → worker thread, and the return path carries only `EventRecord`. There is no channel on which an `AxError` can travel back. A refusal belongs to the peer that caused it, so the desk has to carry a reply address rather than the refusal being broadcast to everyone watching.

- Lands in `crates/sprawling/` and `crates/channels/`; `ServerFrame::Refusal` already exists and is already sent for socket-level refusals
- The `let _ =` binding on a `Result` in production code is itself the defect the hardening rules exist to catch; whatever replaces it must not reintroduce one

## P3 — `sprawling call` and `sprawling enrol`

The scriptable half, and the one an agent can drive. `call` sends one `ClientFrame` and prints every `ServerFrame` until the city goes quiet, computing the handshake in-process. `enrol` reads a credential from **stdin**, never from `argv`, and posts it to the local enrolment route.

Both exist because the wire is supposed to be the whole API. Today that claim has one client, which makes `channels::wire` a hypothetical seam by the repository's own test in ARCHITECTURE.md §4; a second client is what makes it real.

`enrol` is also better custody than what exists: the browser path requires the page to hold the plaintext key in memory first.

- Lands in `crates/sprawling/`
- Deletes the throwaway probe that had to reimplement `schema_hash` and `IdemKey::derive` outside the workspace to do this once

## P4 — Redo the front end

Rebuild the client's appearance from the ground up rather than adjusting it. The client is correct today and does not yet look like something a person wants to keep open all day. Everything else in this project is judged through it, so this is not cosmetic.

Ordered, because each step needs the one above it:

1. **Routing.** `View` becomes a URL. Without it there is no deep link, no browser back, no bookmark, and no way to photograph any page but the first — which is also why the front end cannot be regression-tested today.
2. **The isometric city, in SVG rather than canvas.** `DisplayList` was designed as a shape table so a second renderer could consume it, and `Face { id, token, points }` is an SVG polygon already. Canvas is a fixed 1000×560 bitmap that is then CSS-scaled, so the picture and its text are resampled on every display that is not exactly that size; canvas also cannot read a CSS custom property, which is why the selection outline settles for `G10` where `city_view.rs:703` plainly wanted `--ACCENT`. SVG restores hover, focus, keyboard reach, transitions, and a city that fills whatever space it is given. **Contradicts a recorded decision** — web-SPEC §3's "canvas, not DOM" — whose stated reason is a thousand Residents; a city holds tens of Buildings, so the reason does not reach this case. The SPEC is corrected first, with that reasoning
3. **Space, and a card grammar.** Roughly 45% of the window is empty while the canvas states three facts an adjacent list states better. Every centre panel gets a conclusion heading, a subtitle carrying scope and legend, the body, and a line saying where the numbers came from — the last of which is missing from a product whose whole claim is an auditable Ledger
4. **`web::lang`, and a font.** An exhaustive `Msg` enum makes a missing translation fail to compile. No font file ships today: the stylesheet names Noto Sans SC and Zen Kaku Gothic New and relies on the reader's machine owning them. Embedding a subset is the only change in sight that could approach the 2 MB client budget, so it is measured before it is chosen. Making it stick needs a gate — **needs a ruling**
5. **Settings, two levels.** Models / City / Interface / Security / About. The City pane needs Queries the wire does not have yet, so `WIRE_V` goes from 4 to 5
6. **The repetitions.** `Root` takes nineteen parameters, ten of which are one concept; eight modules each hold their own copy of the "ask once the socket is live" dance; `city_view.rs:703` sets a stroke style that the next line overwrites

- Lands in `crates/web/`; colour comes from `web::theme` and nowhere else, which the `color` gate holds
- Decisions go into `crates/web/web-SPEC.md` first

## P5 — Reach a real MCP server

Configuring Exa's hosted endpoint and dispatching a run against it answered `-32601: Method not found`. The reason is recorded in our own source: `protocol/src/mcp.rs:17` says there is no protocol-level session, and `mcp_http.rs:24` says HTTP carries none. The MCP streamable-HTTP transport requires `initialize`, then `notifications/initialized`, then a session header on every later call.

So the stateless adapter can speak to a shrinking minority of servers. This is not an Exa special case and must not be built as one: the outcome is that any streamable-HTTP MCP server can be reached, which is what ARCHITECTURE.md §8 already promises about this whole class of integration.

- Lands in `crates/protocol/` and `crates/sprawling/`; the "no session" decision is corrected in the SPEC first, with the specification as its reason
- `McpTransport::Http`'s `header` does not redeem `secret:` references, so a paid key would sit in plaintext in a building's `CONFIG.toml`. That contradicts the credential rule and `xtask secret` cannot see it, because a city's config is not in this repository
- Also here: `assembly.rs:1925` and `1936` label every MCP failure `bin::mcp_stdio`, including failures of the HTTP transport, which sends a reader to the wrong file

## P6 — Find out whether a model will use what it is given

A dispatched run assembled a prefix whose Resident segment was **106 bytes** — about the length of the catalog's header sentence alone. No tool was disclosed to the model, not the three built-in ones and not any from a server. Until that is understood, connecting anything is pointless: a tool the model is never told about does not exist to it.

After that, the ablation: hold the task fixed, vary how much the catalog says — bare name, one-line disclosure, disclosure plus an expansion fetched on demand, everything eagerly — and measure which tool gets called, how often the wrong one gets called, and what each level costs in prefix bytes. `runtime::catalog` already implements two-level disclosure, which is the arrangement the published work recommends; nobody has yet measured whether it works here.

Per-building admission is the design: a building that does not handle mail is not given a mail server, and a general capability like search sits in the city layer that every building inherits. `city::config_layers` already resolves exactly those three layers, so the mechanism exists and only the reading of it is untested.

- Needs P3 to drive it and P5 to have anything external to call
- A catalog budget belongs in `xtask/budgets.toml` once there is a number

## P7 — Ship a Linux archive

`release.yml` verifies on Windows and builds archives for Windows and macOS. Linux is on hold by the person's ruling; bringing it back means adding `ubuntu-latest` to both the `verify` and `archive` matrices, once somebody has run the result on a Linux desktop and seen `start.sh` open a browser there.

The first CI run on Linux found `xtask/src/mem.rs` importing `std::process::Command` where nothing used it — the class of defect a host-only build never sees. Expect a few more of those before Linux is shippable.
