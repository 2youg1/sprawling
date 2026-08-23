# TODO

Future work, highest priority first. A row names an outcome; its design lands in the SPEC of the crate that owns it before any code is written. A row leaves this file when the work is done, not when it is planned.

Where a row says **needs a ruling**, it touches something in the `guard` row of AGENTS.md and cannot start without the person's word, recorded as a `Verdict:` trailer.

## P1 — Redo the front end

Rebuild the client's appearance from the ground up rather than adjusting it. The client is correct today and does not yet look like something a person wants to keep open all day. Everything else in this project is judged through it, so this is not cosmetic.

Ordered, because each step needs the one above it:

1. **Routing.** `View` becomes a URL. Without it there is no deep link, no browser back, no bookmark, and no way to photograph any page but the first — which is also why the front end cannot be regression-tested today.
2. **The isometric city, in SVG rather than canvas.** `DisplayList` was designed as a shape table so a second renderer could consume it, and `Face { id, token, points }` is an SVG polygon already. Canvas is a fixed 1000×560 bitmap that is then CSS-scaled, so the picture and its text are resampled on every display that is not exactly that size; canvas also cannot read a CSS custom property, which is why the selection outline settles for `G10` where `city_view.rs:703` plainly wanted `--ACCENT`. SVG restores hover, focus, keyboard reach, transitions, and a city that fills whatever space it is given. **Contradicts a recorded decision** — web-SPEC §3's "canvas, not DOM" — whose stated reason is a thousand Residents; a city holds tens of Buildings, so the reason does not reach this case. The SPEC is corrected first, with that reasoning
3. **Space, and a card grammar.** Roughly 45% of the window is empty while the canvas states three facts an adjacent list states better. Every centre panel gets a conclusion heading, a subtitle carrying scope and legend, the body, and a line saying where the numbers came from — the last of which is missing from a product whose whole claim is an auditable Ledger
4. **`web::lang`, and a font.** An exhaustive `Msg` enum makes a missing translation fail to compile. No font file ships today: the stylesheet names Noto Sans SC and Zen Kaku Gothic New and relies on the reader's machine owning them. Embedding a subset is the only change in sight that could approach the 2 MB client budget, so it is measured before it is chosen. Making it stick needs a gate — **needs a ruling**
5. **Settings, two levels.** Models / City / Interface / Security / About. The City pane needs Queries the wire does not have yet, so `WIRE_V` goes from 4 to 5
6. **The repetitions.** `Root` takes twenty-one parameters, twelve of which are one concept; eight modules each hold their own copy of the "ask once the socket is live" dance; `city_view.rs:703` sets a stroke style that the next line overwrites

- Lands in `crates/web/`; colour comes from `web::theme` and nowhere else, which the `color` gate holds
- Decisions go into `crates/web/web-SPEC.md` first

## P2 — A tool that reads a file, and the catalog ablation behind it

The catalog now reaches the model (`runtime-SPEC.md` §8-11), but only its first level. A skill's `expansion` is an address under the reserved prefix `.sprawling/`, and no tool in this build reads a file — `edit` changes one, it does not read one. So the reading room can name a skill and can never hand it over, and `Catalog::expand` has no path to a caller.

A read tool closes that, and it is a capability the model visibly lacks for its own sake: today the only way for a run to see a file it did not write is to shell out through `exec`.

With it, the ablation this was blocked on: hold the task fixed, vary how much the catalog says — bare name, one-line disclosure, disclosure plus an expansion fetched on demand, everything eagerly — and measure which tool gets called, how often the wrong one gets called, and what each level costs in prefix bytes. Two of those four arms cannot be run at all until the expansion is fetchable.

- Per-building admission is the design: a building that does not handle mail is not given a mail server, and a general capability like search sits in the city layer that every building inherits. `city::config_layers` already resolves exactly those three layers, so the mechanism exists and only the reading of it is untested
- The first-level reading is recorded: Resident 106 B → 1,176 B for eight tools and one mode

## P3 — Enrolment answers before the credential is stored

`POST /enroll` returns 201 as soon as the command is posted to the desk, and the worker's refusal reaches nobody — the one instance of the P2 defect that is still open, recorded in `channels-SPEC.md` §8. `sprawling enrol` therefore reports "accepted", not "stored", which is honest but is not the answer a person wants.

The desk is not read while a dispatch is running, so a synchronous wait would hang the request for minutes. The shape that fits: subscribe to the event stream, post the command with a `Reply`, and answer whichever arrives first within a bounded wait — `secret_captured` naming this reference (201), a refusal (422), or neither (202, and say why).

- Lands in `crates/channels/` and `crates/sprawling/`; the positive answer is already an event, so nothing new has to be invented to carry it

## P4 — Claims made by construction rather than by observation

Neither is known to be wrong; neither has been watched.

- Nobody has unpacked the Linux archive on a Linux desktop and seen `start.sh` open a browser. `release.yml` builds it, `ci.yml` tests the tree that goes into it, and no human has run the result
- Ctrl-C on a served city is still a process death rather than an orderly close. `/quit` closes the console and leaves the city serving; `sprawling resume` already recovers a hard stop, so what is missing is the Handoff a clean shutdown would write
- `dist/start.cmd` and `dist/start.sh` lost their reason to exist when `sprawling install` landed. Removing them changes `xtask package`'s entry table — **needs a ruling**
