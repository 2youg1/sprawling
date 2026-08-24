# TODO

Future work, highest priority first. A row names an outcome; its design lands in the SPEC of the crate that owns it before any code is written. A row leaves this file when the work is done, not when it is planned.

Where a row says **needs a ruling**, it touches something in the `guard` row of AGENTS.md and cannot start without the person's word, recorded as a `Verdict:` trailer.

Where a row says **evidence**, the claim was checked by running something or by a search whose empty result is the finding. Rows without it are judgements, and are marked as such.

## P0 — A person who already has a workspace

**evidence**: `sprawling init` lays a city into a directory; `sprawling adopt <city-dir> <addr>` takes a directory that is *already inside a city* in as a building, and its own usage line says "move or clone the directory under the city first". Nothing takes a folder that exists somewhere else and makes it workable, and the first-run screen (`bin::firstrun`) offers only a new city.

The outcome: **a person points at the work they already have, and the city forms around it rather than beside it.** Two halves, and both have to hold the rule that a city owns its own subtree and nothing else:

1. `init` over a directory that is not empty must be a decision with an exhaustive answer — what it found, what it will lay down, what it will not touch — rather than a refusal or a silent overwrite.
2. The first screen and the wire need the same door, so a person who never opens a terminal gets there too. `adopt` already exists and is the shape to reuse; what is missing is the case where the folder is not yet under the city.

## P0.1 — Dragging is one gesture that means one thing

**evidence**: `grep -rn "ondrop\|ondragover\|drag" crates/web/src` is empty. The client has no drag handling at all; attachments reach the city through the upload route and a form.

The outcome: **a person who works by dragging can start work, file something, and move work between rooms without typing an address.** Each gesture needs its own answer to "what did that mean", and the honest ones are few: a file dropped on a room becomes an attachment for that room; a file dropped on a building becomes something filed there; a run dragged onto a room is not a move and must say so rather than pretending. A gesture whose meaning this build cannot name is refused with the reason, never guessed.

## P4 — The ledger of numbers

1. **Two size readings have drifted past their slack.** `frontend_artifact` measures 524,254 B against a best of 490,594 B, and `release_binary` 8,317,952 B against 7,249,920 B. The likely cause is `web::lang`'s two-language table (F2.14, F2.18) and the binary that embeds it. `just check` does not build either artifact, so CI is not red; a `just dist` is. Rebuild both, then either recover the size or record the readings with the reason. **needs a ruling**: `xtask/budgets.toml`.

## P5 — Claims made by construction rather than by observation

Not known to be wrong; not watched.

- Nobody has unpacked the Linux archive on a Linux desktop and watched the binary open a browser. `release.yml` builds it, `ci.yml` tests the tree that goes into it, and no human has run the result. **Split it**: the ubuntu job can prove the archive unpacks and the binary serves — what stays unproven is a person seeing a browser window, and the release notes say so rather than implying otherwise.
- The Chinese interface has been read on a headless Edge at three window sizes and by nobody in a real browser. What a screenshot cannot show: whether the wording is right to somebody who did not write it.
