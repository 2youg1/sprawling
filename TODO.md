# TODO

Future work, highest priority first. A row names an outcome; its design lands in the SPEC of the crate that owns it before any code is written. A row leaves this file when the work is done, not when it is planned.

Where a row says **needs a ruling**, it touches something in the `guard` row of AGENTS.md and cannot start without the person's word, recorded as a `Verdict:` trailer.

## P0 — done

`sprawling install` landed and was verified in both directions on this machine. What is left of the row: `dist/start.cmd` and `dist/start.sh` have lost their reason to exist, and removing them changes `xtask package`'s entry table — **needs a ruling**.

## P1 — A console, so the terminal stops being a dead end

`sprawling up` prints four lines and then blocks until Ctrl-C. That terminal is a surface the product currently throws away, and it is the only surface a machine without a browser has at all.

Serving a city enters a console instead. A line beginning `/` is a control verb; any other line is work dispatched to the selected room. `/web` opens the WebUI and carries the pairing token, so nobody has to copy one. With no argument, the working directory is the city — the gesture `claude` and `git` already taught everybody.

- **The verb table is a projection of `channels::wire`'s `COMMAND_NAMES` and `QUERY_NAMES`, never a second hand-written list.** A hand-written list is a second vocabulary that drifts, and the drift is invisible
- The console decides nothing: every judgement stays server-side, exactly as the browser client's does
- Ctrl-C becomes an orderly close that writes a Handoff, rather than a death `sprawling resume` has to clean up
- Not a TTY, not interactive: fall back rather than hang
- Refuses to grow tables or pictures. Those belong to the browser, and serving two masters is what the CLI literature warns against

## P2 — done

The desk carries a reply address (`channels::Reply`), the worker hands every refusal to whoever asked, and the client draws it where that person is looking. Verified over a real socket against a real city: a `SelectModel` on an endpoint nobody attached came back as `E_CONFIG_INVALID` with its recovery line, in the same connection that sent it.

One instance of the same defect is left, recorded in `channels-SPEC.md` section 8: the `/enroll` route answers 201 before the worker has taken the credential, so a refusal there still reaches nobody. Bounding that wait belongs to P3.

## P3 — done

`sprawling call` and `sprawling enrol` landed. The handshake is computed from `channels::WIRE_V` and `channels::schema_hash()` in-process, so the throwaway probe that reimplemented both outside the workspace is deleted. Verified against a real city: enrolment from stdin, then attach, select, create and dispatch, all over `call`.

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

## P5 — done

The lifecycle the specification defines (`initialize`, then `notifications/initialized`) is one authority in `protocol::handshake`, above both transports; the HTTP transport keeps the session and the negotiated version, and forgets a session the server ended. A configured header redeems a `secret:` reference at the wire and nowhere earlier. Every MCP diagnostic now names the transport that actually failed.

Verified against a real hosted server: `exa is exa-search-server speaking 2025-06-18, offering 2 tool(s)`, after which the model called its search tool unprompted, read real results, and answered in one turn.

Two things only running found. A hosted server behind a content delivery network answers 403 `browser_signature_banned` to a client that will not name itself, before any MCP message is read. And a search server's results carry relevance scores, which the ledger's no-floats rule refused — four calls in a row died on `1249.4`. Fractional numbers from a server are now recorded as their own digits: this city governs what it sends and adapts what it receives.

## P6 — partly done: the model does use what it is given

**The earlier diagnosis was wrong and is withdrawn.** The 106-byte Resident segment is `city::resident::EPHEMERAL_SEGMENT` — "You have no standing identity…" — and has nothing to do with the catalog. Tools reach the model through `ChatRequest.tools`, and always did.

A real dispatch against a real provider called `status`, then `edit`, wrote the file it was asked for, reported, and froze `done` with evidence. What had actually killed the earlier run was a provider answering HTTP 200 with `"choices": null` when `max_tokens` exceeds the chosen model's ceiling, and a refusal whose `recovery` was the empty string. Both are fixed (`gateway-SPEC.md` section 8-1).

**The disclosure half is now wired.** `Catalog::render()` joins the Resident segment and `set_mode` is called, so the reading room's skills and the run's own mode reach a model for the first time — `city::library`'s admission was a gate with nothing downstream of it. Measured on one real dispatch: Resident 106 B → 1,176 B, and a model asked what it can reach names all eight tools plus the mode.

**What is left, and why.** Second-level disclosure is still unreachable: a skill's `expansion` is an address under the reserved prefix `.sprawling/`, and this build has no tool that reads a file — `edit` changes one, it does not read one. `render()` deliberately does not print that address, because telling a model to fetch something it cannot fetch is worse than not telling it.

So the remaining row is **a read tool**, which is a new capability rather than a defect in this one. Two of the ablation's four arms — "disclosure plus an expansion fetched on demand" and the comparison against it — need it before they can be run at all.

## P7 — Ship a Linux archive

`release.yml` verifies on Windows and builds archives for Windows and macOS. Linux is on hold by the person's ruling; bringing it back means adding `ubuntu-latest` to both the `verify` and `archive` matrices, once somebody has run the result on a Linux desktop and seen `start.sh` open a browser there.

The first CI run on Linux found `xtask/src/mem.rs` importing `std::process::Command` where nothing used it — the class of defect a host-only build never sees. Expect a few more of those before Linux is shippable.
